//! Configuration for Deep Agent mode.
//!
//! Controls planning (write_todos), file system tools, task (subagent) tools,
//! long-term memory (store), and customization (context, middleware, skills, memory).
//!
//! See [Customize Deep Agents](https://docs.langchain.com/oss/python/deepagents/customization).
//!
//! **Subagents**: When the task tool is enabled, a general-purpose subagent is included (for
//! [create_deep_agent]); add custom subagents with [DeepAgentConfig::with_subagent]. The main
//! agent uses each subagent's description to choose when to delegate. See [Subagents](https://docs.langchain.com/oss/python/deepagents/subagents).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::{AgentCheckpointer, InterruptConfig, Middleware, UnifiedAgent};
use crate::schemas::StructuredOutputStrategy;
use crate::tools::{FileBackend, ToolContext, ToolStore};

/// Configuration for creating a Deep Agent.
///
/// Enables or disables planning, file system tools, and task (subagent) tools.
/// Optionally sets workspace root, store, context, middleware, response format,
/// and skills/memory for prompt injection. Aligns with the official
/// [Deep Agents Middleware](https://docs.langchain.com/oss/python/deepagents/middleware):
/// planning + optional system prompt ≈ TodoListMiddleware; filesystem + optional prompt/descriptions ≈ FilesystemMiddleware;
/// task tool + subagents + general-purpose ≈ SubAgentMiddleware.
///
/// Note: `Clone` is not implemented because `response_format` uses `Box<dyn StructuredOutputStrategy>`.
pub struct DeepAgentConfig {
    /// Enable the write_todos tool for task planning and decomposition.
    pub enable_planning: bool,
    /// Enable file system tools (ls, read_file, write_file, edit_file).
    pub enable_filesystem: bool,
    /// Enable the task tool for delegating to subagents.
    pub enable_task_tool: bool,
    /// Root path for file system tools; all paths are relative to this.
    /// Can also be provided via ToolContext::get("workspace_root").
    pub workspace_root: Option<PathBuf>,
    /// Subagents to expose via the task tool: (agent, name, description).
    #[allow(clippy::type_complexity)]
    pub subagents: Vec<SubagentSpec>,
    /// Store for long-term memory and todos. If None, InMemoryStore is used.
    pub store: Option<Arc<dyn ToolStore>>,
    /// Custom context (session_id, user_id, etc.). If set, used instead of building from workspace_root.
    pub context: Option<Arc<dyn ToolContext>>,
    /// Middleware to apply during execution.
    pub middleware: Option<Vec<Arc<dyn Middleware>>>,
    /// Optional structured output strategy.
    pub response_format: Option<Box<dyn StructuredOutputStrategy>>,
    /// Paths to skill files (e.g. .md); contents are read and appended to system prompt under "## Skills".
    pub skill_paths: Vec<PathBuf>,
    /// Inline skill (name, content) pairs; appended to system prompt under "## Skills".
    pub skill_contents: Vec<(String, String)>,
    /// Skill directories for progressive disclosure: each dir must contain SKILL.md with frontmatter (name, description).
    /// Only frontmatter is read at startup; full content is loaded and injected when the user message matches.
    pub skill_dirs: Vec<PathBuf>,
    /// Paths to memory files (e.g. AGENTS.md); contents appended under "## Memory".
    pub memory_paths: Vec<PathBuf>,
    /// Inline memory (name, content) pairs; appended under "## Memory".
    pub memory_contents: Vec<(String, String)>,
    /// Tool names before which to require human-in-the-loop approval (e.g. "write_file", "edit_file").
    /// When [Self::interrupt_on] is also set, both are merged: interrupt_before_tools act as enabled with default decisions.
    pub interrupt_before_tools: Vec<String>,
    /// Per-tool interrupt config (enable + allowed_decisions). Takes precedence; [Self::interrupt_before_tools] fills in tools not listed here.
    pub interrupt_on: HashMap<String, InterruptConfig>,
    /// When set, evict tool results larger than this many (estimated) tokens to the store; None = disabled.
    pub evict_tool_result_over_tokens: Option<usize>,
    /// When using [crate::agent::deep_agent::create_deep_agent_from_llm] with `enable_task_tool`,
    /// optional model string for the general-purpose subagent. When set, the general-purpose
    /// subagent is built with this model; when None, the general-purpose subagent is not added
    /// (the provided LLM is used only for the main agent).
    pub default_subagent_model: Option<String>,
    /// Optional file backend for FS tools; when None, tools use workspace_root from config/context.
    pub file_backend: Option<Arc<dyn FileBackend>>,
    /// Optional checkpointer for human-in-the-loop (required for interrupt/resume; use same thread_id).
    pub checkpointer: Option<Arc<dyn AgentCheckpointer>>,
    /// Optional extra system prompt when planning is enabled (TodoListMiddleware-style).
    pub planning_system_prompt: Option<String>,
    /// Optional extra system prompt when filesystem is enabled (FilesystemMiddleware-style).
    pub filesystem_system_prompt: Option<String>,
    /// Override tool description by tool name (e.g. "ls", "read_file", "write_todos"). Applied to built-in tools.
    pub custom_tool_descriptions: HashMap<String, String>,
    /// When set (e.g. "/memories/"), paths under this prefix are routed to the store (long-term memory); other paths use the default backend. Requires a store (defaults to InMemoryStore).
    pub long_term_memory_prefix: Option<String>,
}

