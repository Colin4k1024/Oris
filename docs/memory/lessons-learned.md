# Lessons Learned

## 2026-04-14: experience-repo-phase2

### 场景
实现经验仓库二期 Share 功能（OEN Envelope + Ed25519 签名 + Key Service）

### 问题

#### 1. 签名验证实现依赖 PKI
**现象**：代码使用随机 UUID 作为公钥，导致签名验证完全被绕过
**根因**：设计时假设会有公钥注册表，但 MVP 阶段未实现
**教训**：架构设计中的依赖组件如果无法按时交付，应该显式禁用功能而非留下空壳实现

**建议**：复杂功能（签名验证）应该采用渐进式实现 - 先禁用功能并文档化，等依赖就绪再启用

### 场景
处理 rusqlite Connection 线程安全问题

### 问题

#### 2. 非 ThreadSafe 类型在多线程环境中的使用
**现象**：`rusqlite::Connection` 不是 `Sync`，无法直接放在 `Arc<Mutex<T>>` 中供多个线程使用
**解决**：使用 `Arc<Mutex<KeyStore>>` 包装，确保 Mutex 在正确位置

**教训**：与外部库集成时需要检查 trait 实现（Sync, Send），特别是数据库连接、文件句柄等资源

### 场景
依赖 API 变更

### 问题

#### 3. GeneStore trait 方法名变更
**现象**：`store.store_gene(&gene)` 编译失败
**根因**：实际方法是 `upsert_gene` 而非 `store_gene`
**教训**：使用外部 crate 时应先检查 trait 定义而非假设方法名

### 场景
安全评审发现

#### 4. MVP 阶段简化实现的取舍
**现象**：Ed25519 签名验证、rate limiting 等安全功能被推迟
**接受理由**：MVP 仅内部使用，API Key 提供基本访问控制
**教训**：安全与进度的取舍需要明确文档化，并在文档中标注为"已接受风险"

### 建议

1. **渐进式安全**：复杂安全功能（PKI、签名验证）应该分阶段实现，每阶段都能保持系统安全
2. **依赖检查**：集成外部 crate 前先检查 trait 实现（Sync, Send）
3. **空壳禁止**：如果功能无法实现，应该显式禁用而非留下空实现
4. **文档化取舍**：安全和功能的折衷需要明确文档化，包括接受的风险和后续计划

## 2026-04-14: experience-repo-pki (PKI 公钥注册表)

### 场景
实现 Ed25519 签名验证依赖 PKI 公钥注册表

### 问题

#### 1. Rate Limiting 基础设施 vs 完全集成
**现象**：实现了 RateLimiterRegistry，但只接入了 POST /experience，其他端点未保护
**教训**：基础设施就绪不等于功能就绪，需要明确标注"基础设施已完成但未完全接入"

#### 2. 签名缓存允许短时间重放攻击
**现象**：OenVerifier 中签名缓存 TTL 为 5 分钟，同一 envelope 可在此窗口内重放
**缓解**：攻击窗口有限，API Key 提供额外保护
**教训**：安全缓解措施需要量化说明（"5分钟窗口"而非"短期"）

#### 3. 公钥撤销后缓存未立即失效
**现象**：公钥撤销后，缓存公钥信息 5 分钟 TTL 后才失效
**缓解**：TTL 机制自动失效
**教训**：撤销场景需要明确缓存失效策略

### 建议

1. **安全功能分阶段**：Rate Limiting 基础设施完成后应立即完全接入所有端点，而非仅部分
2. **缓存策略显式化**：重放攻击窗口、缓存 TTL、撤销失效时间需要量化并在文档中标注
3. **API Key + 签名双重验证**：API Key 验证身份，Ed25519 签名验证消息完整性，不可相互替代

## 2026-04-14: 并行任务执行

### 场景
3 个遗留项并行执行

### 教训

#### 1. 并行任务执行加速交付
**现象**：3 个遗留项并行执行，25 分钟内全部完成
**教训**：独立的后端任务应该并行化，由不同 agent 同时处理

#### 2. 后台任务需要主动追踪
**现象**：部分 agent 任务 ID 记录有误，需要多次查询状态
**教训**：启动后台任务时立即记录完整 task ID，便于追踪

## 2026-05-11: Cargo workspace path dep vs registry dep

### 场景
在 workspace 内新增 trait 和字段后，使用 registry 版本锁定的 sibling crate 无法看到新符号。

### 问题
`oris-experience-repo` 的 `oris-genestore`、`oris-evolution`、`oris-evolution-network` 依赖原先通过 crates.io registry 锁定，Cargo 不会自动升级到 workspace 本地版本，导致 `NetworkPublisher` trait 和 `contributor_id` 字段编译不可见。

### 建议
workspace 内 sibling crate 一律用 `{ path = "../..." }` 引用；发版前检查 `Cargo.lock` 无 registry 来源的 workspace member。

## 2026-05-11: SQLite ALTER TABLE ADD COLUMN IF NOT EXISTS 版本门槛

### 场景
SQLite schema 幂等迁移。

### 问题
`ALTER TABLE ... ADD COLUMN IF NOT EXISTS` 需要 SQLite ≥ 3.37.0；libsqlite3-sys 0.30.1 bundled 版本较低，所有调用 `open()` 的测试 panic。

### 建议
幂等 ALTER TABLE 统一用 `let _ = conn.execute("ALTER TABLE t ADD COLUMN col TEXT", []);` 忽略"duplicate column name"错误，兼容全部 SQLite 3.x。

## 2026-05-11: exp-repo-evokernel-wire

### 场景

将 `EvolutionNetworkNode` 的两个方法（`ensure_builtin_experience_assets`、`record_reported_experience`）从同步升级为异步，以支持网络推送路径。

### 问题

#### 1. async 传播遗漏导致批量编译错误

**现象**：将 trait 方法改为 `async fn` 后，约 15 个调用点分布在 tests、handlers、helpers、examples 多个文件中，全部出现 `no method found` 或 `await inside non-async function` 编译错误。

**根因**：Rust 中将方法签名从 `fn` → `async fn` 属于破坏性变更，所有调用点必须：
1. 在已有 async 上下文中加 `.await`
2. 或将调用方自身升格为 `async fn`（并递归处理其调用方）

**教训**：
- 在动 widely-used struct 方法签名之前，先用 `grep -rn "\.ensure_builtin\|\.record_reported"` 枚举所有调用点
- 区分"调用方是 async 的"和"调用方是 sync 的"两类，后者需要递归升格（例如 sync helper → async helper → test 需改为 `#[tokio::test]`）
- 批量 async 改造建议在单独 commit 中完成，便于 blame 追溯

#### 2. HTTP 测试的 auth bootstrap 问题

**现象**：`test_create_and_list_key` 等测试在引入 API Key 鉴权后开始失败，返回 `ApiKeyMissing`。

**根因**：`create_key` handler 要求请求方已有合法 API Key（admin 鉴权），但测试使用的是空 `HeaderMap`。测试需要一个 bootstrap 机制绕过 HTTP 层、直接在 DB 中插入种子 key。

**教训**：
- HTTP 鉴权 handler 测试应提供两类 helper：`create_test_state_with_key()` 和 `create_authed_headers()`
- Bootstrap key 应通过直接调用底层 `KeyStore::create_key()` 插入，不走 HTTP 层（避免循环依赖）
- 每次新增鉴权 endpoint 时，同步更新相关测试的 auth setup

### 建议

- 改动 async 方法签名前，先做全局 grep 统计影响面
- 鉴权 handler 测试文件顶部维护 `test_state_with_auth()` 标准 fixture
