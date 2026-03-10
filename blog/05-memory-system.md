# 记忆系统：Memory 实现与上下文管理

> 没有记忆的 Agent 就像没有过去的——每次都是全新的开始。

## 0. 为什么需要记忆？

```
对话 1：
User: 帮我写个排序算法
Agent: 好的，这里是冒泡排序...

对话 2（同一 Agent）：
User: 刚才那个算法能优化吗？
Agent: ???

问题：Agent 没有"记忆"，不知道之前聊过什么。
```

**Memory = Agent 的"大脑"——存储经验、上下文、知识。**

## 1. Memory 架构

### 1.1 记忆层次

```
┌─────────────────────────────────────────────────────────────┐
│                    Oris 记忆层次                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Working Memory (工作记忆)                │   │
│  │         当前对话的上下文，几千 tokens                  │   │
│  └─────────────────────────────────────────────────────┘   │
│                           │                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           Conversational Memory (会话记忆)          │   │
│  │         当前会话的历史，几十条消息                    │   │
│  └─────────────────────────────────────────────────────┘   │
│                           │                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           Long-Term Memory (长期记忆)                │   │
│  │         持久化的知识，可跨会话，矢量检索               │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 Memory Trait

```rust
use serde::{Deserialize, Serialize};
use async_trait::async_trait;

#[async_trait]
pub trait Memory: Send + Sync {
    /// 记忆类型
    fn memory_type(&self) -> MemoryType;
    
    /// 添加记忆
    async fn add(&self, entry: MemoryEntry) -> Result<(), MemoryError>;
    
    /// 搜索记忆
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, MemoryError>;
    
    /// 获取最近记忆
    async fn get_recent(&self, count: usize) -> Result<Vec<MemoryEntry>, MemoryError>;
    
    /// 清空记忆
    async fn clear(&self) -> Result<(), MemoryError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub memory_type: MemoryType,
    pub created_at: i64,
    pub metadata: HashMap<String, Value>,
    pub importance: f32,  // 重要性 0-1
}
```

## 2. 工作记忆 (Working Memory)

### 2.1 用途

**工作记忆 = 当前任务的临时存储**

```rust
use oris_runtime::memory::working::WorkingMemory;

let working = WorkingMemory::new(8192);  // 8K tokens

// 在 Agent 执行过程中使用
graph.add_node("process", |state| {
    // 读取工作记忆
    let context = working.get()?;
    
    // 写入工作记忆
    working.set("current_step", "analyzing")?;
    
    Ok(())
})?;
```

### 2.2 自动管理

```rust
// 自动截断（当超过限制时）
let working = WorkingMemory::new(8192)
    .auto_truncate(true)
    .truncation_strategy(TruncationStrategy::Summarize)  // 或 KeepFirst, KeepRecent
    .build()?;
```

## 3. 会话记忆 (Conversational Memory)

### 3.1 消息存储

```rust
use oris_runtime::memory::conversational::{ConversationalMemory, Message};

let memory = ConversationalMemory::new(100);  // 保留 100 条消息

// 添加消息
memory.add(Message {
    role: Role::User,
    content: "帮我写个排序算法".to_string(),
    created_at: now(),
}).await?;

memory.add(Message {
    role: Role::Assistant,
    content: "好的，这里是冒泡排序...".to_string(),
    created_at: now(),
}).await?;

// 获取上下文
let context = memory.get_context(4096).await?;  // 4K tokens 的上下文
```

### 3.2 消息格式

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub name: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    pub created_at: i64,
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}
```

### 3.3 对话摘要

```rust
let memory = ConversationalMemory::new(100)
    .enable_summary(true)        // 启用自动摘要
    .summary_threshold(50)       // 超过 50 条消息时摘要
    .summary_llm(llm.clone())
    .build()?;
```

## 4. 长期记忆 (Long-Term Memory)

### 4.1 矢量存储

