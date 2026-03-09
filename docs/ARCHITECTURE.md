# Oris Evolution Core — Extension Modules

## Overview

These two crates fill the gaps identified in the Oris vs EvoMap evaluation:

| Gap | Crate |
|-----|-------|
| Mutation 语义退化无法检测 | `oris-mutation-evaluator` |
| JSONL 存储 O(n) 扫描瓶颈 | `oris-genestore` |

---

## oris-mutation-evaluator

### 两阶段评估门控

```
MutationProposal
       │
       ▼
┌──────────────────────┐
│   Static Analysis    │  ← 无 I/O，毫秒级
│   (deterministic)    │
└──────────┬───────────┘
           │ blocking AP?
     YES ──┴──► Reject (score=0, 跳过 LLM)
           │ NO
           ▼
┌──────────────────────┐
│     LLM Critic       │  ← 插件化，支持任意 provider
│  (pluggable trait)   │
└──────────┬───────────┘
           │
           ▼
    DimensionScores
    ┌──────────────────────────────────────┐
    │ signal_alignment     × 0.30          │
    │ semantic_correctness × 0.30          │
    │ generalisability     × 0.20          │
    │ test_coverage_delta  × 0.10          │
    │ complexity_impact    × 0.10          │
    └──────────────────────────────────────┘
           │ composite
    ≥ 0.72 → Promote (写入 Gene Pool)
    ≥ 0.45 → ApplyOnly (一次性使用，不固化)
    < 0.45 → Reject
```

### 检测的 Anti-Patterns

| 类型 | 是否阻断 | 例子 |
|------|---------|------|
| `NoOpMutation` | ✅ 阻断 | proposed == original |
| `HardcodedBypass` | ✅ 阻断 | `return 42;` 仅出现在 proposed |
| `TestDeletion` | ✅ 阻断 | proposed 中 `#[test]` 数量减少 |
| `ErrorSuppression` | ⚠️ 软警告 | 新增 `let _ =` / `unwrap_or_default()` |
| `BlastRadiusViolation` | ⚠️ 软警告 | 改动 >60% 但信号 ≤2 个 |

### 接入 Oris EvolutionPipeline

```rust
// 在 Validate → Evaluate 阶段之间插入
use oris_mutation_evaluator::{MutationEvaluator, MutationProposal};

let evaluator = MutationEvaluator::new(your_llm_critic);
let report = evaluator.evaluate(&mutation_proposal).await?;

match report.verdict {
    Verdict::Promote   => pipeline.solidify(proposal, report).await?,
    Verdict::ApplyOnly => pipeline.apply_once(proposal).await?,
    Verdict::Reject    => pipeline.discard(proposal, report.rationale).await?,
}
```

---

## oris-genestore

### 存储架构

```
┌─────────────────────────────────────────────────┐
│             SQLite (WAL mode)                   │
│                                                 │
│  genes table                                    │
│  ├── PRIMARY KEY: id (UUID)                     │
│  ├── INDEX: confidence  ← stale_genes() 快速查询 │
│  └── tags_json                                  │
│                                                 │
│  gene_tags table (normalized)                   │
│  ├── INDEX: tag          ← tag filter O(log n)  │
│  └── FOREIGN KEY → genes (CASCADE DELETE)       │
│                                                 │
│  capsules table                                 │
│  ├── INDEX: gene_id                             │
│  └── INDEX: confidence                          │
└─────────────────────────────────────────────────┘
```

### Confidence Lifecycle

```
Gene 新建      confidence = 0.70
                    │
              每次查询 decay
                    │ -0.002
                    ▼
              成功复用 boost        失败复用 penalty
                 +0.05                  -0.08
                    │                      │
                    └──────────┬───────────┘
                               ▼
                     < 0.30 → stale_genes()
                               │
                    触发 re-evolution cycle
```

### JSONL → SQLite 迁移

```bash
# 一行命令完成迁移
cargo run -p oris-genestore --example migrate \
  --genes-jsonl ./data/genes.jsonl \
  --capsules-jsonl ./data/capsules.jsonl \
  --output ./data/genes.db
```

```rust
// 或在代码中调用
use oris_genestore::{SqliteGeneStore, migrate_from_jsonl};

let store = SqliteGeneStore::open("genes.db")?;
let report = migrate_from_jsonl("genes.jsonl", "capsules.jsonl", &store).await?;
println!("Migrated {} genes, {} capsules, {} errors",
         report.genes, report.capsules, report.errors);
```

### 性能对比

| 操作 | JSONL (1000 genes) | SQLiteGeneStore |
|------|--------------------|-----------------|
| get_gene(id) | ~50ms (全扫描) | ~0.1ms (主键索引) |
| search_genes(tags=["lifetime"]) | ~50ms | ~0.5ms (tag 索引) |
| stale_genes() | ~50ms | ~0.2ms (confidence 索引) |
| decay_all() | 重写整个文件 | 单条 UPDATE，~1ms |
| 崩溃安全性 | 部分写入风险 | WAL 原子提交 |

---

## 集成路线图

```
Phase 1 (现在可以做)
  ├── oris-genestore 替换 oris-evolution 中的 JSONL 写入
  └── oris-mutation-evaluator 插入 EvolutionPipeline Evaluate 阶段

Phase 2 (下一步)
  ├── 实现真实 LlmCritic (OpenAI / Anthropic)
  └── 在 oris-evokernel 的 Signal 提取中自动填充 tags

Phase 3 (EvoMap 对齐)
  ├── GDI 评分 = composite_score × 使用权重
  └── PostgresGeneStore for 跨实例 Gene 共享
```
