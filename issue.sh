#!/usr/bin/env bash
# 在 /Users/jiafan/Desktop/poc/Oris 目录下运行
# 前置：gh auth status 已通过

REPO="Colin4k1024/Oris"

# --- 先确保常用标签存在 ---
gh label create "evolution" --color "0075ca" --description "Self-evolution pipeline" --repo "$REPO" 2>/dev/null || true
gh label create "P0" --color "d93f0b" --description "Critical priority" --repo "$REPO" 2>/dev/null || true
gh label create "P1" --color "e4e669" --description "High priority" --repo "$REPO" 2>/dev/null || true
gh label create "P2" --color "0e8a16" --description "Medium priority" --repo "$REPO" 2>/dev/null || true
gh label create "P3" --color "bfd4f2" --description "Low priority" --repo "$REPO" 2>/dev/null || true
gh label create "phase-1" --color "c5def5" --description "Phase 1: Hardening & Stream A" --repo "$REPO" 2>/dev/null || true
gh label create "phase-2" --color "bfe5bf" --description "Phase 2: Task-Class & Orchestration" --repo "$REPO" 2>/dev/null || true
gh label create "phase-3" --color "fef2c0" --description "Phase 3: Confidence Control & LLM Mutation" --repo "$REPO" 2>/dev/null || true
gh label create "phase-4" --color "f9d0c4" --description "Phase 4: Federated Evolution & Release Gate" --repo "$REPO" 2>/dev/null || true

# =====================================================================
# Phase 1 — 硬化收尾（P0，截止 4/30 · Stream A）
# =====================================================================

gh issue create \
  --repo "$REPO" \
  --title "[Phase 1] Pipeline: 接入 SignalExtractor（Detect）和 Sandbox（Execute）阶段" \
  --label "evolution,P0,phase-1" \
  --body "## 背景