```rust
use oris_runtime::memory::long_term::LongTermMemory;
use oris_runtime::vectorstore::qdrant::Qdrant;

let vector_store = Qdrant::new("localhost:6334", "agent_memory")?;

let memory = LongTermMemory::new(vector_store)
    .embedder(embedder)  // 嵌入模型
    .build()?;
```

### 4.2 添加记忆

```rust
// 手动添加
memory.add(MemoryEntry {
    id: "fact_001".to_string(),
    content: "Rust 是一种系统编程语言，注重安全性和并发性".to_string(),
    memory_type: MemoryType::Fact,
    importance: 0.8,
    ..Default::default()
}).await?;

// 自动添加（根据重要性）
memory.auto_archive(entry, 0.5).await?;
```

### 4.3 语义搜索

```rust
// 搜索相关记忆
let results = memory.search(
    "什么是 Rust 语言的特性",
    5  // 返回 5 条最相关的
).await?;

for entry in results {
    println!("Relevance: {:.2}", entry.score);
    println!("Content: {}", entry.content);
    println!();
}
```

### 4.4 记忆衰减

```rust
let memory = LongTermMemory::new(vector_store)
    .enable_decay(true)           // 启用衰减
    .decay_rate(0.01)            // 每天衰减 1%
    .min_importance(0.1)         // 最低重要性阈值
    .build()?;
```

## 5. 向量存储集成

### 5.1 支持的存储

| 存储 | 说明 |
|------|------|
| **Qdrant** | 高性能向量数据库 |
| **pgvector** | PostgreSQL 向量扩展 |
| **Chroma** | 开源向量数据库 |
| **Weaviate** | 云原生向量搜索 |
| **Milvus** | 大规模向量检索 |

### 5.2 Qdrant 示例

```rust
use oris_runtime::vectorstore::qdrant::Qdrant;

let qdrant = Qdrant::builder()
    .url("http://localhost:6334")
    .collection("my_agent")
    .vector_size(1536)  // OpenAI ada-002 维度
    .distance(Distance::Cosine)
    .build()?;

let memory = LongTermMemory::new(qdrant)
    .embedder(OpenAIEmbedder::default())
    .build()?;
```

### 5.3 pgvector 示例

```rust
use oris_runtime::vectorstore::pgvector::PgVector;

let pg = PgVector::new(pool)
    .table("embeddings")
    .vector_column("embedding")
    .build()?;
```

## 6. 嵌入模型

### 6.1 支持的模型

| 模型 | 维度 | 说明 |
|------|------|------|
| OpenAI ada-002 | 1536 | 默认 |
| OpenAI text-embedding-3-small | 1536 | 新版，更便宜 |
| Ollama | 多种 | 本地模型 |
| Claude | 1536 | Anthropic |

### 6.2 使用

```rust
use oris_runtime::embedding::openai::OpenAIEmbedder;

let embedder = OpenAIEmbedder::new("text-embedding-ada-002");

// 嵌入查询
let query_vector = embedder.embed_query("Rust 编程").await?;

// 嵌入文档
let doc_vector = embedder.embed_document("Rust 是...").await?;
```

## 7. Agent 集成记忆

### 7.1 配置记忆

```rust
use oris_runtime::agent::deep::DeepAgent;
use oris_runtime::memory::combined::CombinedMemory;

let memory = CombinedMemory::builder()
    .working(WorkingMemory::new(4096))
    .conversational(ConversationalMemory::new(50))
    .long_term(LongTermMemory::new(vector_store))
    .build()?;

let agent = DeepAgent::builder()
    .memory(memory)
    .build()?;
```

### 7.2 自动记忆检索

```rust
let agent = DeepAgent::builder()
    .memory(memory)
    .retrieve_similar(true)      // 自动检索相似记忆
    .retrieve_count(5)            // 检索 5 条
    .retrieve_threshold(0.7)      // 相似度阈值
    .build()?;
```

### 7.3 手动记忆访问

