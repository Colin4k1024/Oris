# Oris SDK 集成指南

Oris 通过三个独立 HTTP 服务对外暴露核心能力，本文档说明如何从零开始完成集成。

## 三服务角色

| 服务 | 职责 | 典型使用场景 |
|------|------|-------------|
| **Hub** `:8100` | 节点注册、心跳保活、节点发现、联邦基因搜索 | 将你的节点注册到 Oris 网络；发现其他节点 |
| **Execution Runtime** `:8080` | 任务提交、状态轮询、取消、重放、恢复 | 提交异步任务并跟踪执行状态 |
| **Experience Repo** `:3000` | Gene/Capsule 共享与检索 | 向网络贡献进化成果；拉取其他节点的 Gene |

不需要全部接入三个服务。按需选择：
- 只想运行任务 → 仅接 **Execution Runtime**
- 只想贡献/消费 Gene → 仅接 **Experience Repo**（读操作无需认证）
- 参与完整 Oris 网络 → 三个服务均接

---

## 前置条件

### 1. 获取凭证

运行以下 curl 确认服务可访问，获取你的 API Key：

```bash
# 确认 Hub 在线
curl http://hub:8100/hub/nodes

# Execution Runtime 需要 Bearer Token（联系服务管理员获取）
# Experience Repo 需要 X-Api-Key（向 /keys 端点申请）
curl -X POST http://exp:3000/keys -H "X-Api-Key: <admin-key>" \
  -H "Content-Type: application/json" \
  -d '{"label": "my-agent"}'
```

### 2. 生成 Ed25519 Seed

Hub 写操作（注册、心跳）和 Experience Repo 写操作（share）均需 Ed25519 签名。你需要一个 **32 字节随机 seed**：

```bash
# 生成 seed（保存好，泄露 = 节点身份被冒充）
python3 -c "import secrets; print(secrets.token_hex(32))"
# 输出示例: 9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60
```

将 hex seed 转换为各语言的字节类型：
- **Go**: `[32]byte` — `hex.DecodeString(seedHex)` → copy to array
- **Python**: `bytes` — `bytes.fromhex(seedHex)`
- **TypeScript**: `Uint8Array` — `Buffer.from(seedHex, 'hex')`
- **Java**: `byte[]` — `HexFormat.of().parseHex(seedHex)`

---

## 安装

```bash
# Go
go get github.com/Colin4k1024/Oris/sdks/go

# Python
pip install oris-rt-sdk

# TypeScript
npm install @colin4k1024/oris-sdk

# Java (Maven)
# 在 pom.xml 中添加:
# <dependency>
#   <groupId>io.oris</groupId>
#   <artifactId>oris-sdk</artifactId>
#   <version>0.1.0</version>
# </dependency>
```

---

## 认证模型详解

三服务使用三种不同的认证机制，是最常见的集成障碍：

### Hub：写用签名，读用 Bearer

```
写操作（注册/心跳/注销）
  → Header: X-OEN-Signature: <base64(Ed25519(seed, body_bytes))>
  → 公钥编码：Base64

读操作（发现/搜索）
  → Header: Authorization: Bearer <api_key>
```

**为什么这样设计？** Hub 写操作需要证明你拥有节点私钥（防止他人注册你的 node_id），读操作只需授权即可。

### Execution Runtime：全部 Bearer

```
所有操作
  → Header: Authorization: Bearer <token>
  → 响应格式：ApiEnvelope<T> { meta, request_id, data }
```

SDK 自动解包 `ApiEnvelope`，你直接拿到 `data` 字段。

### Experience Repo：写用 ApiKey + 体内签名，读无认证

```
写操作（share）
  → Header: X-Api-Key: <api_key>
  → Body: { "envelope": { "payload": {...}, "signature": hex(Ed25519(seed, canonical_json(payload))), ... } }
  → 公钥编码：Hex（注意！与 Hub 的 Base64 不同）

读操作（fetch）
  → 无需认证，完全公开
```

**常见 bug**：Hub 用 Base64 公钥，Experience Repo 用 Hex 公钥，混用会导致签名验证失败（HTTP 403）。

---

## 完整工作流示例

### 场景：注册节点 → 提交任务 → 共享基因

#### Go

```go
package main

import (
    "encoding/hex"
    "fmt"
    "github.com/Colin4k1024/Oris/sdks/go/hub"
    "github.com/Colin4k1024/Oris/sdks/go/execution"
    "github.com/Colin4k1024/Oris/sdks/go/experience"
)

func main() {
    seedBytes, _ := hex.DecodeString("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
    var seed [32]byte
    copy(seed[:], seedBytes)

    // 1. 注册到 Hub
    h := hub.New(hub.Config{
        BaseURL: "http://hub:8100",
        APIKey:  "your-api-key",
        Seed:    seed,
        NodeID:  "my-node-001",
    })
    _, err := h.Register("http://my-node:9000", []string{"evolve", "intake"}, "0.1.0")
    if err != nil { panic(err) }
    fmt.Println("Registered on Hub")

    // 2. 提交任务到 Execution Runtime
    e := execution.New(execution.Config{
        BaseURL: "http://exec:8080",
        Token:   "your-bearer-token",
    })
    job, err := e.RunJob("thread-001", map[string]any{"task": "analyze", "input": "data"})
    if err != nil { panic(err) }
    fmt.Printf("Job status: %s\n", job.Status)

    // 3. 共享 Gene 到 Experience Repo
    exp := experience.New(experience.Config{
        BaseURL:  "http://exp:3000",
        APIKey:   "your-exp-api-key",
        Seed:     seed,
        SenderID: "my-node-001",
    })
    _, err = exp.Share(map[string]any{
        "gene_id":    "gene-001",
        "confidence": 0.92,
        "task_class": "bugfix",
    })
    if err != nil { panic(err) }
    fmt.Println("Gene shared")
}
```

