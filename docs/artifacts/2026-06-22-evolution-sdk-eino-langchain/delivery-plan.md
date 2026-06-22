# Delivery Plan — Oris Evolution SDK Adapters for Eino and LangChain

**状态**: implemented  
**日期**: 2026-06-22  
**阶段**: plan  
**主责角色**: tech-lead  
**关联 PRD**: `prd.md`  
**关联架构**: `arch-design.md`

---

## 目标

把 Oris 的经验进化能力以框架原生方式接入：

- Go / Eino: 提供 adapter package 和可运行示例。
- Python / LangChain: 提供 middleware package 和可运行示例。
- 复用现有 `sdks/go`、`sdks/python`，不重写底层 HTTP client。

MVP 成功标准：

1. 两个 adapter 均能完成 Detect -> Select -> Replay -> Validate -> Solidify 的本地闭环。
2. 示例不依赖真实 LLM 和远端 Oris 服务即可跑通。
3. 当配置 Experience Repo 时，可选 fetch/share 走现有 SDK。
4. Go/Python SDK 原有测试继续通过。

---

## Requirement Challenge 收口

| # | 问题 | 结论 | 阻断 |
|---|------|------|------|
| C-1 | 是否需要从零做 Go/Python SDK？ | 不需要。现有 `sdks/go`、`sdks/python` 已覆盖底层能力，本轮做框架 adapter。 | 否 |
| C-2 | PRD 中 Experience endpoint 是否准确？ | 需修正。当前 SDK 使用 `/experience` 与 `/public-keys`。 | Story 0 |
| C-3 | Experience signature 是 hex 还是 base64？ | 以 `sdks/spec/signing-spec.md` 为准，当前 SDK/server verifier 目标是 base64 signature，public key hex。 | Story 0 |
| C-4 | Replay 是否直接执行修复？ | MVP 不直接执行危险动作，只返回 replay instructions/hints，由框架或用户回调执行。 | 否 |
| C-5 | 是否强依赖远端 Experience Repo？ | 不强依赖。MVP local-first，远端 fetch/share 可选。 | 否 |
| C-6 | 是否需要先研究框架 API？ | 已完成初步核对：LangChain 用 middleware/wrap_tool_call；Eino 用 Graph handler、callback、ADK middleware。实现前需 pin 版本。 | Story 0 |

---

## Story 0：修正事实源与锁定版本

**目标**: 消除 PRD 与当前仓库状态不一致，固定 adapter 的依赖边界。

验收标准：

- [ ] 更新 `prd.md` 中旧假设：已有 Go/Python SDK、Experience endpoint、signature 编码、local-first 范围。
- [ ] 在 `sdks/README.md` 或 adapter README 中声明 Eino/LangChain adapter 属于 framework layer。
- [ ] 锁定最低支持版本：
  - Go Eino: 选择 `github.com/cloudwego/eino` 与需要的 ADK/ext 包版本。
  - Python LangChain: 选择支持 `langchain.agents.middleware` 的最低版本。
- [ ] 明确 examples 是否放在 `examples/` 还是各 SDK 内部。建议：
  - Go: `sdks/go/examples/eino_evolution_agent`
  - Python: `sdks/python/examples/langchain_evolution_agent`

验证：

- 文档 review。
- 不要求 cargo/go/pytest。

---

## Story 1：共享 Adapter Core 模型

**目标**: 在 Go/Python 各自 SDK 中补齐一致的 Oris evolution adapter 模型。

Go 交付：

- [ ] `sdks/go/evolution/types.go`
- [ ] `EvolutionSignal`
- [ ] `EvolutionCandidate`
- [ ] `ReplayDecision`
- [ ] `ValidationResult`
- [ ] `SolidifyInput`
- [ ] 单测覆盖 fingerprint normalization。

Python 交付：

- [ ] `sdks/python/src/oris_sdk/evolution/types.py`
- [ ] 对应 dataclass / pydantic-free model。
- [ ] 单测覆盖 fingerprint normalization。

验收标准：

- [ ] 不引入框架依赖。
- [ ] 能基于错误文本和 task_class 生成稳定 fingerprint。
- [ ] 能从 store query 结果映射为 candidate。

验证：

```bash
cd sdks/go && go test ./...
cd sdks/python && PYTHONPATH=src pytest tests/ -v
```

---

## Story 2：Go Eino Adapter

**目标**: 将 adapter core 接入 Eino。

交付：

- [x] `sdks/go/einoadapter/middleware.go`
- [x] `sdks/go/einoadapter/middleware_test.go`

验收标准：

