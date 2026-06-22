# Arch Design — Oris Evolution Adapters for Eino and LangChain

**状态**: draft  
**日期**: 2026-06-22  
**阶段**: architecture  
**主责角色**: architect  
**关联 PRD**: `prd.md`

---

## 结论

本阶段不再从零实现 Go/Python 底层 SDK。仓库已有 `sdks/go` 与 `sdks/python`，并覆盖 Experience Repo、Hub、Execution、Store、Sync 等基础能力。下一步应实现**框架适配层**：

- Go: 在 `sdks/go` 上新增 Eino adapter。
- Python: 在 `sdks/python` 上新增 LangChain adapter。
- 示例: 每个框架提供一个完整 Detect -> Select -> Replay -> Validate -> Solidify quickstart。

核心原则：Oris 负责经验进化语义，Eino/LangChain 负责 Agent 编排。适配层只做生命周期接入，不复制 HTTP client、签名、store、sync 逻辑。

---

## Brownfield 事实

| 维度 | 现状 | 影响 |
|------|------|------|
| Go SDK | `sdks/go/{experience,hub,execution,store,sync}` 已存在 | Eino adapter 应复用现有 client 和 store |
| Python SDK | `sdks/python/src/oris_sdk` 已存在，包名为 `oris-rt-sdk` | LangChain adapter 应作为现有包的可选模块 |
| Experience API | 当前 SDK 使用 `GET/POST /experience` | PRD 中 `/experience-repo/share`、`/experience-repo/fetch` 需校正 |
| Experience 签名 | `sdks/spec/signing-spec.md` 要求 body 内 `envelope.signature` 使用 base64 | PRD 中 hex signature 假设需校正 |
| 本地 store | Go/Python 都已有本地 store 协议 | MVP 可支持 local-first replay，不必强依赖远端服务 |
| Framework docs | LangChain 当前推荐 `create_agent` + middleware `wrap_tool_call`；Eino 支持 Graph、Tool callback、ADK middleware | 适配点应优先选 middleware/callback，不侵入用户 agent 代码 |

---

## 系统边界

```text
Eino Agent / LangChain Agent
        |
        v
Framework Adapter
  - detect runtime signal
  - select reusable gene/capsule
  - inject replay hint or strategy
  - observe validation result
  - solidify successful outcome
        |
        v
Oris SDK Core
  - ExperienceClient
  - Store / SyncManager
  - Hub / Execution clients where needed
        |
        v
Oris services / local store
```

**边界内**:
- 信号抽取：把错误、工具失败、测试失败、异常栈转成 Oris `EvolutionSignal`。
- 经验选择：查询本地 store，必要时调用 Experience Repo fetch。
- 重放注入：把 capsule steps 转成框架可消费的 prompt hint、tool result 或 runnable step。
- 结果沉淀：成功后生成 gene/capsule 并保存本地 store，可选 share。

**边界外**:
- 不移植 EvoKernel。
- 不实现 Governor 本地策略。
- 不替代框架原生 Agent executor。
- 不在 MVP 中强制启动 Oris Execution Server。

---

## 共享语义模型

适配层需要先补一个语言无关的最小模型，保持 Go/Python 一致。

| 概念 | 字段 | 说明 |
|------|------|------|
| `EvolutionSignal` | `task_class`, `error_type`, `message`, `fingerprint`, `context` | 从工具失败、异常、测试日志中提取 |
| `EvolutionCandidate` | `gene_id`, `capsule_id`, `confidence`, `rationale`, `steps` | 可复用经验候选 |
| `ReplayDecision` | `mode`, `candidate`, `instructions`, `metadata` | `skip`、`suggest`、`force` 三种模式 |
| `ValidationResult` | `passed`, `evidence`, `metrics`, `error` | 用户回调或框架执行结果 |
| `SolidifyInput` | `signal`, `solution`, `validation`, `tags` | 沉淀经验的输入 |

MVP 的 `replay` 不直接执行危险动作，只返回可解释策略步骤，由框架 agent 或用户回调执行。

---

## Go / Eino Adapter

建议路径：

```text
sdks/go/einoadapter/
  adapter.go
  signal.go
  replay.go
  middleware.go
  callback.go
  types.go
  adapter_test.go
```

主要接口：

```go
type Adapter struct {
    Store store.Store
    Experience *experience.Client
    Policy ReplayPolicy
}

func (a *Adapter) Detect(ctx context.Context, err error, meta map[string]any) (*EvolutionSignal, error)
func (a *Adapter) Select(ctx context.Context, signal EvolutionSignal) ([]EvolutionCandidate, error)
func (a *Adapter) Replay(ctx context.Context, candidate EvolutionCandidate) (*ReplayDecision, error)
func (a *Adapter) Solidify(ctx context.Context, input SolidifyInput) error
```

Eino 接入点：