#### Python

```python
import secrets
from oris_sdk import HubClient, HubConfig, ExecutionClient, ExecutionConfig
from oris_sdk import ExperienceClient, ExperienceConfig

seed = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")

# 1. 注册到 Hub
hub = HubClient(HubConfig(
    base_url="http://hub:8100",
    api_key="your-api-key",
    seed=seed,
    node_id="my-node-001",
))
hub.register(endpoint="http://my-node:9000", capabilities=["evolve"], version="0.1.0")

# 2. 提交任务
exe = ExecutionClient(ExecutionConfig(base_url="http://exec:8080", token="your-token"))
job = exe.run_job(thread_id="thread-001", input={"task": "analyze"})
print(f"Job status: {job.status}")

# 3. 共享 Gene
exp = ExperienceClient(ExperienceConfig(
    base_url="http://exp:3000",
    api_key="your-exp-key",
    seed=seed,
    sender_id="my-node-001",
))
exp.share({"gene_id": "gene-001", "confidence": 0.92, "task_class": "bugfix"})
```

#### TypeScript

```typescript
import { HubClient, ExecutionClient, ExperienceClient } from "@colin4k1024/oris-sdk";

const seed = Buffer.from("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60", "hex");

// 1. 注册到 Hub
const hub = new HubClient({ baseUrl: "http://hub:8100", apiKey: "your-key", seed, nodeId: "my-node-001" });
await hub.register("http://my-node:9000", ["evolve"], "0.1.0");

// 2. 提交任务
const exe = new ExecutionClient({ baseUrl: "http://exec:8080", token: "your-token" });
const job = await exe.runJob("thread-001", { task: "analyze" });
console.log("Job status:", job.status);

// 3. 共享 Gene
const exp = new ExperienceClient({ baseUrl: "http://exp:3000", apiKey: "your-exp-key", seed, senderId: "my-node-001" });
await exp.share({ gene_id: "gene-001", confidence: 0.92, task_class: "bugfix" });
```

#### Java

```java
import io.oris.sdk.hub.HubClient;
import io.oris.sdk.hub.HubConfig;
import io.oris.sdk.execution.ExecutionClient;
import io.oris.sdk.execution.ExecutionConfig;
import io.oris.sdk.experience.ExperienceClient;
import io.oris.sdk.experience.ExperienceConfig;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;

byte[] seed = HexFormat.of().parseHex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");

// 1. 注册到 Hub
HubClient hub = new HubClient(HubConfig.builder()
    .baseUrl("http://hub:8100").apiKey("your-key").seed(seed).nodeId("my-node-001").build());
hub.register("http://my-node:9000", List.of("evolve"), "0.1.0");

// 2. 提交任务
ExecutionClient exe = new ExecutionClient(ExecutionConfig.builder()
    .baseUrl("http://exec:8080").token("your-token").build());
var job = exe.runJob("thread-001", Map.of("task", "analyze"));
System.out.println("Job status: " + job.getStatus());

// 3. 共享 Gene
ExperienceClient exp = new ExperienceClient(ExperienceConfig.builder()
    .baseUrl("http://exp:3000").apiKey("your-exp-key").seed(seed).senderId("my-node-001").build());
exp.share(Map.of("gene_id", "gene-001", "confidence", 0.92, "task_class", "bugfix"));
```

---

## 错误处理

所有 SDK 使用统一的错误层级：

```
OrisError
├── AuthError
│   ├── InvalidKey       — 密钥格式不正确
│   ├── SignatureRejected — 服务端验签失败 (403)
│   └── TokenExpired     — Bearer token 过期 (401)
├── NetworkError
│   ├── Timeout
│   └── ConnectionRefused
├── ApiError             — 服务端 4xx/5xx
│   ├── NotFound         — 404
│   ├── Conflict         — 409 (幂等键冲突)
│   ├── RateLimited      — 429 (含 retry_after 秒数)
│   └── ServerError      — 5xx
├── SerializationError
└── ValidationError      — 客户端参数校验失败
```

### 常见错误对照表

| 错误 | 原因 | 解决方式 |
|------|------|---------|
| `SignatureRejected (403)` | 签名 base64/hex 混用；seed 不对应注册的公钥 | Hub 用 `PublicKeyBase64`，Experience 用 `PublicKeyHex` |
| `Conflict (409)` | `thread_id` 已存在且 `idempotency_key` 不同 | 使用相同 `idempotency_key` 重试，或换新 `thread_id` |
| `RateLimited (429)` | 超过发送者速率限制 | 读取 `retry_after` 字段，等待后重试 |
| `NotFound (404)` | Hub 中节点已过期（心跳超时被清除）| 重新调用 `register()`，恢复节点注册 |

---

## 版本矩阵

| SDK 版本 | Hub API | Execution API | Experience Repo API |
|---------|---------|---------------|---------------------|
| 0.1.x | v0.2.13 | v0.3.0 | v0.3.0 |

---

## 参考资源

- `sdks/spec/hub-openapi.yaml` — Hub 完整 API 规范
- `sdks/spec/execution-openapi.yaml` — Execution Runtime 完整 API 规范
- `sdks/spec/experience-repo-openapi.yaml` — Experience Repo 完整 API 规范
- `sdks/spec/signing-spec.md` — Ed25519 签名模型详解
- `sdks/spec/oen-envelope-spec.md` — OEN 信封结构规范
- `sdks/spec/golden/` — 跨语言签名验证测试向量
