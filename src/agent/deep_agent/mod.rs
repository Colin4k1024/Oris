//! Deep Agent mode: planning, file system tools, subagent task tool, long-term memory.
//!
//! Aligned with [LangChain Deep Agents](https://docs.langchain.com/oss/python/deepagents/overview).
//! For customization (model, system prompt, tools, context, middleware, skills, memory), see
//! [Customize Deep Agents](https://docs.langchain.com/oss/python/deepagents/customization).
//!
//! **Subagents**: When the task tool is enabled, the main agent can delegate via the `task` tool.
//! See [Subagents](https://docs.langchain.com/oss/python/deepagents/subagents). A built-in
//! general-purpose subagent (same prompt and tools as main) is always included when using
//! [create_deep_agent]; add custom subagents with [DeepAgentConfig::with_subagent]. Use
//! `subagent_id` (e.g. `"general-purpose"` or a custom name) and `input` or `task_description`
//! when calling the task tool.
//!
//! **File backends**: FS tools (ls, read_file, write_file, edit_file, glob, grep) use an optional
//! [FileBackend]. When none is set, paths are resolved from the workspace root (config or context).
//! See [Backends](https://docs.langchain.com/oss/python/deepagents/backends) and the [backends]
//! module for [WorkspaceBackend], [StoreBackend], and [CompositeBackend] (prefix-based routing).
//!
//! **Long-term memory**: Use [DeepAgentConfig::with_long_term_memory] so that paths under a prefix
//! (e.g. `/memories/`) are stored in the [ToolStore] and persist across threads; see [Long-term memory](https://docs.langchain.com/oss/python/deepagents/long-term-memory).

mod config;
pub use config::{DeepAgentConfig, SubagentSpec};

pub mod backends;
pub mod skills;
pub use backends::{CompositeBackend, FileBackend, FileInfo, StoreBackend, WorkspaceBackend};

pub mod tools;
pub use tools::fs::FileSystemToolError;
pub use tools::{
    EditFileTool, GlobTool, GrepTool, LsTool, ReadFileTool, TaskTool, TodoItem, TodoStatus,
    WriteFileTool, WriteTodosTool,
};

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::middleware::{HumanInTheLoopMiddleware, ToolResultEvictionMiddleware};
use crate::agent::InterruptConfig;
use crate::agent::{
    create_agent_with_runtime, create_agent_with_runtime_from_llm, AgentError, Middleware,
    SubagentTool, UnifiedAgent,
};
use crate::language_models::llm::LLM;
use crate::tools::{InMemoryStore, SimpleContext, Tool, ToolContext, ToolStore};

/// Default system prompt when none is provided: describes deep agent built-in tools.
const DEFAULT_DEEP_AGENT_SYSTEM_PROMPT: &str = "You are a deep agent with planning and optional file system and task tools. \
Use write_todos to break complex tasks into steps and track progress. \
Use ls, read_file, write_file, edit_file, glob, and grep to work inside the workspace when available. \
Use the task tool to delegate to specialized subagents when configured.";

/// Name of the built-in general-purpose subagent. Always present when the task tool is enabled.
/// See [Subagents – The general-purpose subagent](https://docs.langchain.com/oss/python/deepagents/subagents#the-general-purpose-subagent).
pub const GENERAL_PURPOSE_SUBAGENT_NAME: &str = "general-purpose";

/// Description for the built-in general-purpose subagent.
const GENERAL_PURPOSE_SUBAGENT_DESCRIPTION: &str = "General-purpose subagent for context isolation. Same system prompt and tools as the main agent; use for multi-step tasks that would clutter the main context.";

/// Build a subagent spec from a model string, name, description, system prompt, and tools.
///
/// Convenience for the dictionary-style subagent pattern: instead of building an agent manually
/// and calling [DeepAgentConfig::with_subagent], use this then pass the result to `with_subagent`.
/// See [Subagents – SubAgent (Dictionary-based)](https://docs.langchain.com/oss/python/deepagents/subagents#subagent-dictionary-based).
pub fn build_subagent_spec(
    model: &str,
    name: impl Into<String>,
    description: impl Into<String>,
    system_prompt: impl AsRef<str>,
    tools: Vec<Arc<dyn Tool>>,
) -> Result<SubagentSpec, AgentError> {
    let agent = crate::agent::create_agent(model, &tools, Some(system_prompt.as_ref()), None)?;
    Ok(SubagentSpec {
        agent: Arc::new(agent),
        name: name.into(),
        description: description.into(),
        interrupt_on: None,
    })
}