```rust
graph.add_node("remember", |state| {
    // 存储重要信息
    memory.add(MemoryEntry {
        content: format!("User prefers {}", preference),
        importance: 0.9,
        ..Default::default()
    }).await?;
    
    Ok(())
})?;
```

## 8. 记忆管理策略

### 8.1 重要性评分

```rust
fn calculate_importance(entry: &MemoryEntry) -> f32 {
    let mut score = 0.0;
    
    // 用户明确标记重要的
    if entry.metadata.get("important") == Some(&Value::Bool(true)) {
        score += 0.5;
    }
    
    // 包含关键信息的
    if contains_keyword(&entry.content, &["bug", "fix", "important", "remember"]) {
        score += 0.3;
    }
    
    // 被多次引用的
    if entry.metadata.get("references").and_then(|v| v.as_u64()).unwrap_or(0) > 3 {
        score += 0.2;
    }
    
    score.min(1.0)
}
```

### 8.2 自动摘要

```rust
let memory = ConversationalMemory::new(100)
    .enable_summary(true)
    .summary_trigger(|count| count > 50)
    .summary_llm(llm)
    .summary_template(r#"
Summarize the key points of this conversation:

{ messages }

Keep the summary under 500 words.
"#)
    .build()?;
```

### 8.3 记忆清理

```rust
// 清理低重要性的记忆
memory.prune(min_importance = 0.2).await?;

// 清理特定时间之前的
memory.prune_older_than(Duration::from_days(30)).await?;

// 清理特定类型的
memory.prune_by_type(MemoryType::Ephemeral).await?;
```

## 9. 完整示例

### 9.1 配置完整记忆系统

```rust
use oris_runtime::memory::{
    combined::CombinedMemory,
    working::WorkingMemory,
    conversational::ConversationalMemory,
    long_term::LongTermMemory,
};
use oris_runtime::vectorstore::qdrant::Qdrant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 向量存储
    let qdrant = Qdrant::new("localhost:6334", "agent_memory")?;
    
    // 2. 长期记忆
    let long_term = LongTermMemory::new(qdrant)
        .embedder(OpenAIEmbedder::default())
        .enable_decay(true)
        .build()?;
    
    // 3. 会话记忆
    let conversational = ConversationalMemory::new(50)
        .enable_summary(true)
        .summary_llm(llm.clone())
        .build()?;
    
    // 4. 工作记忆
    let working = WorkingMemory::new(4096);
    
    // 5. 组合记忆
    let memory = CombinedMemory::builder()
        .working(working)
        .conversational(conversational)
        .long_term(long_term)
        .build()?;
    
    // 6. 用于 Agent
    let agent = DeepAgent::builder()
        .memory(memory)
        .build()?;
    
    Ok(())
}
```

### 9.2 记忆检索流程

```
用户查询 → 
    │
    ├─→ 工作记忆（当前上下文）
    │
    ├─→ 会话记忆（最近对话）
    │
    ├─→ 长期记忆（语义搜索相似记忆）
    │
    └─→ 合并 → 构建 prompt → LLM
```

## 10. 与其他系统的对比

| 特性 | Oris Memory | LangChain Memory |
|------|-------------|------------------|
| 记忆类型 | 工作+会话+长期 | 会话+长期 |
| 向量存储 | 多后端 | 有限 |
| 重要性评分 | ✅ | ❌ |
| 自动摘要 | ✅ | ⚠️ |
| 记忆衰减 | ✅ | ❌ |
| 持久化 | ✅ | ⚠️ |

## 11. 小结

Oris 的记忆系统：

1. **工作记忆** — 临时上下文
2. **会话记忆** — 对话历史
3. **长期记忆** — 矢量存储 + 语义检索
4. **组合记忆** — 多层记忆组合
5. **重要性评分** — 自动识别重要信息
6. **自动摘要** — 长对话压缩
7. **记忆衰减** — 防止遗忘 + 清理无用

**没有记忆的 Agent 是愚蠢的——有了记忆，Agent 才能真正"学习"。**

---

*下篇预告：RAG 与向量存储——知识检索实战*