impl std::fmt::Debug for DeepAgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeepAgentConfig")
            .field("enable_planning", &self.enable_planning)
            .field("enable_filesystem", &self.enable_filesystem)
            .field("enable_task_tool", &self.enable_task_tool)
            .field("workspace_root", &self.workspace_root)
            .field("subagents", &self.subagents.len())
            .field("store", &self.store.as_ref().map(|_| "..."))
            .field("context", &self.context.as_ref().map(|_| "..."))
            .field("middleware", &self.middleware.as_ref().map(|m| m.len()))
            .field(
                "response_format",
                &self.response_format.as_ref().map(|_| "..."),
            )
            .field("skill_paths", &self.skill_paths)
            .field("skill_contents", &self.skill_contents.len())
            .field("skill_dirs", &self.skill_dirs)
            .field("memory_paths", &self.memory_paths)
            .field("memory_contents", &self.memory_contents.len())
            .field("interrupt_before_tools", &self.interrupt_before_tools)
            .field("interrupt_on", &self.interrupt_on.len())
            .field(
                "evict_tool_result_over_tokens",
                &self.evict_tool_result_over_tokens,
            )
            .field("default_subagent_model", &self.default_subagent_model)
            .field("file_backend", &self.file_backend.as_ref().map(|_| "..."))
            .field("checkpointer", &self.checkpointer.as_ref().map(|_| "..."))
            .field(
                "planning_system_prompt",
                &self.planning_system_prompt.as_ref().map(|_| "..."),
            )
            .field(
                "filesystem_system_prompt",
                &self.filesystem_system_prompt.as_ref().map(|_| "..."),
            )
            .field(
                "custom_tool_descriptions",
                &self.custom_tool_descriptions.len(),
            )
            .field("long_term_memory_prefix", &self.long_term_memory_prefix)
            .finish()
    }
}

/// Spec for one subagent used by the task tool.
#[derive(Clone)]
pub struct SubagentSpec {
    /// The subagent to invoke.
    pub agent: Arc<UnifiedAgent>,
    /// Tool-facing name (e.g. "researcher", "coder").
    pub name: String,
    /// Description for the LLM to choose when to use this subagent.
    pub description: String,
    /// Optional per-tool interrupt config for this subagent (overrides main agent interrupt_on when building this subagent).
    pub interrupt_on: Option<HashMap<String, InterruptConfig>>,
}

impl std::fmt::Debug for SubagentSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubagentSpec")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

