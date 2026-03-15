# Release v0.32.1

## Issue #243 — GeneStore SQLite CRUD + Solidify/Reuse 集成

### 变更摘要

实现了 `oris-genestore` SQLite 持久化层与进化 pipeline 的完整集成，
将 Solidify/Reuse 阶段从 placeholder 升级为真正的持久化路径。

### 新增内容

#### `oris-evolution`
- `port.rs`：新增 `GeneStorePersistPort` trait（trait-port 注入模式）
  - `persist_gene(gene_id, signals, strategy, validation) -> bool`
  - `mark_reused(gene_id, capsule_ids) -> bool`
- `pipeline.rs`：Solidify/Reuse 阶段接线
  - `StandardEvolutionPipeline::with_gene_store()` 构建器方法
  - Solidify 阶段：为每个候选基因调用 `GeneStorePersistPort::persist_gene`
  - Reuse 阶段：为每个候选基因调用 `GeneStorePersistPort::mark_reused`
  - `execute_stage` 同步路径同样接线
- 新增 `test_solidify_reuse_calls_gene_store` 集成测试（MockGeneStore）

#### `oris-genestore`
- `store.rs`：13 项单元测试覆盖全部 CRUD 路径
  - Gene upsert/get/delete、search_genes（tag 索引）、decay_all
  - record_gene_outcome（成功/失败）、stale_genes
  - Capsule CRUD + record_capsule_outcome
- `migrate.rs`：新增 JSONL → SQLite 一次性迁移工具
  - `migrate::from_jsonl(path, store) -> Result<usize>`
  - 跳过空行、`#` 注释行、JSON 解析失败行（带警告）
  - 2 项测试：roundtrip 迁移、无效行跳过

#### `oris-evokernel`
- `adapters.rs`：新增 `SqliteGeneStorePersistAdapter`
  - 实现 `GeneStorePersistPort`，将 oris-evolution Gene 映射到 oris-genestore Gene
  - 异步桥接模式与 `LocalSandboxAdapter` 一致
- `Cargo.toml`：添加 `oris-genestore = "0.1.0"` 和 `anyhow = "1.0"` 依赖

#### `oris-runtime`（集成测试）
- `evolution_feature_wiring.rs`：新增 `genestore_persist_adapter_resolves` 测试
  - 验证 `oris_runtime::evolution::adapters::SqliteGeneStorePersistAdapter` 可达
  - 验证 `:memory:` 内存数据库可正常打开

### 验收标准核对

- ✅ Gene 写入和读取通过 SQLite 持久化（13 个单元测试）
- ✅ 查询复杂度 O(log n) / O(1)（SQLite 索引：idx_gene_tags_tag、idx_genes_confidence）
- ✅ 现有 JSONL gene 可一次性迁移（`migrate::from_jsonl`）
- ✅ 与 oris-evolution Solidify/Reuse 路径集成测试通过（evolution_feature_wiring 2 passed）

### 验证

```
cargo fmt --all -- --check        ✅
cargo test -p oris-genestore      13 passed
cargo test -p oris-evolution      54 passed
cargo test --test evolution_feature_wiring --features full-evolution-experimental  2 passed
cargo build --all --release --all-features  ✅
cargo test --release --all-features  0 failures
```
