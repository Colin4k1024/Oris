# Oris SDKs

Multi-language client SDKs for the Oris Runtime services. **v0.3.0** — now with MySQL storage backend.

## Services

|Service|Auth Model|
|---|---|
|**Hub**|Writes: `X-OEN-Signature` required, `X-OEN-Timestamp` recommended. Legacy mode signs raw body only. Preferred mode signs `timestamp + "\n" + method + "\n" + path + "\n" + body`. Reads: `Authorization: Bearer`|
|**Execution Runtime**|All endpoints: `Authorization: Bearer`. Responses wrapped in `ApiEnvelope<T>`|
|**Experience Repo**|Writes: `X-Api-Key` + OEN signature inside body envelope. Reads: no auth|

## Languages

|Language|Path|Package|Install|
|---|---|---|---|
|Go|`sdks/go/`|`github.com/Colin4k1024/Oris/sdks/go`|`go get github.com/Colin4k1024/Oris/sdks/go@v0.3.0`|
|Python|`sdks/python/`|[`oris-rt-sdk`](https://pypi.org/project/oris-rt-sdk/)|`pip install oris-rt-sdk==0.3.0`|
|TypeScript|`sdks/typescript/`|[`@colin4k1024/oris-sdk`](https://www.npmjs.com/package/@colin4k1024/oris-sdk)|`npm install @colin4k1024/oris-sdk@0.3.0`|

## Storage Backends

|Backend|Use Case|Dependency|
|---|---|---|
|**SQLite** (default)|Single-node, local-first, zero config|Built-in|
|**MySQL** (v0.3.0+)|Multi-node teams, shared server|Go: `go-sql-driver/mysql`, Python: `pip install oris-rt-sdk[mysql]`, TS: `npm install mysql2`|

## Quick Start

### Go

```go
import (
    "github.com/Colin4k1024/Oris/sdks/go/hub"
    "github.com/Colin4k1024/Oris/sdks/go/execution"
    "github.com/Colin4k1024/Oris/sdks/go/experience"
)

// Hub — register a node
seed := [32]byte{...} // Ed25519 seed
h := hub.New(hub.Config{BaseURL: "http://hub:8080", APIKey: "key", Seed: seed, NodeID: "n1"})
resp, _ := h.Register("http://my-node:9000", []string{"evolve"}, "0.1.0")

// Execution — run a job
e := execution.New(execution.Config{BaseURL: "http://exec:8080", Token: "tok"})
job, _ := e.RunJob("thread-1", map[string]any{"task": "hello"})

// Experience — share a gene
exp := experience.New(experience.Config{BaseURL: "http://exp:8080", APIKey: "ak", Seed: seed, SenderID: "agent-1"})
gene, _ := exp.Share(map[string]any{"gene_id": "g1", "confidence": 0.9})
```

### Python

```python
from oris_sdk import HubClient, HubConfig, ExecutionClient, ExecutionConfig, ExperienceClient, ExperienceConfig

# Hub
hub = HubClient(HubConfig(base_url="http://hub:8080", api_key="key", seed=b"32-byte-seed...", node_id="n1"))
hub.register(endpoint="http://my-node:9000", capabilities=["evolve"], version="0.1.0")

# Execution
exe = ExecutionClient(ExecutionConfig(base_url="http://exec:8080", token="tok"))
job = exe.run_job(thread_id="thread-1", input={"task": "hello"})

# Experience
exp = ExperienceClient(ExperienceConfig(base_url="http://exp:8080", api_key="ak", seed=b"32-byte-seed...", sender_id="agent-1"))
gene = exp.share({"gene_id": "g1", "confidence": 0.9})
```

### TypeScript

```typescript
import { HubClient, ExecutionClient, ExperienceClient } from "@colin4k1024/oris-sdk";

// Hub
const seed = new Uint8Array(32); // Ed25519 seed
const hub = new HubClient({ baseUrl: "http://hub:8080", apiKey: "key", seed, nodeId: "n1" });
const reg = await hub.register("http://my-node:9000", ["evolve"], "0.1.0");

// Execution
const exe = new ExecutionClient({ baseUrl: "http://exec:8080", token: "tok" });
const job = await exe.runJob("thread-1", { task: "hello" });

// Experience
const exp = new ExperienceClient({ baseUrl: "http://exp:8080", apiKey: "ak", seed, senderId: "agent-1" });
const gene = await exp.share({ gene_id: "g1", confidence: 0.9 });
```

## MySQL Storage (v0.3.0+)

Use MySQL as a shared gene store for team/server deployments. The API is identical to SQLite — just change the config.

### Go (MySQL)

```go
import oris "github.com/Colin4k1024/Oris/sdks/go"

client, _ := oris.NewClient(oris.Config{
    MySQL: &oris.MySQLSync{
        Host:     "127.0.0.1",
        Port:     3306,
        User:     "oris",
        Password: "secret",
        Database: "oris",
    },
})
defer client.Close()

// Same Store API as SQLite
client.Store.Save(ctx, gene)
results, _ := client.Store.Query(ctx, store.StoreQuery{TaskClass: "error-handling"})
```

### Python (MySQL)

```python
from oris_sdk import OrisClient, OrisConfig
from oris_sdk.mysql_store import MySQLConfig

client = OrisClient(OrisConfig(
    mysql=MySQLConfig(host="127.0.0.1", port=3306, user="oris", password="secret", database="oris")
))

# Same store API as SQLite
client.store.save(gene)
results = client.store.query(task_class="error-handling")
```

### TypeScript (MySQL)

```typescript
import { OrisClient } from "@colin4k1024/oris-sdk";

// Use async factory for MySQL
const client = await OrisClient.create({
  mysql: { host: "127.0.0.1", port: 3306, user: "oris", password: "secret", database: "oris" },
});

// Same store API as SQLite
await client.store.save(gene);
const results = await client.store.query({ taskClass: "error-handling" });
client.close();
```

## Signing

All SDKs use Ed25519 with a 32-byte raw seed:

- **Hub writes, legacy compatibility**: sign the raw JSON body and send it as `X-OEN-Signature` (base64)
- **Hub writes, preferred mode**: set `X-OEN-Timestamp` (unix seconds), sign `timestamp + "\n" + HTTP_METHOD + "\n" + REQUEST_PATH + "\n" + raw JSON body`, and send the signature as `X-OEN-Signature` (base64)
- **Hub replay protection**: when `X-OEN-Timestamp` is present, the hub enforces a configurable age window and rejects timestamps that are too far in the future
- **Experience writes**: sign only the `payload` field, embed signature inside `envelope.signature` (base64)
- **Public key for Hub**: base64 encoding
- **Public key for Experience**: hex encoding

## Testing

```bash
# Go
cd sdks/go && go test ./...

# Python
cd sdks/python && PYTHONPATH=src pytest tests/ -v

# TypeScript
cd sdks/typescript && npm test
```

## Specs

Reference specifications live in `sdks/spec/`:

- `signing-spec.md` — Ed25519 signing models
- `oen-envelope-spec.md` — OEN envelope structure
- `hub-openapi.yaml` — Hub API spec
- `execution-openapi.yaml` — Execution Runtime API spec
- `experience-repo-openapi.yaml` — Experience Repo API spec
- `golden/` — Golden test fixtures for cross-language validation