impl Default for DeepAgentConfig {
    fn default() -> Self {
        Self {
            enable_planning: true,
            enable_filesystem: true,
            enable_task_tool: false,
            workspace_root: None,
            subagents: Vec::new(),
            store: None,
            context: None,
            middleware: None,
            response_format: None,
            skill_paths: Vec::new(),
            skill_contents: Vec::new(),
            skill_dirs: Vec::new(),
            memory_paths: Vec::new(),
            memory_contents: Vec::new(),
            interrupt_before_tools: Vec::new(),
            interrupt_on: HashMap::new(),
            evict_tool_result_over_tokens: None,
            default_subagent_model: None,
            file_backend: None,
            checkpointer: None,
            planning_system_prompt: None,
            filesystem_system_prompt: None,
            custom_tool_descriptions: HashMap::new(),
            long_term_memory_prefix: None,
        }
    }
}

impl DeepAgentConfig {
    /// Create a new config with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable the write_todos (planning) tool.
    pub fn with_planning(mut self, enable: bool) -> Self {
        self.enable_planning = enable;
        self
    }

    /// Enable or disable file system tools.
    pub fn with_filesystem(mut self, enable: bool) -> Self {
        self.enable_filesystem = enable;
        self
    }

    /// Enable or disable the task (subagent) tool.
    pub fn with_task_tool(mut self, enable: bool) -> Self {
        self.enable_task_tool = enable;
        self
    }

    /// Set the workspace root for file system tools.
    pub fn with_workspace_root(mut self, root: PathBuf) -> Self {
        self.workspace_root = Some(root);
        self
    }

    /// Set the workspace root when present (for building default subagent config).
    pub fn with_workspace_root_opt(mut self, root: Option<PathBuf>) -> Self {
        self.workspace_root = root;
        self
    }

    /// Add a subagent for the task tool.
    pub fn with_subagent(
        mut self,
        agent: Arc<UnifiedAgent>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.subagents.push(SubagentSpec {
            agent,
            name: name.into(),
            description: description.into(),
            interrupt_on: None,
        });
        self.enable_task_tool = true;
        self
    }

    /// Add a subagent with optional per-tool interrupt config (overrides main agent for this subagent).
    pub fn with_subagent_and_interrupt_on(
        mut self,
        agent: Arc<UnifiedAgent>,
        name: impl Into<String>,
        description: impl Into<String>,
        interrupt_on: Option<HashMap<String, InterruptConfig>>,
    ) -> Self {
        self.subagents.push(SubagentSpec {
            agent,
            name: name.into(),
            description: description.into(),
            interrupt_on,
        });
        self.enable_task_tool = true;
        self
    }

    /// Set the store for long-term memory and todos.
    pub fn with_store(mut self, store: Arc<dyn ToolStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Set custom context (session_id, user_id, workspace_root via get(), etc.).
    pub fn with_context(mut self, context: Arc<dyn ToolContext>) -> Self {
        self.context = Some(context);
        self
    }

    /// Set middleware (logging, retry, human-in-the-loop, etc.).
    pub fn with_middleware(mut self, middleware: Vec<Arc<dyn Middleware>>) -> Self {
        self.middleware = Some(middleware);
        self
    }

    /// Set structured output strategy.
    pub fn with_response_format(
        mut self,
        response_format: Box<dyn StructuredOutputStrategy>,
    ) -> Self {
        self.response_format = Some(response_format);
        self
    }

    /// Add a skill file path; content is read at build time and appended under "## Skills".
    pub fn with_skill_path(mut self, path: PathBuf) -> Self {
        self.skill_paths.push(path);
        self
    }

    /// Add multiple skill file paths.
    pub fn with_skill_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.skill_paths.extend(paths);
        self
    }

    /// Add inline skill (name, content) to be appended under "## Skills".
    pub fn with_skill_content(
        mut self,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.skill_contents.push((name.into(), content.into()));
        self
    }

    /// Add a skill directory (must contain SKILL.md with frontmatter). Enables progressive disclosure:
    /// only matched skills are loaded per turn. Later dirs override earlier for same skill name.
    pub fn with_skill_dir(mut self, dir: PathBuf) -> Self {
        self.skill_dirs.push(dir);
        self
    }

