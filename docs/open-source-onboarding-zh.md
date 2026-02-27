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

仓库内已提供完整 starter 工程（Axum + Oris Runtime）：

- `examples/oris_starter_axum`
- `examples/templates`（三套可脚手架模板）

启动：

```bash
cargo run -p oris_starter_axum
```

默认地址：

- `ORIS_SERVER_ADDR=127.0.0.1:8080`
- `ORIS_SQLITE_DB=oris_starter.db`

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

如果希望直接生成你自己的项目骨架：

```bash
bash scripts/scaffold_example_template.sh axum_service /tmp/my-oris-service
bash scripts/scaffold_example_template.sh worker_only /tmp/my-oris-worker
bash scripts/scaffold_example_template.sh operator_cli /tmp/my-oris-ops
```

## 3. 如何接入现有 Rust 服务

推荐接入方式：

1. 在现有 Axum/Actix 服务中挂载 Oris API 路由（独立前缀）。
2. 使用 Tokio 运行时，避免阻塞式操作进入 async handler。
3. 默认启用 `sqlite-persistence`，将 `thread_id` 作为稳定业务键。
4. 为 run/resume/report-step 建立幂等键策略。
5. 全链路接入 `tracing`，日志字段至少包含 `thread_id`、`run_id`、`attempt_id`。

参考文档：

- `docs/rust-ecosystem-integration.md`
- `docs/kernel-api.md`

## 4. 对外发布时的最小生产清单

在开源项目或团队正式对外使用前，建议至少满足：

1. **可恢复性**：完成崩溃恢复测试（checkpoint + event tail）。
2. **可重放性**：同一事件流重放结果等价，不产生额外副作用。
3. **可中断性**：interrupt 能被查询、恢复、审计。
4. **故障切换安全**：lease 过期可重排队，单 attempt 不并发执行。
5. **可观测性**：日志、指标、trace 可关联到具体 thread/run。
6. **操作面隔离**：operator API 与业务 API 分离，默认需要鉴权。
7. **变更可控**：在 CI 中加入关键回归测试（run/list/inspect/resume/replay/cancel）。

## 5. 外部用户常见落地模式

- **模式 A：库嵌入**  
  将 Oris 作为应用内执行内核，直接复用你的 HTTP 层和鉴权体系。

- **模式 B：执行服务化**  
  独立部署 execution server，通过业务服务调用 `v1/jobs/*` 控制执行流。

- **模式 C：CLI + API 运维**  
  给 SRE/运营提供最小 operator CLI，覆盖 run/list/inspect/resume/replay/cancel。

## 6. 下一步建议

1. 先复制 `examples/oris_starter_axum`，替换成你的业务 graph 节点。
2. 为你的 `thread_id` 设计稳定主键规则（建议与业务实体一一映射）。
3. 把“崩溃恢复 + replay 等价 + interrupt 恢复”三类测试接入 CI。