issue_146 中 \`StandardEvolutionPipeline\` 的 Detect 和 Execute 阶段目前是 stub，导致完整 8 阶段流无法端到端运行。

## 目标

- **Detect 阶段**：在 \`crates/oris-evolution/src/pipeline.rs\` 中接入 \`SignalExtractor\`，使信号可从运行时诊断（编译错误、panic、测试失败）流入 pipeline
- **Execute 阶段**：接入 \`oris-sandbox\` 沙箱，用来安全执行变异提案
- **指标暴露**：暴露每个 stage 的 duration 指标，供可观测性层消费

## 验收标准

- [ ] \`PipelineContext.signals\` 可由 SignalExtractor 正确填充
- [ ] Execute 阶段通过沙箱执行并记录 \`execution_result\`
- [ ] Stage duration 被写入 metrics
- [ ] \`cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental\` 通过

## 相关文件

- \`crates/oris-evolution/src/pipeline.rs\`
- \`crates/oris-evokernel/src/core.rs\`
- \`crates/oris-sandbox/src/\`

## 依赖

无（Phase 1 起点任务）"

echo "✅ Issue 1 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 1] GeneStore：实现 SQLite CRUD 替换 O(n) JSONL 扫描" \
  --label "evolution,P0,phase-1" \
  --body "## 背景

当前 \`oris-genestore\` 使用 JSONL 文件存储 gene，查询复杂度为 O(n)，在 gene 数量增加后将成为性能瓶颈。实现度约 30%，架构已定义但持久化逻辑缺失。

## 目标

- 在 \`crates/oris-genestore/src/\` 中实现基于 SQLite 的 Gene CRUD（Create/Read/Update/Delete）
- 支持按 task-class、confidence score、gene id 等字段索引查询
- 提供从现有 JSONL 资产迁移的工具或脚本

## 验收标准

- [ ] Gene 写入和读取通过 SQLite 持久化
- [ ] 查询复杂度降至 O(log n) 或 O(1)（索引命中）
- [ ] 现有 JSONL gene 可一次性迁移
- [ ] 与 \`oris-evolution\` 的 Solidify/Reuse 路径集成测试通过

## 相关文件

- \`crates/oris-genestore/src/\`
- \`crates/oris-evolution/src/pipeline.rs\`（Solidify/Reuse 阶段）

## 依赖

无（可与 Issue #1 并行）"

echo "✅ Issue 2 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 1] Stream A CI 门控：evo_oris_repo 补全示例 + evolution_feature_wiring 回归门控" \
  --label "evolution,P0,phase-1" \
  --body "## 背景

Stream A（3/5-4/30）的目标是"可检查的示例场景 + CI 门控"。当前 \`examples/evo_oris_repo/\` 已有基础框架，但尚未覆盖完整的自我进化场景，且 \`evolution_feature_wiring\` 测试未在 CI 中作为门控运行。

## 目标

- 在 \`examples/evo_oris_repo/\` 中补全可展示完整 Detect→Solidify→Reuse 路径的示例场景
- 在 CI pipeline 中添加以下测试作为 merge 门控：
  \`\`\`
  cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
  \`\`\`
- 确保 fail-closed 错误码（PolicyDenied、ValidationFailed、UnsafePatch、Timeout）在示例中有对应覆盖

## 验收标准

- [ ] 示例可端到端演示有界监督式闭环
- [ ] CI 在 \`full-evolution-experimental\` feature 下运行 wiring 测试
- [ ] PR 在 wiring 测试失败时被阻断合并

## 相关文件

- \`examples/evo_oris_repo/\`
- CI 配置文件（.github/workflows/）

## 依赖

建议在 Issue #1（Detect/Execute 集成）完成后运行，但可以并行推进示例骨架"

echo "✅ Issue 3 created"

# =====================================================================
# Phase 2 — Task-Class Generalization & 有界编排（P1，4/1-6/30 · Stream B）
# =====================================================================

gh issue create \
  --repo "$REPO" \
  --title "[Phase 2] Task-Class Generalization：语义等价层设计与实现" \
  --label "evolution,P1,phase-2" \
  --body "## 背景

当前 Reuse 路径基于精确 signal 匹配，无法跨语义等价任务复用已学习的 gene。接纳清单中 Task-Class Generalization 是 \`Next\` 级别的优先项，目标是"一次学习、语义等价任务均可命中"。

## 目标

- 在 \`crates/oris-evolution/\` 中设计和实现 **task-class 抽象**：
  - 定义 \`TaskClass\` 类型（代表一类语义等价任务）
  - 扩展 \`Selector\` 逻辑：候选 gene 按 task-class 匹配，而非只按精确 signal 字符串
  - 扩展 \`Reuse\` 路径：命中 task-class 时触发 replay
- 设计语义等价的判定规则（初期可基于规则，后续可接 LLM embedding）

## 验收标准

- [ ] 同一 task-class 下不同 signal 变体均可命中已学习 gene
- [ ] 跨 task-class 无误匹配（假阳性 = 0%）
- [ ] 单元测试覆盖正向/负向/边界场景

## 相关文件

- \`crates/oris-evolution/src/\`（Selector、Reuse 路径）
- \`crates/oris-evokernel/src/core.rs\`

## 依赖

Phase 1 完成后开工（GeneStore SQLite 稳定是前提）"

echo "✅ Issue 4 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 2] Intake：CI/CD 集成（GitHub webhook + 监控告警信号接入）" \
  --label "evolution,P1,phase-2" \
  --body "## 背景

\`oris-intake\` 当前实现度约 30%：框架和 deduplication 接口已定义，但缺乏与真实信号源（GitHub Actions CI 失败、监控告警）的集成，导致 Detect 阶段仍需手动触发。

## 目标

- 在 \`crates/oris-intake/src/\` 中实现以下信号接入：
  - **GitHub webhook**：接收 CI 失败事件（check_run failed、test failure）并转成 \`RuntimeSignal\`
  - **监控告警**（可选：Prometheus alertmanager webhook 格式）
- 完整的 **signal deduplication**：相同根因的重复信号合并为一条
- **优先级分类**：按影响面、频率等维度打分排序

## 验收标准

- [ ] GitHub CI 失败事件可自动流入 Intake
- [ ] 重复信号被正确去重（deduplication 命中率 ≥ 95%）
- [ ] 优先级分类结果与预期标签一致
- [ ] 集成测试覆盖 webhook → intake → signal 对象转换链路

## 相关文件

- \`crates/oris-intake/src/\`

## 依赖

可与 Issue #4（Task-Class Generalization）并行推进"

echo "✅ Issue 5 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 2] Orchestrator：端到端 Pipeline 自动编排（intake → selector → mutation → sandbox）" \
  --label "evolution,P1,phase-2" \
  --body "## 背景

\`oris-orchestrator\` 当前实现度约 20%，协调合约已定义但端到端的自动任务分配和 pipeline 调度逻辑缺失，目前流程仍需手工组装各模块。

## 目标

实现 \`crates/oris-orchestrator/src/\` 中的完整编排逻辑：

1. 从 Intake 读取已优先级化的信号
2. 调用 Selector 选取候选 gene / 变异方案（依赖 Task-Class 抽象）
3. 触发 mutation 提案生成
4. 提交到 Sandbox 安全执行
5. 收集执行结果并路由到 Validate/Evaluate 阶段
6. 在 Acceptance Gate 触发人工审核（fail-closed 原则）

## 验收标准

- [ ] 端到端 intake → execution 链路在 supervised devloop 中无需手工介入
- [ ] fail-closed：PolicyDenied / ValidationFailed / UnsafePatch / Timeout 均正确中止并记录
- [ ] 集成测试覆盖 success path + 每种 fail-closed 路径

## 相关文件

- \`crates/oris-orchestrator/src/\`
- \`crates/oris-intake/src/\`
- \`crates/oris-evolution/src/pipeline.rs\`

## 依赖

**依赖 Issue #4（Task-Class Generalization）和 Issue #5（Intake 集成）完成后开工**"

echo "✅ Issue 6 created"

# =====================================================================
# Phase 3 — 置信度控制 & LLM 变异（P2，目标 Q3）
# =====================================================================

gh issue create \
  --repo "$REPO" \
  --title "[Phase 3] ConfidenceController：连续置信度控制与 stale 资产自动降级" \
  --label "evolution,P2,phase-3" \
  --body "## 背景

接纳清单 Section 7 中，Continuous Confidence Control 是 \`+3\` 级别的下一阶段目标。当前已有 confidence-control-design.md 设计文档，但 \`ConfidenceController\` 未实现。stale/失效的 gene 可能导致错误重用。

## 目标

按 \`docs/plans/confidence-control-design.md\` 设计在 \`crates/oris-evolution/\` 中实现：

- \`ConfidenceController\` 组件：追踪每个 gene/capsule 的置信度分数
- 自动降级逻辑：当 gene 在一定时间窗口内失败率超阈值时，降低其置信度分数直至暂停复用
- 重新验证触发器：置信度降级后自动触发再验证流程

## 验收标准

- [ ] 置信度分数随成功/失败记录动态更新
- [ ] stale 资产在阈值内自动降级，不再被 Selector 优先选取
- [ ] 降级事件写入可观测性日志
- [ ] 单元测试覆盖典型衰减 + 恢复场景

## 相关文件

- \`crates/oris-evolution/src/\`
- \`docs/plans/confidence-control-design.md\`

## 依赖

依赖 Phase 2（Reuse 路径 + Task-Class 抽象）稳定后开工"

echo "✅ Issue 7 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 3] LLM 驱动变异与评估：通过 oris_runtime::llm 接入内置多后端" \
  --label "evolution,P2,phase-3" \
  --body "## 背景

当前 Mutate 阶段（70% 实现）和 Evaluate 阶段（70% 实现）仅支持基于规范/模式的变异和静态分析评估，缺乏 LLM 驱动的创意变异能力。

决策：**直接复用 \`oris_runtime::llm\` 内置多后端**（OpenAI/Claude/Ollama 统一抽象），不引入独立外部 agent。

## 目标

- 在 \`crates/oris-mutation-evaluator/src/\` 中实现 LLM 后端集成：
  - 通过 \`oris_runtime::llm\` trait 调用已有多后端（OpenAI/Claude/Ollama）
  - 设计 Mutate 阶段的 **LLM prompt 合约**（输入：signal + 上下文；输出：结构化变异提案）
  - LLM 辅助 Evaluate：对静态分析结果进行语义增强，输出置信度评分
- 保留静态分析作为 LLM 评估的底线安全保障（双轨并行）

## 验收标准

- [ ] LLM 可生成格式正确的变异提案（通过 proposal contract 校验）
- [ ] LLM 评估输出置信度分数，与静态分析结论一致性 ≥ 80%
- [ ] 支持通过 env var 切换后端（OPENAI_API_KEY / ANTHROPIC_API_KEY / OLLAMA_HOST）
- [ ] 集成测试覆盖 LLM mutation → sandbox 执行 → validate 链路

## 相关文件

- \`crates/oris-mutation-evaluator/src/\`
- \`crates/oris-runtime/src/llm/\`（复用现有 LLM trait）
- \`crates/oris-evokernel/src/core.rs\`（Mutation Proposal 合约）

## 依赖

可与 Issue #7（ConfidenceController）并行推进；依赖 Phase 2 全链路稳定"

echo "✅ Issue 8 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 3] Agent Feedback Loop：重放成功记录反馈至 agent 推理路径" \
  --label "evolution,P2,phase-3" \
  --body "## 背景

接纳清单 Section 7 中，Agent Feedback Loop 是 \`+4\` 级别目标：将 replay 成功的经验反馈回 agent 推理路径，使 agent 在处理重复任务时逐步降低推理延迟和 token 消耗。

## 目标

- 设计 agent 推理路径中的 replay 优先策略：在 agent plan() 执行前先查询 GeneStore，若有高置信度 gene 命中，直接进入 replay path（跳过完整 LLM 推理）
- 记录 replay 触发时机、命中率、推理时间节省等指标
- 在 agent 循环中植入 feedback hook，将 replay 结果写回 gene 的使用历史

## 验收标准

- [ ] 高置信度 gene 触发时，agent 推理延迟相比冷启动降低可测量
- [ ] feedback 数据正确写入 GeneStore 使用历史
- [ ] 低置信度场景不触发提前返回（安全保障）

## 相关文件

- \`crates/oris-runtime/src/agent/agent.rs\`
- \`crates/oris-genestore/src/\`
- \`crates/oris-evolution/src/\`（Reuse 路径）

## 依赖

依赖 Issue #7（ConfidenceController）和 Issue #8（LLM 变异）完成"

echo "✅ Issue 9 created"

# =====================================================================
# Phase 4 — 联合进化 & 发布门控（P3-P5，Q3-Q4 · Stream C+D）
# =====================================================================

gh issue create \
  --repo "$REPO" \
  --title "[Phase 4] Evolution Network：实现完整 Gossip 同步协议" \
  --label "evolution,P3,phase-4" \
  --body "## 背景

\`oris-evolution-network\` 当前实现度约 40%：A2A 协议类型已定义，但多节点间的 Gossip 同步逻辑缺失，导致联合进化（Federated Evolution）无法실제 运行。

## 目标

按 \`docs/plans/federated-evolution-hardening-design.md\` 实现：

- 完整的 **Gossip 协议**：节点间 gene/capsule 资产的增量同步
- **隔离生命周期**：远程资产先进入隔离区，本地验证通过后才可复用
- **可靠性策略**：网络分区、消息丢失场景下的一致性保障（目标：远程资产正确性 ≥ 99.5%）

## 验收标准

- [ ] 两节点间 gene 同步端到端测试通过
- [ ] 隔离 → 验证 → 启用 生命周期正确执行
- [ ] 网络故障场景下不引入未验证 gene

## 相关文件

- \`crates/oris-evolution-network/src/\`
- \`docs/plans/federated-evolution-hardening-design.md\`

## 依赖

依赖 Phase 3 全链路（置信度控制 + LLM 变异）稳定"

echo "✅ Issue 10 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 4] Economics：补全 EVU 账本计算逻辑" \
  --label "evolution,P3,phase-4" \
  --body "## 背景

\`oris-economics\` 当前实现度约 70%：EVU（Evolution Value Unit）账本结构已定义，但核心计算逻辑（EVU 如何根据 replay ROI 计算、如何在节点间结算）尚未完成。

## 目标

按 \`docs/plans/replay-feedback-roi-stability-design.md\` 完成：

- EVU 计算逻辑：根据 gene 被成功复用次数、节省的推理成本等计算 EVU 增量
- 账本记录：每次 replay 触发时写入 EVU 交易记录
- ROI 稳定性：防止 EVU 通胀/偏移，确保长期账本可信

## 验收标准

- [ ] EVU 在 replay 成功后正确累积
- [ ] 账本序列在重启后可恢复（持久化）
- [ ] ROI 指标在测试场景中与预期基线偏差 < 5%

## 相关文件

- \`crates/oris-economics/src/\`
- \`docs/plans/replay-feedback-roi-stability-design.md\`

## 依赖

可与 Issue #10（Gossip 协议）并行推进"

echo "✅ Issue 11 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 4] Supervised DEVLOOP 扩展：扩大有界自主任务覆盖范围" \
  --label "evolution,P3,phase-4" \
  --body "## 背景

接纳清单 Section 7 中，Supervised DEVLOOP 是 \`+5\` 级别目标。当前 supervised devloop（\`oris-evokernel\`）已实现针对有界任务子集的监督式闭环，本任务目标是扩大其任务类型覆盖范围，使更多类型的开发工作可进入有界自主执行。

## 目标

- 梳理并定义可扩展的新任务类型（如：文档更新、依赖升级、lint 修复等）
- 在 \`oris-evokernel\` 的 supervised devloop 中添加对新任务类型的支持
- 每类新任务类型需配备对应的 safety boundary 规则（防止越界）
- 与 Stream B 中 Task-Class Generalization 结果协同

## 验收标准

- [ ] 至少 2 种新任务类型可进入 supervised devloop
- [ ] 新任务类型的 fail-closed 边界测试全部通过
- [ ] 人工干预节点数量相比当前可量化减少

## 相关文件

- \`crates/oris-evokernel/src/core.rs\`
- \`examples/evo_oris_repo/\`

## 依赖

依赖 Phase 2（Task-Class Generalization）和 Phase 3（ConfidenceController）完成"

echo "✅ Issue 12 created"

# ---

gh issue create \
  --repo "$REPO" \
  --title "[Phase 4] Issue-to-Release 自主闭环：打通 issue discovery → mutation → PR → release 全链路" \
  --label "evolution,P3,phase-4" \
  --body "## 背景

接纳清单 Section 6 列出的 6 个现有差距，Stream D（10/1-12/31）的终态目标，也是 Oris 自我进化的最终里程碑：**去除 issue 发现、变异提案生成、PR/release 等环节的手工干预节点**，实现受 Safety Gate 约束的自主软件开发闭环。

## 目标

1. **自主 issue discovery**：从监控/CI 信号自动生成高质量 issue（依赖 Intake 集成）
2. **自主 mutation proposal 生成**：LLM 驱动生成变异提案，通过 proposal contract 校验（依赖 LLM 变异）
3. **自主 PR delivery**：生成 branch + PR，包含 evidence 和 diff（当前 W8 #237 已实现有界版本）
4. **发布门控自动化**：通过 CI 门控后自动触发 \`cargo publish\` 流程

## 当前已有基础（W8 成果）

- #237：Bounded Branch & PR Delivery ✅
- #238：Acceptance Gate ✅
- Supervised devloop ✅

## 验收标准

- [ ] 从 CI 失败信号到 PR 创建全程无需手工操作
- [ ] Acceptance Gate 仍作为可配置的人工/自动审批节点
- [ ] issue→merge→release 完整链路在 staging 环境端到端测试通过

## 相关文件

- \`crates/oris-orchestrator/src/\`
- \`crates/oris-intake/src/\`
- \`crates/oris-evokernel/src/core.rs\`
- \`examples/evo_oris_repo/\`

## 依赖

**依赖 Issues #10、#11、#12（Phase 4 其他任务）全部完成后开工**"

echo "✅ Issue 13 created"

echo ""
echo "🎉 全部 13 个 issue 创建完成！"
echo "查看：gh issue list --repo $REPO --label evolution"