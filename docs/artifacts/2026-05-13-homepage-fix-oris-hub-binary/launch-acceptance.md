# Launch Acceptance — homepage-fix + oris-hub binary

| 字段 | 值 |
|------|-----|
| 任务 Slug | `2026-05-13-homepage-fix-oris-hub-binary` |
| 验收时间 | 2026-05-13 |
| 验收角色 | qa-engineer |
| 验收方式 | 代码审查（code-reviewer + security-reviewer）+ 编译验证 |

---

## 验收范围

### In Scope

- `crates/oris-hub/src/main.rs` — 新增 binary 入口
- `.github/scripts/gen_docs_site.py` — 文档修订（架构图、feature flag、crate 数量）

### Out of Scope

- `HubServer` 内部路由、认证中间件、订阅持久化（独立功能迭代）
- CI/CD 端到端部署验证

---

## 验收证据

| 证据项 | 结论 |
|--------|------|
| `cargo build -p oris-hub` | PASS — 6.27s clean build（交付前已验证）|
| code-reviewer 审查 `main.rs` | 0 CRITICAL，2 HIGH（非本次新引入），2 MEDIUM，2 LOW |
| security-reviewer 审查两文件 | 1 CRITICAL（既存 `server.rs` 问题，`main.rs` 放大），2 HIGH，4 MEDIUM，4 LOW |
| `gen_docs_site.py` diff 正确性 | 架构图叶节点、feature flag、crate 数量均与 workspace 实际状态一致 |

---

## 风险判断

### 已满足项

- `main.rs` 本身无新增安全漏洞
- 文档修订均为事实性纠错，无功能影响
- `oris-hub` 当前无对外网络部署

### 可接受风险（本次放行）

- `println!` vs `tracing::info!` 不一致（LOW，运营影响小）
- CDN 资源无 SRI（LOW，docs 静态页面，影响面有限）

### 阻塞项（部署前必须修复，不阻塞代码合并）

| 编号 | 阻塞条件 | Owner | 修复期限 |
|------|----------|-------|----------|
| B-1 | Dashboard routes 无认证（`routes.rs`），在任何对外部署前必须修复 | — | 部署前 |
| B-2 | `TokenStore` 无 env var 种子，认证路由实际不可用，部署前必须补 `HUB_API_KEYS` 读取 | — | 部署前 |
| B-3 | `main.rs` 默认 `0.0.0.0:3000` 需改为 `127.0.0.1:3000` | — | 部署前 |

---

## 上线结论

| 对象 | 结论 |
|------|------|
| `gen_docs_site.py` 文档修订 | **允许合并上线** — 无阻塞项，修复准确 |
| `crates/oris-hub/src/main.rs` | **允许合并至仓库** — 但不允许在修复 B-1/B-2/B-3 前对外网络部署 |

**上线前提：** B-1、B-2、B-3 在任何 `oris-hub` 网络部署前完成修复并通过二次验证。

**观察重点（上线后）：** GitHub Pages CI 能正确重新生成 `_site/index.html`；架构图、crate 数量、feature flag 在线上页面显示与 py 脚本一致。

**确认记录：** qa-engineer 基于 code-reviewer + security-reviewer 双审查结果出具本结论，2026-05-13。
