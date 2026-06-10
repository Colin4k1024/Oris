# Multi-Language SDK Capability Audit

**Date:** 2026-06-10  
**Auditor:** Claude Code (Opus 4.6)  
**Scope:** Client Agent 使用经验 / 上报经验 / 注册 Hub — 多语言 SDK 覆盖情况

---

## 总结判断

| 维度 | TypeScript | Python | Go | Java |
|------|:---:|:---:|:---:|:---:|
| 使用经验（Fetch） | ✅ | ✅ | ✅ | ❌ 缺失 |
| 上报经验（Share） | ✅ | ✅ | ✅ | ❌ 缺失 |
| Ed25519 签名 | ✅ | ✅ | ✅ | ❌ 缺失 |
| 注册公钥（PKI） | ✅ | ✅ | ✅ | ❌ 缺失 |
| Hub 节点注册 | ✅ | ✅ | ✅ | ❌ 缺失 |
| Hub 心跳 | ✅ | ✅ | ✅ | ❌ 缺失 |
| Hub 节点发现 | ✅ | ✅ | ✅ | ❌ 缺失 |
| Hub 联邦搜索 | ✅ | ✅ | ✅ | ❌ 缺失 |
| Hub 订阅/退订 | ✅ | ✅ | ✅ | ❌ 缺失 |
| 本地 Gene Store | ✅ SQLite | ✅ SQLite | ✅ SQLite | ❌ 缺失 |
| MySQL Store | ✅ | ✅ | ✅ | ❌ 缺失 |
| Sync Manager | ✅ | ✅ | ✅ | ❌ 缺失 |
| Execution Client | ✅ | ✅ | ✅ | ❌ 缺失 |
| 单元测试 | ✅ | ✅ | ✅ | ❌ 缺失 |
| OpenAPI Spec | ✅ 共享 | ✅ 共享 | ✅ 共享 | ✅ 可用 |

**结论：TypeScript / Python / Go 三语言完全满足；Java SDK 完全缺失。**

---

## 一、已实现语言 SDK 详细能力矩阵

### TypeScript SDK (`sdks/typescript/`)

| 模块 | 文件 | 能力 |
|------|------|------|
| `ExperienceClient` | `experience.ts` | share (Ed25519签名) / fetch / registerPublicKey |
| `HubClient` | `hub.ts` | register / heartbeat / discover / search / subscribe / unsubscribe / listSubscriptions |
| `ExecutionClient` | `execution.ts` | runJob / getJob / listJobs |
| `LocalStore` | `store.ts` | SQLite (better-sqlite3) — save/get/delete/query/updateStats/getUnsynced/markSynced/logSync |
| `MySQLStore` | `mysql-store.ts` | MySQL (mysql2) — 同 LocalStore 接口 |
| `SyncManager` | `sync.ts` | pushToHub / pullFromHub / getSyncLog — 含冲突检测与统计合并 |
| `signing` | `signing.ts` | signBody / signPayload / publicKeyBase64 / publicKeyHex (@noble/ed25519) |
| 测试 | `tests/*.test.ts` | experience / hub / signing / store / sync / execution / mysql-store |

**包发布：** `@colin4k1024/oris-sdk@0.3.0` (npm)

---

### Python SDK (`sdks/python/`)

| 模块 | 文件 | 能力 |
|------|------|------|
| `ExperienceClient` | `experience.py` | share / fetch / register_public_key |
| `HubClient` | `hub.py` | register / heartbeat / discover / search / subscribe / unsubscribe / list_subscriptions |
| `ExecutionClient` | `execution.py` | run_job / get_job / list_jobs |
| `LocalStore` | `store.py` | SQLite — save/get/delete/query/update_stats/list_genes/get_unsynced/mark_synced/log_sync |
| `MySQLStore` | `mysql_store.py` | MySQL — 同 StoreProtocol 接口 |
| `SyncManager` | `sync_manager.py` | push_to_hub / pull_from_hub / register_node / get_sync_log |
| `StoreProtocol` | `store_protocol.py` | typing.Protocol — 存储层抽象接口 |
| `signing` | `signing.py` | sign_body / sign_payload / public_key_base64 / public_key_hex |
| 测试 | `tests/*.py` | test_experience / test_hub / test_signing / test_store / test_sync / test_execution / test_mysql_store |

**包发布：** `oris-rt-sdk==0.3.0` (PyPI)  
**HTTP 客户端：** httpx

---

### Go SDK (`sdks/go/`)

| 模块 | 文件 | 能力 |
|------|------|------|
| `experience.Client` | `experience/client.go` | Share / Fetch / RegisterPublicKey |
| `hub.Client` | `hub/client.go` | Register / Heartbeat / Discover / Search / Subscribe / Unsubscribe / ListSubscriptions |
| `execution.Client` | `execution/client.go` | RunJob / GetJob / ListJobs |
| `store.Store` | `store/iface.go` | interface — Save/Get/Delete/Query/UpdateStats/List/GetUnsynced/MarkSynced/LogSync/GetSyncLog |
| `store.SQLiteStore` | `store/store.go` | SQLite 实现 |
| `store.MySQLStore` | `store/mysql.go` | MySQL 实现 |
| `sync.SyncManager` | `sync/sync.go` | PushToHub / PullFromHub / RegisterNode / GetSyncLog |
| `internal.signing` | `internal/signing.go` | SignBody / SignPayload / PublicKeyBase64 / PublicKeyHex (crypto/ed25519) |
| 测试 | `*_test.go` | experience / hub / signing / store / sync / execution / mysql |