/// Load skill and memory sections from config (paths + inline contents).
fn load_skills_and_memory_sections(
    config: &DeepAgentConfig,
) -> Result<(String, String), AgentError> {
    let mut skill_parts: Vec<String> = Vec::new();
    for path in &config.skill_paths {
        let content = fs::read_to_string(path).map_err(|e| {
            AgentError::OtherError(format!(
                "Failed to read skill file {}: {}",
                path.display(),
                e
            ))
        })?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("skill");
        skill_parts.push(format!("### {}\n{}", name, content));
    }
    for (name, content) in &config.skill_contents {
        skill_parts.push(format!("### {}\n{}", name, content));
    }
    let skills_section = if skill_parts.is_empty() {
        String::new()
    } else {
        format!("\n\n## Skills\n\n{}", skill_parts.join("\n\n"))
    };

    let mut memory_parts: Vec<String> = Vec::new();
    for path in &config.memory_paths {
        let content = fs::read_to_string(path).map_err(|e| {
            AgentError::OtherError(format!(
                "Failed to read memory file {}: {}",
                path.display(),
                e
            ))
        })?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("memory");
        memory_parts.push(format!("### {}\n{}", name, content));
    }
    for (name, content) in &config.memory_contents {
        memory_parts.push(format!("### {}\n{}", name, content));
    }
    let memory_section = if memory_parts.is_empty() {
        String::new()
    } else {
        format!("\n\n## Memory\n\n{}", memory_parts.join("\n\n"))
    };

    Ok((skills_section, memory_section))
}

/// Build context: use config.context if set, otherwise SimpleContext with workspace_root.
fn build_context(config: &DeepAgentConfig) -> Arc<dyn ToolContext> {
    if let Some(ref ctx) = config.context {
        return Arc::clone(ctx);
    }
    let mut ctx = SimpleContext::new();
    if let Some(ref root) = config.workspace_root {
        ctx = ctx.with_custom("workspace_root".to_string(), root.display().to_string());
    }
    Arc::new(ctx)
}

/// Build middleware list: optional skills (progressive disclosure), tool result eviction, HumanInTheLoop from interrupt_on and interrupt_before_tools, then config.middleware.
fn build_middleware(config: &DeepAgentConfig) -> Option<Vec<Arc<dyn Middleware>>> {
    let mut list: Vec<Arc<dyn Middleware>> = Vec::new();
    if !config.skill_dirs.is_empty() {
        if let Ok(Some(skills_mw)) =
            crate::agent::middleware::build_skills_middleware(&config.skill_dirs)
        {
            list.push(skills_mw);
        } else {
            log::warn!("Failed to load skill index from skill_dirs");
        }
    }
    if let Some(limit) = config.evict_tool_result_over_tokens {
        list.push(Arc::new(
            ToolResultEvictionMiddleware::new().with_token_limit(Some(limit)),
        ));
    }
    let mut interrupt_map = config.interrupt_on.clone();
    for name in &config.interrupt_before_tools {
        interrupt_map
            .entry(name.clone())
            .or_insert_with(InterruptConfig::enabled);
    }
    if !interrupt_map.is_empty() {
        let mut hitl = HumanInTheLoopMiddleware::new().with_approval_required_for_tool_calls(false);
        for (name, cfg) in interrupt_map {
            hitl = hitl.with_interrupt_config(name, cfg);
        }
        list.push(Arc::new(hitl));
    }
    if let Some(ref mw) = config.middleware {
        list.extend(mw.iter().cloned());
    }
    if list.is_empty() {
        None
    } else {
        Some(list)
    }
}

/// Returns the file backend to use: when [DeepAgentConfig::long_term_memory_prefix] is set, builds a
/// [CompositeBackend] that routes that prefix to [StoreBackend](store); otherwise returns [DeepAgentConfig::file_backend].
fn effective_file_backend(
    config: &DeepAgentConfig,
    store: &Arc<dyn ToolStore>,
) -> Option<Arc<dyn FileBackend>> {
    let prefix = match &config.long_term_memory_prefix {
        Some(p) => p.clone(),
        None => return config.file_backend.clone(),
    };
    let default: Arc<dyn FileBackend> = config
        .file_backend
        .clone()
        .or_else(|| {
            config
                .workspace_root
                .clone()
                .map(|r: PathBuf| Arc::new(WorkspaceBackend::new(r)) as Arc<dyn FileBackend>)
        })
        .unwrap_or_else(|| Arc::new(WorkspaceBackend::new(std::env::temp_dir())));
    let composite = CompositeBackend::new(default)
        .with_route(prefix, Arc::new(StoreBackend::new(Arc::clone(store))));
    Some(Arc::new(composite))
}

