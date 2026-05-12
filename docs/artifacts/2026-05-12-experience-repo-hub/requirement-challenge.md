# Requirement Challenge Session

> 角色：tech-lead 主持
> 日期：2026-05-12
> 输入：PRD `experience-repo-hub`

---

## 挑战记录

### C-1: Hub 是否真的需要中心化？

**质疑**：OEN 网络层已有 gossip 同步能力，为什么还需要一个中心化 Hub？去中心化发现是否可行？

**结论**：保留 Hub 设计。理由：
1. Gossip 适合小规模（<20 节点）自发现，但跨广域网、跨组织时缺乏稳定的入口点。
2. Hub 可以做全局视图聚合，gossip 只能做局部传播。
3. 不排除未来 Hub of Hubs 或混合模式，但 v0.5.0 先做单 Hub 验证价值。
4. Hub 不做 Gene 内容存储，只做元数据索引，职责轻量。

### C-2: 联邦查询的一致性保证

**质疑**：多节点并行查询，结果可能不一致（节点间数据延迟、部分节点降级）。用户是否能接受 eventual consistency？

**结论**：接受 eventual consistency。Hub 查询结果标注 `freshness` 时间戳和 `coverage`（覆盖了多少节点/总节点数），让调用方自行判断。

### C-3: Dashboard 技术栈

**质疑**：Rust 生态前端方案（Leptos/Dioxus）成熟度有限，是否会成为维护负担？

**结论**：推荐 **embed static SPA** 方案。
- 前端使用 React + Vite 构建为静态资源。
- Hub 进程使用 axum 的 `ServeDir` 直接 serve 静态文件。
- 好处：前端开发不受 Rust 编译周期限制，部署时只需一个二进制。
- 前后端通过 Hub 自身 REST API 通信，无需额外 BFF。

### C-4: v0.4.0 P0 修复是否真的是阻塞项？

**质疑**：Hub 的注册发现和联邦查询能力似乎不依赖网络推送（那是节点间的 Gene 传播）。是否可以解耦？

**结论**：部分解耦。
- Phase 1（注册发现）和 Phase 2（联邦查询）不依赖 P0 修复，可以并行开发。
- Phase 3（订阅推送）依赖 Ed25519 签名正确性，需要 P0 修复。
- 调整里程碑：Phase 1-2 可立即启动，Phase 3 等 v0.4.0。

### C-5: 安全边界

**质疑**：Hub 作为中心节点，一旦被攻破可以操纵全网路由。安全设计是否足够？

**结论**：必须满足：
1. 节点注册需 Ed25519 签名验证（复用 OEN verifier）。
2. Hub API 对外暴露需 TLS + API Key。
3. 联邦查询只转发请求，不缓存/存储 Gene 内容。
4. Rate limit 防止注册/发现风暴。
5. Dashboard 需要独立的 admin auth（v0.5.0 使用简单 Bearer token，后续升级）。

### C-6: 单 Hub 单点故障

**质疑**：单 Hub 意味着单点故障。是否需要 HA？

**结论**：v0.5.0 不做 HA，但架构上预留：
- Hub 状态存储在 SQLite/PostgreSQL，可替换为共享存储。
- 节点心跳带 TTL，Hub 重启后节点重新注册即可恢复。
- 后续 v0.6.0 考虑 Hub 副本或 Hub of Hubs。

---

## 挑战结论

PRD 范围合理，以下调整纳入：
1. Phase 1-2 不依赖 v0.4.0，可立即启动。
2. Dashboard 使用 React SPA + embed 到 Hub 进程。
3. 联邦查询结果标注 `freshness` 和 `coverage`。
4. 安全边界明确写入 arch-design。

**就绪状态**：`ready-for-review`
**下一跳**：architect 输出 arch-design → tech-lead 主持 Design Review Board
