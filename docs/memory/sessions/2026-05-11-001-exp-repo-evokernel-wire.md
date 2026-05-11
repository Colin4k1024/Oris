# Session Summary — exp-repo-evokernel-wire Closeout

| 字段 | 值 |
|------|----|
| 日期 | 2026-05-11 |
| Slug | exp-repo-evokernel-wire |
| 角色 | tech-lead |
| 阶段 | follow-up-required |

## 链路起止

- **起点**：`/team-intake` → PRD 定义三个缺口（share_experience 缺失、EvoKernel 推送缺失、contributor_id 为 None）
- **终点**：`/team-closeout` → 确认 R1–R8 全部交付，识别 3 个 P0 阻塞问题，任务状态 follow-up-required

## 主要任务完成情况

| 工作项 | 状态 |
|--------|------|
| R1：ExperienceRepoClient.share_experience() | ✅ |
| R2：EvolutionNetworkNode 字段扩展 + builder | ✅ |
| R3：async 升格 + network push 路径 | ✅ |
| R4：GeneStore.get_gene() 实现 | ✅ |
| R5：NetworkPublisher DIP 抽象 + 实现 | ✅ |
| R6：contributor_id 填充 | ✅ |
| R7：全量 async call-site 修复（15 处） | ✅ |
| R8：Handler 测试修复（auth bootstrap） | ✅ |
| 579 tests 全绿 | ✅ |

## 主要产出

- `crates/oris-evokernel/src/core.rs` — NetworkPublisher 注入、maybe_push_to_network
- `crates/oris-evolution-network/src/publisher.rs` — NetworkPublisher trait + EvolutionNetworkPublisher
- `crates/oris-experience-repo/src/client/client.rs` — share_experience() 实现
- `crates/oris-experience-repo/src/server/handlers.rs` — contributor_id 填充 + 测试 fixtures
- `crates/oris-genestore/src/store.rs` — get_gene() 方法
- `docs/artifacts/2026-05-11-exp-repo-evokernel-wire/` — prd / test-plan / launch-acceptance / deployment-context / release-plan / closeout-summary

## 遗留事项

| 优先级 | 事项 |
|--------|------|
| P0 | `publish_envelope` 空签名（v0.4.0 修复前不开放 PKI push） |
| P0 | `sender_id` 语义错误（gene.id 非节点 ID） |
| P0 | Key 管理端点缺少 `verify_key` 鉴权（独立 issue，立即） |
| P1 | 3 处晋升路径未调用 maybe_push_to_network |
| P1 | ClientConfig Debug 暴露 api_key |
| P1 | reqwest 无超时 |
| P2 | E2E 集成测试 |

## 关键决策

1. `network_publisher: Option<Arc<dyn NetworkPublisher>>` — 可选设计确保向后兼容，未配置时静默跳过
2. `rebuild_projection()` 仅在 `network_publisher.is_some()` 时调用 — 避免无 publisher 节点的性能开销
3. Handler 测试 bootstrap 通过直接调用 `KeyStore::create_key()` — 不走 HTTP 层避免循环依赖