/// Build all tools (user + built-in from config), final system prompt, context, store, middleware.
fn build_deep_agent_parts(
    tools: &[Arc<dyn Tool>],
    system_prompt: Option<&str>,
    config: &DeepAgentConfig,
) -> Result<
    (
        Vec<Arc<dyn Tool>>,
        String,
        Arc<dyn ToolContext>,
        Arc<dyn ToolStore>,
        Option<Vec<Arc<dyn Middleware>>>,
    ),
    AgentError,
> {
    let (skills_section, memory_section) = load_skills_and_memory_sections(config)?;

    let base_prompt = system_prompt
        .map(|s| s.to_string())
        .unwrap_or_else(|| DEFAULT_DEEP_AGENT_SYSTEM_PROMPT.to_string());
    let mut full_system_prompt = base_prompt;
    if config.enable_planning {
        if let Some(ref s) = config.planning_system_prompt {
            full_system_prompt.push_str("\n\n");
            full_system_prompt.push_str(s);
        }
    }
    if config.enable_filesystem {
        if let Some(ref s) = config.filesystem_system_prompt {
            full_system_prompt.push_str("\n\n");
            full_system_prompt.push_str(s);
        }
    }
    full_system_prompt.push_str(&skills_section);
    full_system_prompt.push_str(&memory_section);

    fn maybe_wrap_tool(
        tool: Arc<dyn Tool>,
        custom_descriptions: &std::collections::HashMap<String, String>,
    ) -> Arc<dyn Tool> {
        let name = tool.name();
        if let Some(desc) = custom_descriptions.get(&name) {
            Arc::new(tools::ToolWithCustomDescription::with_description(
                tool,
                desc.clone(),
            ))
        } else {
            tool
        }
    }

    let mut all_tools: Vec<Arc<dyn Tool>> = tools.to_vec();

    if config.enable_planning {
        let t = Arc::new(tools::WriteTodosTool::new());
        all_tools.push(maybe_wrap_tool(t, &config.custom_tool_descriptions));
    }

    if config.enable_filesystem {
        let wr = config.workspace_root.clone();
        let t = Arc::new(tools::LsTool::new().maybe_workspace_root(wr.clone()));
        all_tools.push(maybe_wrap_tool(t, &config.custom_tool_descriptions));
        let t = Arc::new(tools::ReadFileTool::new().maybe_workspace_root(wr.clone()));
        all_tools.push(maybe_wrap_tool(t, &config.custom_tool_descriptions));
        let t = Arc::new(tools::WriteFileTool::new().maybe_workspace_root(wr.clone()));
        all_tools.push(maybe_wrap_tool(t, &config.custom_tool_descriptions));
        let t = Arc::new(tools::EditFileTool::new().maybe_workspace_root(wr.clone()));
        all_tools.push(maybe_wrap_tool(t, &config.custom_tool_descriptions));
        let t = Arc::new(tools::GlobTool::new().maybe_workspace_root(wr.clone()));
        all_tools.push(maybe_wrap_tool(t, &config.custom_tool_descriptions));
        let t = Arc::new(tools::GrepTool::new().maybe_workspace_root(wr));
        all_tools.push(maybe_wrap_tool(t, &config.custom_tool_descriptions));
    }

    // Task tool is not added here; callers (create_deep_agent / create_deep_agent_from_llm)
    // add it when enable_task_tool, with general-purpose + config.subagents.

    let store: Arc<dyn ToolStore> = config
        .store
        .clone()
        .unwrap_or_else(|| Arc::new(InMemoryStore::new()));

    let context = build_context(config);
    let middleware = build_middleware(config);

    Ok((all_tools, full_system_prompt, context, store, middleware))
}

