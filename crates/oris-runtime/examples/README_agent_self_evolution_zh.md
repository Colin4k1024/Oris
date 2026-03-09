# Agent 自我进化示例（`agent_self_evolution`）

这个示例演示一个最小可复现闭环：

1. Agent 从运行遥测中识别异常（高延迟 + 重试热点 + 成功率下降）
2. 把异常转换为演化 signals 和 mutation proposal
3. 通过 EvoKernel 捕获 mutation，生成可复用 Gene/Capsule 资产
4. 在下一轮相似信号下优先 replay（自愈），必要时才 fallback

## 运行前提

- 在仓库根目录执行
- 需要开启 Evo 实验能力面

```bash
cargo run -p oris-runtime --example agent_self_evolution --features "full-evolution-experimental"
```

## 关键输出说明

示例会按步骤打印：

- `[1] Detected anomaly`：异常检测结果、severity、signals
- `[2] Capture mutation -> Gene/Capsule`：生成的 `gene_id`、`capsule_id`、confidence、state
- `[3] Replay similar anomaly`：是否命中已捕获资产（`used_capsule`）
- `[4] Agent feedback`：给 planner 的 replay/fallback 指令
- `[5] Artifact locations`：本次 demo 的 sandbox/store 路径

## 资产结构建议（可直接参考）

- `signals`：至少包含
  - 问题类型（如 `latency_spike`）
  - 场景维度（如 `op:planner_build_context`）
  - 自愈意图（如 `self_heal`）
- `proposal.expected_effect`：写成可验证结果（延迟下降、重试减少）
- `validation_plan`：尽量使用稳定、可重复命令（例如 `cargo check --lib`）
- `governor`：在演示环境可用低门槛提升（例如 `promote_after_successes=1`），生产环境应提高门槛

## 对应文件

- 示例代码：`crates/oris-runtime/examples/agent_self_evolution.rs`
- 本说明：`crates/oris-runtime/examples/README_agent_self_evolution_zh.md`
