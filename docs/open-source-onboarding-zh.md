# Oris 开源用户上手指南（中文）

本文面向第一次接触 Oris 的 Rust 开发者，目标是在最短路径内完成：

1. 本地跑通最小可用示例。
2. 理解如何集成到现有 Rust 服务。
3. 明确生产接入前必须补齐的能力。

## 1. 环境准备

- Rust stable（建议使用最新稳定版）
- Cargo
- 本地可写文件系统（用于 SQLite 持久化文件）

安装依赖：

```bash
cargo add oris-runtime
```

如需持久化执行（推荐）：

```bash
cargo add oris-runtime --features sqlite-persistence
```

## 2. 5 分钟跑通（推荐路径）

仓库内已提供可直接运行的集成路径：

- `examples/oris_starter_axum`
- `examples/oris_worker_tokio`
- `examples/oris_operator_cli`
- `examples/templates`（三套可脚手架模板）

启动：

```bash
cargo run -p oris_starter_axum
```

默认地址：

- `ORIS_SERVER_ADDR=127.0.0.1:8080`
- `ORIS_SQLITE_DB=oris_starter.db`

如果你不是要嵌入 HTTP 服务，而是只需要 worker 或运维 CLI：

- 独立 worker：`cargo run -p oris_worker_tokio`
- 运维 CLI：`cargo run -p oris_operator_cli -- --help`

### API 冒烟验证

创建运行：

```bash
curl -s -X POST http://127.0.0.1:8080/v1/jobs/run \
  -H 'content-type: application/json' \
  -d '{"thread_id":"starter-1","input":"hello from starter","idempotency_key":"starter-key-1"}'
```

查看运行：

```bash
curl -s http://127.0.0.1:8080/v1/jobs/starter-1
```

列出运行：

```bash
curl -s http://127.0.0.1:8080/v1/jobs
```

如果希望直接生成你自己的项目骨架，推荐使用 `cargo-generate`：

```bash
cargo install cargo-generate
cargo generate --git https://github.com/Colin4k1024/Oris.git --subfolder examples/templates/axum_service --name my-oris-service
cargo generate --git https://github.com/Colin4k1024/Oris.git --subfolder examples/templates/worker_only --name my-oris-worker
cargo generate --git https://github.com/Colin4k1024/Oris.git --subfolder examples/templates/operator_cli --name my-oris-ops
```

### Evo 实验路径（用于审查自演化能力）

如果你当前要验证的是 EvoKernel 的“已落地实现”而不是 HTTP 执行服务，请直接使用：

- `examples/evo_oris_repo`
- `docs/evokernel/README.md`

启用方式：

```bash
cargo add oris-runtime --features full-evolution-experimental
cargo run -p evo_oris_repo
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

这条路径对应当前仓库里真实可运行的实验闭环：

```text
AgentTask
-> MutationProposal
-> capture_from_proposal
-> feedback_for_agent
-> replay_or_fallback_for_run
```

需要明确两点：

1. 如果你只需要 `oris_runtime::evolution` 这一层 API，可只启用 `evolution-experimental`。
2. 仓库内现成示例与联调烟测依赖的是 `full-evolution-experimental`，因为它还会暴露 governor、agent contract、economics、spec 和 network 的实验 facade。
3. 如果你要让 replay 事件可直接关联到当前执行，优先使用 `replay_or_fallback_for_run`；旧的 `replay_or_fallback` 会自动生成 replay run id。

当前尚未形成可直接投产的自治闭环，仍缺少常驻调度、自动 issue intake、自动分支/发布编排等能力。

## 3. 如何接入现有 Rust 服务

推荐接入方式：

1. 如果你要把 Oris 直接挂进业务 HTTP 服务，先从 `examples/oris_starter_axum` 开始。
2. 如果控制面已独立部署、你只需要执行器，先从 `examples/oris_worker_tokio` 开始。
3. 如果你只需要 SRE/运营入口，先从 `examples/oris_operator_cli` 开始。
4. 使用 Tokio 运行时，避免阻塞式操作进入 async handler。
5. 默认启用 `sqlite-persistence`，将 `thread_id` 作为稳定业务键。
6. 为 run/resume/report-step 建立幂等键策略。
7. 全链路接入 `tracing`，日志字段至少包含 `thread_id`、`run_id`、`attempt_id`。

参考文档：

- `docs/rust-ecosystem-integration.md`
- `docs/kernel-api.md`
- `docs/production-operations-guide.md`
- `docs/incident-response-runbook.md`
- `docs/evokernel/README.md`
- `docs/evokernel/devloop.md`

## 4. 对外发布时的最小生产清单

在开源项目或团队正式对外使用前，建议至少满足：

1. **可恢复性**：完成崩溃恢复测试（checkpoint + event tail）。
2. **可重放性**：同一事件流重放结果等价，不产生额外副作用。
3. **可中断性**：interrupt 能被查询、恢复、审计。
4. **故障切换安全**：lease 过期可重排队，单 attempt 不并发执行。
5. **可观测性**：日志、指标、trace 可关联到具体 thread/run。
6. **操作面隔离**：operator API 与业务 API 分离，默认需要鉴权。
7. **变更可控**：在 CI 中加入关键回归测试（run/list/inspect/resume/replay/cancel）。
8. **实验能力隔离**：如果对外暴露 Evo 相关能力，必须保留 feature gate，并在 README/发布说明里明确标注 experimental 与当前未覆盖的自治能力边界。

## 5. 外部用户常见落地模式

- **模式 A：库嵌入**  
  将 Oris 作为应用内执行内核，直接复用你的 HTTP 层和鉴权体系。

- **模式 B：执行服务化**  
  独立部署 execution server，通过业务服务调用 `v1/jobs/*` 控制执行流。

- **模式 C：CLI + API 运维**  
  给 SRE/运营提供最小 operator CLI，覆盖 run/list/inspect/resume/replay/cancel。

## 6. 下一步建议

1. 先选最近的集成路径：`oris_starter_axum` / `oris_worker_tokio` / `oris_operator_cli`。
2. 再替换成你的业务 graph 节点或操作命令。
3. 为你的 `thread_id` 设计稳定主键规则（建议与业务实体一一映射）。
4. 把“崩溃恢复 + replay 等价 + interrupt 恢复”三类测试接入 CI。

## 7. 批量创建路线图 Issue（可选）

已提供脚本，可从 `docs/issues-roadmap.csv` 批量创建 GitHub Issues。
这个 CSV 现在同时也是路线图账本：前四列 `title/body/labels/milestone` 用于导入，后续归档与状态列仅用于追踪，导入脚本会自动忽略它们。

```bash
bash scripts/import_issues_from_csv.sh --repo Colin4k1024/Oris --create-milestones --create-labels
```

先预览不落库：

```bash
bash scripts/import_issues_from_csv.sh --repo Colin4k1024/Oris --dry-run --create-milestones --create-labels
```

导入后建议立刻做一次状态对账与编号回填（默认会在 `issue_number` 为空时按标题精确匹配回填）：

```bash
python3 scripts/sync_issues_roadmap_status.py --repo Colin4k1024/Oris
```

仅预览不落盘：

```bash
python3 scripts/sync_issues_roadmap_status.py --repo Colin4k1024/Oris --dry-run
```

如果只想同步某个路线（例如 EvoMap）：

```bash
python3 scripts/sync_issues_roadmap_status.py --repo Colin4k1024/Oris --track evomap-alignment --dry-run
```