- [x] 支持从 tool error 生成 `EvolutionSignal`。
- [x] 支持从 store 查询候选经验。
- [x] 支持把 replay decision 转成 Eino `compose.ToolOutput` hint。
- [x] 支持 validation callback 成功后通过 core adapter 保存 gene。
- [x] Eino 依赖隔离在 `einoadapter`，核心 `evolution` 包不依赖 Eino。

验证：

```bash
cd sdks/go && go test ./...
```

---

## Story 3：Python LangChain Adapter

**目标**: 将 adapter core 接入 LangChain agent middleware。

交付：

- [x] `sdks/python/src/oris_sdk/evolution/adapter.py`
- [x] `sdks/python/src/oris_sdk/langchain/middleware.py`
- [x] `sdks/python/tests/test_langchain_adapter.py`

验收标准：

- [x] 提供 `create_oris_middleware(adapter)`。
- [x] 支持 `create_agent(..., middleware=[oris_middleware])`。
- [x] tool failure 时能 detect/select/replay。
- [x] 用户 validation callback 通过后能通过 core adapter solidify。
- [x] LangChain 依赖作为 optional extra：`oris-rt-sdk[langchain]`。

验证：

```bash
cd sdks/python && PYTHONPATH=src pytest tests/ -v
```

---

## Story 4：Eino Quickstart 示例

**目标**: 让 Go 用户 5 分钟内跑通本地经验复用闭环。

交付：

- [x] `sdks/go/examples/eino_evolution_agent/main.go`
- [x] `sdks/go/examples/eino_evolution_agent/README.md`

示例必须展示：

1. 初始化 local store。
2. 预置一个 capsule/gene。
3. 构造一个失败 tool。
4. Eino adapter 捕获失败并返回 replay hint。
5. 下一轮执行成功。
6. 保存新的经验。

验证：

```bash
cd sdks/go/examples/eino_evolution_agent && go run .
```

---

## Story 5：LangChain Quickstart 示例

**目标**: 让 Python 用户 5 分钟内跑通本地经验复用闭环。

交付：

- [x] `sdks/python/examples/langchain_evolution_agent/main.py`
- [x] `sdks/python/examples/langchain_evolution_agent/README.md`

示例必须展示：

1. 初始化 local store。
2. 预置一个 capsule/gene。
3. 创建 LangChain agent 和 Oris middleware。
4. tool failure 触发 replay。
5. 成功后 solidify。

验证：

```bash
cd sdks/python && PYTHONPATH=src python examples/langchain_evolution_agent/main.py
```

---

## Story 6：文档与发布准备

**目标**: 把 adapter 作为现有 SDK 的扩展能力纳入文档。

交付：

- [ ] 更新 `sdks/README.md`，新增 framework adapters 章节。
- [ ] 更新 `sdks/INTEGRATION_GUIDE.md`，说明 local-first evolution flow。
- [ ] Python `pyproject.toml` 新增 optional extra: `langchain`。
- [ ] Go README 标注 Eino adapter 的 import path。

验证：

```bash
cd sdks/go && go test ./...
cd sdks/python && PYTHONPATH=src pytest tests/ -v
```

---

## 推荐执行顺序

1. Story 0：先修正事实源。
2. Story 1：先做无框架 core model。
3. Story 3：优先 LangChain adapter，因为 middleware API 接入点更明确。
4. Story 2：并行或随后做 Eino adapter。
5. Story 4/5：示例补齐。
6. Story 6：文档和发布整理。

---

## 不建议的路径

| 路径 | 为什么不建议 |
|------|--------------|
| 直接写 Eino/LangChain 示例，不做 adapter core | 示例很快能跑，但经验模型会散落在框架代码里，后续无法扩展 |
| 从 Rust EvoKernel 移植到 Go/Python | 工作量大且偏离 Oris 当前边界，MVP 不需要 |
| 示例依赖真实 LLM 和真实 Experience Repo | 验证不稳定，5 分钟 quickstart 很难成立 |
| 一开始支持所有 Agent 框架 | 框架生命周期差异大，应先用 Eino/LangChain 验证 adapter 抽象 |

---

## 交接给研发

**backend-engineer 输入**:
- `arch-design.md`
- 本 `delivery-plan.md`
- `sdks/spec/signing-spec.md`
- 现有 Go/Python SDK 测试

**实现边界**:
- 只新增 adapter/core/example 文件。
- 除 Story 0 文档校正外，不改 Rust crates。
- 不改现有 HTTP 协议。

**验收命令**:

```bash
cd sdks/go && go test ./...
cd sdks/python && PYTHONPATH=src pytest tests/ -v
```