    /// Add multiple skill directories.
    pub fn with_skill_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.skill_dirs.extend(dirs);
        self
    }

    /// Add a memory file path (e.g. AGENTS.md); content appended under "## Memory".
    pub fn with_memory_path(mut self, path: PathBuf) -> Self {
        self.memory_paths.push(path);
        self
    }

    /// Add multiple memory file paths.
    pub fn with_memory_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.memory_paths.extend(paths);
        self
    }

    /// Add inline memory (name, content) under "## Memory".
    pub fn with_memory_content(
        mut self,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.memory_contents.push((name.into(), content.into()));
        self
    }

    /// Require human-in-the-loop approval before calling these tools (e.g. "write_file", "edit_file").
    pub fn with_interrupt_before_tools(mut self, tool_names: Vec<String>) -> Self {
        self.interrupt_before_tools = tool_names;
        self
    }

    /// Set per-tool interrupt config (tool name -> InterruptConfig). Replaces existing interrupt_on; use with [Self::with_interrupt_before_tools] to also set legacy list (merged in build_middleware).
    pub fn with_interrupt_on(mut self, interrupt_on: HashMap<String, InterruptConfig>) -> Self {
        self.interrupt_on = interrupt_on;
        self
    }

    /// Add or set one tool's interrupt config.
    pub fn with_interrupt_on_tool(
        mut self,
        tool_name: impl Into<String>,
        config: InterruptConfig,
    ) -> Self {
        self.interrupt_on.insert(tool_name.into(), config);
        self
    }

    /// Evict tool results larger than this many (estimated) tokens to the store; None = disabled.
    pub fn with_evict_tool_result_over_tokens(mut self, limit: Option<usize>) -> Self {
        self.evict_tool_result_over_tokens = limit;
        self
    }

    /// When using `create_deep_agent_from_llm` with task tool, use this model for the general-purpose subagent.
    pub fn with_default_subagent_model(mut self, model: Option<String>) -> Self {
        self.default_subagent_model = model;
        self
    }

    /// Set the file backend for FS tools (ls, read_file, write_file, edit_file, glob, grep).
    pub fn with_file_backend(mut self, backend: Option<Arc<dyn FileBackend>>) -> Self {
        self.file_backend = backend;
        self
    }

    /// Set checkpointer for human-in-the-loop (save on interrupt, load on resume; required for HILP).
    pub fn with_checkpointer(mut self, checkpointer: Option<Arc<dyn AgentCheckpointer>>) -> Self {
        self.checkpointer = checkpointer;
        self
    }

    /// Optional extra system prompt when planning is enabled (aligned with TodoListMiddleware(system_prompt=...)).
    pub fn with_planning_system_prompt(mut self, prompt: Option<String>) -> Self {
        self.planning_system_prompt = prompt;
        self
    }

    /// Optional extra system prompt when filesystem is enabled (aligned with FilesystemMiddleware(system_prompt=...)).
    pub fn with_filesystem_system_prompt(mut self, prompt: Option<String>) -> Self {
        self.filesystem_system_prompt = prompt;
        self
    }

    /// Override description for one built-in tool by name (e.g. "ls", "read_file", "write_todos").
    pub fn with_custom_tool_description(
        mut self,
        tool_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.custom_tool_descriptions
            .insert(tool_name.into(), description.into());
        self
    }

    /// Set custom tool descriptions for built-in tools (replaces existing overrides).
    pub fn with_custom_tool_descriptions(mut self, descriptions: HashMap<String, String>) -> Self {
        self.custom_tool_descriptions = descriptions;
        self
    }

    /// Enable long-term memory: paths under this prefix (e.g. `/memories/`) are stored in the store and persist across threads; other paths use the default backend. Uses config.store (defaults to InMemoryStore if not set). See [Long-term memory](https://docs.langchain.com/oss/python/deepagents/long-term-memory).
    pub fn with_long_term_memory(mut self, prefix: impl Into<String>) -> Self {
        self.long_term_memory_prefix = Some(prefix.into());
        self
    }
}
