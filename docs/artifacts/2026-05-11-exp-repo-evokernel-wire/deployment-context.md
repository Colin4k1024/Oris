# Deployment Context

## 发布信息

| 字段 | 内容 |
|------|------|
| 任务 slug | exp-repo-evokernel-wire |
| 日期 | 2026-05-11 |
| 主责角色 | devops-engineer |
| 发布类型 | Library crate 功能补全（无服务部署） |
| 关联 PRD | docs/artifacts/2026-05-11-exp-repo-evokernel-wire/prd.md |

## 环境清单

| 环境 | 用途 | 访问入口 |
|------|------|----------|
| 本地 CI | cargo test --release --all-features | 本地 shell |
| crates.io | oris-runtime 发布 | cargo publish |
| GitHub Actions | 构建 + 测试门禁 | .github/workflows/ |

本次变更为纯 Rust 库层修改，无独立 HTTP 服务发布、无 Docker 镜像、无 Kubernetes manifests。

## 部署入口

| 入口 | 命令 | 前置条件 |
|------|------|----------|
| 主入口 | `cargo publish -p oris-runtime --all-features` | cargo test 全绿 |
| 手工验证 | `cargo build --all --release --all-features` | — |
| 回退入口 | `cargo add oris-runtime@<前一版本>` | 下游 crate 降版本 |

## 配置与密钥

本次变更不新增环境变量或密钥。涉及 Experience Repo HTTP 服务的密钥管理见 `oris-experience-repo` 文档：

| 配置项 | 用途 | 来源 |
|--------|------|------|
| `X-Api-Key` | Experience Repo HTTP 鉴权 | KeyStore（运行时生成） |
| Ed25519 公私钥 | OEN Envelope 签名验证 | 由 agent 在启动时生成 |
| `store_path` | SQLite Gene 存储路径 | ServerConfig |
| `key_store_path` | KeyStore 存储路径 | ServerConfig |

## 运行保障

- **Feature flag 隔离**：`full-evolution-experimental` — 所有网络推送路径均在此 flag 下，未开启时不影响稳定构建
- **network_publisher 可选**：`EvolutionNetworkNode` 的 `network_publisher` 字段为 `Option<Arc<dyn NetworkPublisher>>`，未配置时跳过推送路径，无副作用
- **监控**：无独立服务部署，监控依赖下游集成方的 tracing/metrics
- **观察窗口**：发布后 48h 观察 CI 绿灯状态、下游使用方编译报告

## 恢复能力

| 触发条件 | 回滚路径 | 验证方法 |
|----------|----------|----------|
| 下游编译失败 | 降回前一版本依赖 | `cargo test -p <affected-crate>` |
| async 传播引入运行时 panic | git revert 相关 commit | `cargo test --release --all-features` |
| NetworkPublisher 推送异常 | 移除 `network_publisher` 配置即可静默跳过 | 检查 `is_some()` guard |
