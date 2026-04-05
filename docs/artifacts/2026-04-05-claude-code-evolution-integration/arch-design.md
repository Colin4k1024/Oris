---
artifact: arch-design
task: claude-code-evolution-integration
date: 2026-04-05
role: architect
status: plan-complete
---

# Arch Design — Oris 自我进化能力集成到 Claude Code

## 1. 系统边界

### 外部依赖

| 依赖 | 用途 | 集成方式 |
|------|------|----------|
| Claude Code Harness | 调用方 | IPC (Unix Domain Socket) |
| Oris Evolution Crates | 核心能力 | Rust 直接依赖 |
| SQLite | Gene 存储 | `oris-genestore` |
| OS Sandbox | 隔离执行 | `oris-sandbox` |

### 边界划分

```
┌─────────────────────────────────────────────────────────────┐
│                    Claude Code Harness                         │
│                    (~/.claude/)                               │
│                                                              │
│  ┌─────────────┐    Unix Socket    ┌─────────────────────┐ │
│  │ IPC Client  │◄────────────────►│  Evolution Server   │ │
│  └─────────────┘                   │  (oris-evo-server)  │ │
│                                    │                      │ │
│                                    │  ┌───────────────┐  │ │
│                                    │  │Pipeline Driver│  │ │
│                                    │  └───────┬───────┘  │ │
│                                    │          │          │ │
│                                    │  ┌───────▼───────┐  │ │
│                                    │  │  Gene Pool    │  │ │
│                                    │  │  (SQLite)     │  │ │
│                                    │  └───────────────┘  │ │
│                                    └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. 组件拆分

### 组件列表

| 组件 | 职责 | 代码位置 |
|------|------|----------|
| `oris-evo-server` | 进化服务进程，持有 Pipeline | 新建 crate |
| `oris-evo-ipc-protocol` | IPC 协议定义（请求/响应） | 新建 crate |
| `harness-evo-plugin` | Claude Code 侧 IPC 客户端 | 集成到 harness |
| `evolution-cli` | Gene Pool 管理 CLI | 新建 binary |

### 核心数据流

```
1. Harness Hook (Signal 检测)
       │
       ▼
2. IPC Client → Unix Socket
       │
       ▼
3. Evolution Server (Pipeline Driver)
       │
       ├──► 3a. SignalExtractor (Detect)
       │
       ├──► 3b. Selector (Select)
       │
       ├──► 3c. MutationEvaluator (Validate)
       │         │
       │         ├──► 3c-1. Static Analysis (阻断)
       │         │
       │         └──► 3c-2. LLM Critic (评估)
       │
       ├──► 3d. Sandbox Execute
       │
       ├──► 3e. GeneStore Persist (签名 + 来源)
       │
       └──► 3f. Auto Revert (如需要)