**包发布：** `github.com/Colin4k1024/Oris/sdks/go@v0.3.0` (go module)

---

## 二、跨语言一致性保障

| 保障机制 | 状态 |
|----------|------|
| OpenAPI Spec（Hub / Execution / Experience） | ✅ `sdks/spec/*.yaml` |
| OEN Envelope Spec | ✅ `sdks/spec/oen-envelope-spec.md` |
| Signing Spec | ✅ `sdks/spec/signing-spec.md` |
| Golden Test Fixtures | ✅ `sdks/spec/golden/` |
| 统一 Store 接口抽象 | ✅ 三语言接口方法名对齐 |
| 统一 SyncManager 语义 | ✅ push/pull/conflict/merge 逻辑一致 |

---

## 三、Agent 典型工作流覆盖

### 1. 使用经验（消费）

```
Agent → ExperienceClient.fetch(q="error_type", min_confidence=0.7) → Gene[]
Agent → LocalStore.save(gene) → 本地持久化
```

**三语言均完整实现。** 支持信号匹配、置信度过滤、分页游标。

### 2. 上报经验（生产）

```
Agent → LocalStore.save(gene) → 本地写入
Agent → SyncManager.pushToHub() → 自动获取 unsynced genes → Ed25519 签名 → POST /experience
Agent → LocalStore.markSynced() → 标记已同步
```

**三语言均完整实现。** 含错误处理、sync log 审计、重试标记。

### 3. 注册到 Hub

```
Agent → HubClient.register(endpoint, capabilities, version, region)
Agent → HubClient.startHeartbeatLoop() // 或手动 heartbeat()
Agent → HubClient.subscribe(callbackUrl, filter)
```

**三语言均完整实现。** 节点注册含 Ed25519 签名防护。

### 4. Local-First 架构

```
┌─────────────┐     push      ┌──────────────────┐
│ Local Store │ ───────────── │ Experience Repo  │
│  (SQLite/   │ ◄─────────── │   (Remote Hub)   │
│   MySQL)    │     pull      └──────────────────┘
└─────────────┘
     │ SyncManager: pushToHub / pullFromHub / conflict detection / merge stats
```

**三语言均实现完整的 Local-First 同步循环。**

---

## 四、Java SDK 缺失分析

### 当前状态

- **完全不存在**：`sdks/` 下无 Java 目录，无 `.java` 文件，无 `pom.xml` / `build.gradle`
- OpenAPI Spec 可用作 Java SDK 代码生成的基础

### 需要实现的模块

| 模块 | 对应接口 | 预估工作量 |
|------|----------|-----------|
| `ExperienceClient` | share / fetch / registerPublicKey | ~150 LOC |
| `HubClient` | register / heartbeat / discover / search / subscribe / unsubscribe | ~200 LOC |
| `ExecutionClient` | runJob / getJob / listJobs | ~100 LOC |
| `Ed25519Signing` | signBody / signPayload / publicKeyBase64 / publicKeyHex | ~80 LOC |
| `GeneStore` (interface) | save / get / delete / query / updateStats / getUnsynced / markSynced / logSync | ~50 LOC |
| `SqliteGeneStore` | SQLite 实现 | ~200 LOC |
| `MySQLGeneStore` | MySQL 实现 | ~200 LOC |
| `SyncManager` | pushToHub / pullFromHub / registerNode / getSyncLog | ~150 LOC |
| 单元测试 | 全模块 | ~300 LOC |
| **合计** | | **~1,430 LOC** |

### 建议技术选型

| 关注点 | 建议 |
|--------|------|
| HTTP 客户端 | OkHttp / java.net.http (JDK 11+) |
| Ed25519 | Bouncy Castle (`org.bouncycastle:bcprov-jdk18on`) 或 Tink |
| JSON | Jackson (`com.fasterxml.jackson`) |
| SQLite | `org.xerial:sqlite-jdbc` |
| MySQL | `com.mysql:mysql-connector-j` |
| 构建工具 | Maven + `maven-publish` 到 Maven Central |
| 包名 | `io.oris:oris-sdk` |
| 最低 JDK | 11 (LTS) |

---

## 五、整体评估

| 评估项 | 结论 |
|--------|------|
| TypeScript SDK | ✅ 完整，生产可用，npm 已发布 |
| Python SDK | ✅ 完整，生产可用，PyPI 已发布 |
| Go SDK | ✅ 完整，生产可用，go module 已发布 |
| Java SDK | ❌ **不满足** — 完全缺失 |
| 跨语言协议一致性 | ✅ OpenAPI + Signing Spec + Golden Fixtures 保障 |
| Local-First 存储 | ✅ SQLite + MySQL 双后端，三语言统一接口 |
| Sync Manager | ✅ push/pull/conflict/merge 三语言语义对齐 |

### 最终结论

**三语言满足（TS/Python/Go），Java 不满足。** 若需支持 Java Agent 生态，需新增 `sdks/java/` 实现，预估 ~1,430 LOC，可基于现有 OpenAPI Spec 加速开发。