/// Create a Deep Agent with planning, optional file system and task tools, and store.
///
/// Uses `create_agent_with_runtime` under the hood. If `config.store` is None,
/// uses a default `InMemoryStore`. When `enable_task_tool` is true, a general-purpose
/// subagent (same prompt and tools as main, no task tool) is always included in the
/// task tool alongside any custom subagents from `config.subagents`.
/// See [Subagents](https://docs.langchain.com/oss/python/deepagents/subagents).
pub fn create_deep_agent(
    model: &str,
    tools: &[Arc<dyn Tool>],
    system_prompt: Option<&str>,
    config: DeepAgentConfig,
) -> Result<UnifiedAgent, AgentError> {
    let (base_tools, full_prompt, context, store, middleware) =
        build_deep_agent_parts(tools, system_prompt, &config)?;
    let file_backend = effective_file_backend(&config, &store);

    let all_tools: Vec<Arc<dyn Tool>> = if config.enable_task_tool {
        let general_purpose_agent = create_agent_with_runtime(
            model,
            &base_tools,
            Some(&full_prompt),
            Some(context.clone()),
            Some(store.clone()),
            None,
            middleware.clone(),
            file_backend.clone(),
        )?;
        let mut subagent_tools: Vec<SubagentTool> = vec![SubagentTool::new(
            Arc::new(general_purpose_agent),
            GENERAL_PURPOSE_SUBAGENT_NAME.to_string(),
            GENERAL_PURPOSE_SUBAGENT_DESCRIPTION.to_string(),
        )];
        for s in &config.subagents {
            subagent_tools.push(SubagentTool::new(
                Arc::clone(&s.agent),
                s.name.clone(),
                s.description.clone(),
            ));
        }
        let mut tools_with_task = base_tools;
        tools_with_task.push(Arc::new(TaskTool::from_subagent_tools(subagent_tools)));
        tools_with_task
    } else {
        base_tools
    };

    let mut agent = create_agent_with_runtime(
        model,
        &all_tools,
        Some(&full_prompt),
        Some(context),
        Some(store),
        config.response_format,
        middleware,
        file_backend,
    )?;
    if let Some(ref cp) = config.checkpointer {
        agent = agent.with_checkpointer(Some(Arc::clone(cp)));
    }
    Ok(agent)
}

/// Create a Deep Agent from an existing LLM instance (custom model configuration).
///
/// Same as [create_deep_agent] but uses the provided LLM instead of a model string.
/// When `enable_task_tool` is true, subagents are: a general-purpose subagent (only when
/// `config.default_subagent_model` is set; it uses that model string) plus any
/// `config.subagents`. If `default_subagent_model` is None, the general-purpose subagent
/// is not added (the LLM can only be used once); set `default_subagent_model` to get
/// the general-purpose subagent when using a custom LLM.
pub fn create_deep_agent_from_llm<L: Into<Box<dyn LLM>>>(
    llm: L,
    tools: &[Arc<dyn Tool>],
    system_prompt: Option<&str>,
    config: DeepAgentConfig,
) -> Result<UnifiedAgent, AgentError> {
    let (base_tools, full_prompt, context, store, middleware) =
        build_deep_agent_parts(tools, system_prompt, &config)?;
    let file_backend = effective_file_backend(&config, &store);

    let all_tools: Vec<Arc<dyn Tool>> = if config.enable_task_tool {
        let mut subagent_tools: Vec<SubagentTool> = Vec::new();
        if let Some(ref model_str) = config.default_subagent_model {
            let general_purpose_agent = create_agent_with_runtime(
                model_str,
                &base_tools,
                Some(&full_prompt),
                Some(context.clone()),
                Some(store.clone()),
                None,
                middleware.clone(),
                file_backend.clone(),
            )?;
            subagent_tools.push(SubagentTool::new(
                Arc::new(general_purpose_agent),
                GENERAL_PURPOSE_SUBAGENT_NAME.to_string(),
                GENERAL_PURPOSE_SUBAGENT_DESCRIPTION.to_string(),
            ));
        }
        for s in &config.subagents {
            subagent_tools.push(SubagentTool::new(
                Arc::clone(&s.agent),
                s.name.clone(),
                s.description.clone(),
            ));
        }
        if subagent_tools.is_empty() {
            base_tools
        } else {
            let mut tools_with_task = base_tools;
            tools_with_task.push(Arc::new(TaskTool::from_subagent_tools(subagent_tools)));
            tools_with_task
        }
    } else {
        base_tools
    };

    let mut agent = create_agent_with_runtime_from_llm(
        llm,
        &all_tools,
        Some(&full_prompt),
        Some(context),
        Some(store),
        config.response_format,
        middleware,
        file_backend,
    )?;
    if let Some(ref cp) = config.checkpointer {
        agent = agent.with_checkpointer(Some(Arc::clone(cp)));
    }
    Ok(agent)
}