4. IPC Response → Harness
```

---

## 3. IPC 协议设计

### Socket 地址

```
~/.claude/evolution/evolution.sock
```

### 请求格式 (JSON)

```json
{
  "version": "1.0",
  "method": "evolve",
  "params": {
    "signal": {
      "type": "compiler_error",
      "content": "error[E0502]: ...",
      "source": {
        "file": "/path/to/file.rs",
        "line": 42
      }
    },
    "context": {
      "session_id": "uuid",
      "user_id": "uuid",
      "workspace": "/path/to/workspace"
    }
  },
  "signature": "base64(Ed25519)"
}
```

### 响应格式 (JSON)

```json
{
  "version": "1.0",
  "request_id": "uuid",
  "result": {
    "gene_id": "uuid",
    "confidence": 0.85,
    "action": "solidify | apply_once | reject",
    "revert_triggered": false
  },
  "error": null
}
```

### 方法定义

| 方法 | 说明 | 触发时机 |
|------|------|----------|
| `evolve` | 提交进化请求 | 错误检测到后 |
| `solidify` | 确认固化 Gene | 用户确认后 |
| `revert` | 触发 revert | 自动/手动 |
| `query` | 语义检索 Gene | 复用时 |
| `list` | 列出 Gene Pool | CLI 工具 |

---

## 4. 接口约定

### 服务端接口

| 接口 | 协议 | 认证 |
|------|------|------|
| Unix Socket | stream | 签名验证 |

### 关键 API

```rust
// IPC Protocol Trait
pub trait EvolutionIpc {
    async fn submit_signal(&self, signal: RuntimeSignal) -> Result<EvolutionResult>;
    async fn query_genes(&self, query: GeneQuery) -> Result<Vec<Gene>>;
    async fn solidify(&self, gene_id: GeneId) -> Result<()>;
    async fn revert(&self, gene_id: GeneId, reason: &str) -> Result<()>;
}
```

### 客户端接口

```rust
// Harness Plugin Trait
pub trait EvolutionPlugin {
    fn on_signal(&self, signal: RuntimeSignal) -> impl Future<Output = Result<()>>;
    fn query_similar(&self, pattern: &str) -> impl Future<Output = Result<Vec<Gene>>>;
    fn check_gene_valid(&self, gene_id: GeneId) -> impl Future<Output = Result<bool>>;
}
```

---

## 5. 技术选型

| 组件 | 选型 | 原因 |
|------|------|------|
| IPC 方式 | Unix Domain Socket | 低延迟 (<5ms)、操作系统级隔离 |
| 序列化 | JSON (serde) | 调试友好、跨语言 |
| Pipeline | `StandardEvolutionPipeline` | Oris 现有实现 |
| Gene Store | `oris-genestore` (SQLite) | 已集成、WAL 模式 |
| Sandbox | `oris-sandbox` | OS 级隔离 |
| 签名 | Ed25519 | `oris-evolution-network` 已支持 |

### 依赖版本

| Crate | 版本 | 用途 |
|-------|------|------|
| `oris-evolution` | 0.4.1 | 核心类型和 Pipeline |
| `oris-evokernel` | 0.14.1 | 编排层 |
| `oris-genestore` | 0.2.0 | SQLite Gene 存储 |
| `oris-sandbox` | 0.3.0 | 沙箱执行 |
| `oris-mutation-evaluator` | 0.3.0 | 两阶段评估 |

---

## 6. 数据模型

### Gene 结构（扩展）

```rust
pub struct Gene {
    pub id: GeneId,
    pub content_hash: ContentHash,
    pub capsule: Capsule,
    pub confidence: Confidence,
    pub signature: Ed25519Signature,      // 签名
    pub source_tag: SourceTag,           // 来源标签
    pub created_at: Timestamp,
    pub metadata: GeneMetadata,
}

pub struct SourceTag {
    pub error_type: String,      // "compiler_error" | "panic" | "test_failure"
    pub user_id: UserId,
    pub session_id: SessionId,
    pub timestamp: Timestamp,
}

pub struct EvolutionResult {
    pub gene_id: Option<GeneId>,
    pub confidence: Confidence,
    pub action: Action,         // "solidify" | "apply_once" | "reject"
    pub revert_triggered: bool,
    pub evaluation_report: EvaluationReport,
}
```

### SQLite Schema（扩展）

```sql
CREATE TABLE genes (
    id TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    capsule_json TEXT NOT NULL,
    confidence REAL NOT NULL,
    signature TEXT NOT NULL,
    source_tag_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    metadata_json TEXT
);

