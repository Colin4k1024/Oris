# Test Plan — homepage-fix + oris-hub binary

| 字段 | 值 |
|------|-----|
| 任务 Slug | `2026-05-13-homepage-fix-oris-hub-binary` |
| 变更内容 | 新增 `crates/oris-hub/src/main.rs`；修订 `.github/scripts/gen_docs_site.py` |
| 主责角色 | qa-engineer |
| 状态 | review |
| 日期 | 2026-05-13 |

---

## 测试范围

### 功能范围

| 项目 | 说明 |
|------|------|
| `cargo build -p oris-hub` | 验证新 binary 编译通过（已在交付前确认：6.27s clean build） |
| `cargo run -p oris-hub` | 验证 hub 进程能绑定端口、打印启动信息并响应请求 |
| `HUB_ADDR` 环境变量 | 默认 `0.0.0.0:3000`，自定义值能正确 parse 为 `SocketAddr` |
| `HUB_DB_PATH` 环境变量 | 默认 `hub.db`，自定义路径传入 `HubConfig.db_path` |
| `gen_docs_site.py` 架构图 | Leaf layer 仅含正确叶节点；`oris-evolution` 已移至 Layer 1 |
| `gen_docs_site.py` feature flag | `oris-intake` 行不再显示 `intake-experimental`，改为 `—` |
| `gen_docs_site.py` crate 数量 | 显示 `23 library crates`（原 17） |

### 不覆盖项

- `HubServer` 内部路由逻辑、认证中间件（属独立功能范围）
- GitHub Actions CI 实际触发与 Pages 部署（依赖远端环境）
- `_site/index.html` 本地生成结果（gitignored，由 CI 重生成）

---

## 测试矩阵

| # | 场景 | 类型 | 前置条件 | 预期结果 |
|---|------|------|----------|----------|
| T1 | 默认参数编译 | 单元/编译 | workspace 干净 | `cargo build -p oris-hub` 0 errors |
| T2 | 无环境变量启动 | 集成 | 无 HUB_ADDR / HUB_DB_PATH | 绑定 `0.0.0.0:3000`，打印启动信息，写 `hub.db` |
| T3 | 自定义 HUB_ADDR | 集成 | `HUB_ADDR=127.0.0.1:4000` | 绑定 4000 端口 |
| T4 | 非法 HUB_ADDR | 集成 | `HUB_ADDR=notanaddr` | 进程 panic + 错误消息含 `HUB_ADDR must be a valid socket address` |
| T5 | 自定义 HUB_DB_PATH | 集成 | `HUB_DB_PATH=/tmp/test.db` | SQLite 文件在指定路径创建 |
| T6 | 架构图叶节点 | 文档 | 运行 gen_docs_site.py | Leaf layer 含且仅含 5 个正确节点 |
| T7 | oris-intake feature flag | 文档 | 运行 gen_docs_site.py | `oris-intake` 行 feature 列显示 `—` |
| T8 | Crate 数量 | 文档 | 运行 gen_docs_site.py | 页面显示 `23 library crates` |

---

## 风险与高风险路径

### 阻塞级风险（来自安全审查 H-1）

> **Dashboard routes 无认证（CRITICAL）**
> `routes.rs` 中 `/dashboard` 系列路由未应用 `verify_api_key` 中间件。
> 默认 bind `0.0.0.0:3000` 下，任何网络可达方均可无认证枚举全部 node 元数据和订阅 callback URL。
> **此问题不在本次变更引入，但 `main.rs` 的 `0.0.0.0` 默认值放大了其暴露面。**

### 高优先级风险（来自代码审查 + 安全审查）

| 编号 | 类型 | 问题 | 来源 |
|------|------|------|------|
| R-1 | HIGH (code) | `println!` 早于 TCP bind 触发，与 `server.rs` 内部 tracing 重复 | code-reviewer |
| R-2 | HIGH (code + security) | `TokenStore` 无 env var 种子路径；所有认证路由实际不可用 | 两者均报 |
| R-3 | HIGH (security) | `0.0.0.0` 默认覆盖了 `HubConfig::default()` 的保守值 `127.0.0.1:9090` | security-reviewer |
| R-4 | MEDIUM | 订阅 Store 硬编码 `:memory:`，重启后订阅数据丢失 | code-reviewer |
| R-5 | MEDIUM | 无 graceful shutdown；SIGTERM 下 in-flight 请求被强断 | code-reviewer |
| R-6 | MEDIUM | SSRF 过滤仅检查 IP，hostname 不过滤（DNS rebinding 可绕过）| security-reviewer |

### 非阻塞风险

| 编号 | 问题 | 建议 |
|------|------|------|
| R-7 | token 比较非常量时间 | 用 `subtle::ConstantTimeEq` 替换 |
| R-8 | 全局 rate limiter 非 per-IP | 切换为 keyed limiter |
| R-9 | CDN 资源无 SRI hash | 补 `integrity=` 属性 |
| R-10 | TOFU 注册模型未文档化 | 添加 doc comment 说明 |

---

## 放行建议

**建议在以下条件下放行当前变更（homepage fix + main.rs binary）：**

1. 本次 commit 本身不引入 dashboard 无认证问题（该问题在 `server.rs` 既存），但 `main.rs` 的 `0.0.0.0` 默认值是新增暴露；
2. 当前 `oris-hub` 尚无网络可达部署（仅作为 workspace crate 存在），实际暴露面为零；
3. **阻塞项**：在任何 `oris-hub` 对外部署前，必须先修复 H-1（dashboard 无认证）和 R-2（TokenStore 无种子），并将默认 bind 地址改回 `127.0.0.1`。

文档修复（gen_docs_site.py）：无阻塞项，建议直接放行。