- Graph 场景：在 node pre/post handler 中调用 Detect/Replay/Solidify。
- Tool 场景：通过 callback handler 观察 tool start/end/error。
- ADK Agent 场景：提供 `ChatModelAgentMiddleware`，包装工具错误与执行结果。

示例目标：

```text
examples/eino_evolution_agent/
  go.mod
  main.go
  README.md
```

示例行为：
1. 构造一个会失败的 tool。
2. adapter 从错误中生成 signal。
3. 本地 store 命中 capsule。
4. adapter 将 replay hint 注入下一轮 agent prompt/tool result。
5. 验证回调通过后保存新 gene 并可选 share。

---

## Python / LangChain Adapter

建议路径：

```text
sdks/python/src/oris_sdk/langchain/
  __init__.py
  adapter.py
  middleware.py
  signal.py
  replay.py
  types.py
sdks/python/tests/test_langchain_adapter.py
```

主要接口：

```python
class OrisEvolutionAdapter:
    def detect(self, error: Exception | str, context: dict[str, Any]) -> EvolutionSignal: ...
    def select(self, signal: EvolutionSignal) -> list[EvolutionCandidate]: ...
    def replay(self, candidate: EvolutionCandidate) -> ReplayDecision: ...
    def solidify(self, input: SolidifyInput) -> None: ...
```

LangChain 接入点：

- 首选：`AgentMiddleware.wrap_tool_call` 或 `@wrap_tool_call`，拦截工具失败与成功结果。
- 对外工厂：`create_oris_middleware(adapter)`，用户传入 `create_agent(..., middleware=[...])`。
- 补充工具：提供 `@tool` 风格的 `oris_search_experience`，用于显式查询经验。

示例目标：

```text
sdks/python/examples/langchain_evolution_agent/
  README.md
  main.py
```

示例行为：
1. 创建 LangChain agent。
2. 注册 Oris middleware。
3. 工具首次失败，middleware 生成 signal 并查找 candidate。
4. middleware 返回可执行修复提示或 ToolMessage。
5. 成功后调用 `solidify` 保存经验。

---

## 数据流

### Runtime Failure -> Replay

```text
tool/agent error
  -> adapter.detect(error, context)
  -> store.query(signal fingerprint)
  -> experience.fetch fallback
  -> adapter.replay(candidate)
  -> inject instructions into framework
  -> framework retries or user callback applies strategy
```

### Success -> Solidify

```text
successful task result
  -> validation callback returns passed evidence
  -> adapter.solidify(signal, solution, validation)
  -> store.save(gene/capsule)
  -> sync/share when configured
```

---

## 关键设计决策

| 决策 | Evidence | Reasoning | Implication |
|------|----------|-----------|-------------|
| 做 adapter，不重写 SDK | 仓库已有 Go/Python SDK 与 store/sync | 底层协议已可用，短板是框架生命周期接入 | 交付更快，风险集中在 Eino/LangChain API |
| Replay 默认只建议，不强制执行 | Oris 当前定位是 supervised bounded self-evolution | 跨框架强制执行策略风险高，且验证环境差异大 | MVP 安全，可逐步增加 policy |
| 先本地 store，再远端 fetch | 现有 SDK 已有 local store；远端服务未必可用 | 本地优先能让示例 5 分钟内跑通 | 示例无需部署全套 Oris 服务 |
| LangChain 用 middleware | 当前文档支持 `wrap_tool_call` 和 `AgentMiddleware` | 最贴近工具失败/成功生命周期 | 用户接入成本低 |
| Eino 同时支持 callback 与 ADK middleware | 当前 Eino 文档展示 Graph、Tool callback、Agent middleware | Eino 场景多样，adapter core 应与接入形态解耦 | Go 侧需多一个薄封装层 |

---

## 风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| Eino API 演进快 | 示例可能过期 | adapter core 与 Eino binding 分离，Eino 依赖集中在一个包 |
| LangChain v1 middleware 类型变化 | 编译/运行失败 | pin 最低支持版本，测试覆盖 middleware |
| 信号 fingerprint 过粗 | 误用不相关经验 | MVP 使用 error_type + normalized message + task_class，后续加 embedding |
| 示例依赖真实 LLM | 不稳定、成本高 | quickstart 使用 fake/model stub，另给真实模型示例 |
| PRD 与当前 SDK 事实不一致 | 返工 | Story 0 先校正文档与 spec |

---

## 验证策略

- Go: `cd sdks/go && go test ./...`
- Python: `cd sdks/python && PYTHONPATH=src pytest tests/ -v`
- Adapter 单测：mock store + fake experience client。
- 示例 smoke：不依赖真实 Oris 服务，不依赖真实 LLM。
- 协议回归：不改 `sdks/spec` golden fixtures；若发现不一致，先修 spec 或现有 SDK bug。