CREATE INDEX idx_genes_confidence ON genes(confidence);
CREATE INDEX idx_genes_signature ON genes(signature);
```

---

## 7. 关键数据流

### 进化完整流程

```
┌─────────────────────────────────────────────────────────────────┐
│                      完整进化流程                                  │
│                                                                  │
│  1. Signal 检测 (Harness Hook)                                  │
│     └─► RuntimeSignal { type, content, source }                  │
│                                                                  │
│  2. IPC 提交 (Harness → Server)                                 │
│     └─► JSON { method: "evolve", params: { signal, context } }   │
│                                                                  │
│  3. Pipeline Execute                                             │
│     │                                                            │
│     ├─► 3a. Detect: SignalExtractor.extract()                    │
│     │       └─► Vec<EvolutionSignal>                             │
│     │                                                            │
│     ├─► 3b. Select: Selector.select()                           │
│     │       └─► Vec<CandidateGene>                              │
│     │                                                            │
│     ├─► 3c. Mutate: MutationGenerator.generate()                 │
│     │       └─► MutationProposal                                 │
│     │                                                            │
│     ├─► 3d. Validate: MutationEvaluator.evaluate()               │
│     │       ├─► Static Analysis (阻断检查)                       │
│     │       └─► LLM Critic (评分)                               │
│     │                                                            │
│     ├─► 3e. Execute: Sandbox.execute()                           │
│     │       └─► SandboxExecutionResult                           │
│     │                                                            │
│     ├─► 3f. Evaluate: Confidence Scoring                         │
│     │       └─► EvaluationResult { confidence }                  │
│     │                                                            │
│     └─► 3g. Solidify: GeneStore.persist()                        │
│             ├─► Ed25519 签名                                    │
│             ├─► SourceTag 记录                                   │
│             └─► 写入 SQLite                                      │
│                                                                  │
│  4. 自动 Revert 检查                                             │
│     └─► 置信度骤降 > 20% ? → revert : continue                   │
│                                                                  │
│  5. IPC 响应 (Server → Harness)                                 │
│     └─► JSON { result: { gene_id, confidence, action } }         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 复用流程

```
┌─────────────────────────────────────────────────────────────────┐
│                      Gene 复用流程                                 │
│                                                                  │
│  1. 错误发生 (Harness)                                           │
│                                                                  │
│  2. IPC 查询 (Harness → Server)                                  │
│     └─► JSON { method: "query", params: { pattern } }            │
│                                                                  │
│  3. 语义检索 (Server)                                           │
│     ├─► GeneStore.query_by_similarity(pattern)                   │
│     └─► Vec<Gene> (top-k by similarity)                          │
│                                                                  │
│  4. 签名验证 (Server)                                           │
│     └─► Ed25519::verify(gene.signature, gene.content)           │
│                                                                  │
│  5. 置信度检查                                                  │
│     └─► gene.confidence >= 0.72 ? → 复用 : 正常流程              │
│                                                                  │
│  6. IPC 响应                                                    │
│     └─► JSON { result: { genes, reused: true } }                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 8. 安全设计

### 签名验证流程

```
MutationProposal
       │
       ▼
┌──────────────────┐
│  Ed25519 Sign    │ (使用私钥签名)
│  gene.content    │
└────────┬─────────┘
         │
         ▼
    Gene.signature
         │
         ▼
┌──────────────────┐
│  Ed25519 Verify  │ (使用公钥验证)
│  before persist │
└──────────────────┘
```

### Auto Revert 触发条件

| 条件 | 阈值 | 动作 |
|------|------|------|
| 验证失败 | 任何阶段 | revert |
| 置信度骤降 | > 20% vs 初始 | revert |
| Sandbox 执行失败 | 超时/错误 | revert |
| 签名验证失败 | 任何失败 | reject |

---

## 9. 风险与约束

### 技术风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| IPC 单点故障 | 进化失败 | 服务自重启 + 客户端重连 |
| SQLite 并发 | 写入冲突 | WAL 模式 + 序列化写入 |
| LLM 评估延迟 | 总延迟超标 | 静态分析优先阻断 + 异步 |

### 上线前必须解决

1. **签名验证必须测试通过** — 防止恶意 Gene 注入
2. **Auto revert 必须验证** — 确保误进化可恢复
3. **Sandbox 隔离必须确认** — 防止恶意代码逃逸
4. **性能基准必须达标** — 总延迟 < 500ms

---

## 10. 项目结构

```
oris-evo-server/               # 新建：Evolution Server
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── server.rs             # Socket 服务端
│   ├── pipeline.rs            # Pipeline 驱动
│   ├── handlers.rs            # IPC 方法处理
│   └── error.rs
├── Cargo.toml
└── tests/

oris-evo-ipc-protocol/         # 新建：协议定义
├── src/
│   ├── lib.rs
│   ├── request.rs
│   ├── response.rs
│   └── types.rs
└── Cargo.toml

evolution-cli/                  # 新建：CLI 工具
├── src/
│   ├── main.rs
│   └── commands.rs
└── Cargo.toml
```
