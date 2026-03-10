# Agent Self-Evolution Travel Network 演示报告（2026-03-10）

## 1. 演示目标
验证以下闭环在真实 Qwen3-Max 调用下可完整执行：
- Producer 完成北京 -> 上海长线旅游规划
- 经验沉淀到 producer evolution store
- 经验发布（publish）并被 consumer 导入（import）
- Consumer 在相似任务上命中 replay，并完成新任务规划

## 2. 运行环境与命令
- 仓库：`/Users/jiafan/Desktop/work-code/Oris`
- 示例：`crates/oris-runtime/examples/agent_self_evolution_travel_network.rs`
- 特性：`full-evolution-experimental`
- 模型：`qwen:qwen3-max`

执行命令：
```bash
/bin/zsh -lc 'source ~/.zshrc >/dev/null 2>&1; cargo run -p oris-runtime --example agent_self_evolution_travel_network --features full-evolution-experimental'
```

## 3. 七阶段输出结果

### [1] Producer plan generated
- 成功生成北京 -> 上海 7 天计划（中文）
- 输出包含：交通方案、日程表、住宿建议、预算拆分、风险与备选

### [2] Experience captured to producer store
- `gene_id`: `8e84534b85555ff96d04c29aae315e52d7696ceee22e8cf55502e4f0fd129729`
- `capsule_id`: `97d48ff7197fbcb6fc31f43ac6ad2fc59ba3d2b1f2fc13aff0125eb966763e88`
- `gene_state`: `Promoted`

### [3] Envelope published
- `published_assets`: `3`

### [4] Consumer imported assets
- `accepted`: `true`
- `imported_asset_ids`: `2`

### [5] Consumer replay decision
- `used_capsule`: `true`
- `fallback_to_planner`: `false`
- `reason`: `replayed via cold-start lookup`

### [6] Consumer similar-task plan generated
- 成功生成相似任务：北京 -> 上海 10 天规划
- 输出结构完整（含交通/日程/住宿/预算/风险备选）

### [7] Metrics snapshot
- `replay_attempts_total`: `1`
- `replay_success_total`: `1`
- `replay_success_rate`: `1.00`

## 4. Store 产物与事件证据

### 路径
- Producer store: `/var/folders/m7/khm1j49j2fdg4bykvrldyyl00000gn/T/oris-travel-evo-producer-store`
- Consumer store: `/var/folders/m7/khm1j49j2fdg4bykvrldyyl00000gn/T/oris-travel-evo-consumer-store`

### 事件行数
- Producer `events.jsonl`: `8` 行
- Consumer `events.jsonl`: `12` 行

### Producer 事件统计
```text
mutation_declared: 1
mutation_applied: 1
validation_passed: 1
signals_extracted: 1
gene_projected: 1
promotion_evaluated: 1
gene_promoted: 1
capsule_committed: 1
```

### Consumer 事件统计
```text
mutation_declared: 1
remote_asset_imported: 2
gene_projected: 1
promotion_evaluated: 2
capsule_committed: 1
capsule_quarantined: 1
validation_passed: 1
gene_promoted: 1
capsule_released: 1
capsule_reused: 1
```

### 关键序列证据（consumer）
- `seq=4`: `promotion_evaluated -> Quarantined`（远端资产先隔离）
- `seq=9`: `promotion_evaluated -> Promoted`（本地 replay 验证后提升）
- `seq=12`: `capsule_reused`（`replay_run_id=travel-consumer-replay`）

## 5. 结论
本次演示在真实 Qwen3-Max 调用下完整跑通。验证了：
- 任务完成能力（Producer/Consumer 均能完成北京 -> 上海长线规划）
- 经验生命周期（capture -> publish -> import -> replay -> promote/reuse）
- 跨 Agent 可复用性（Consumer 在相似任务命中 replay 且不 fallback）
