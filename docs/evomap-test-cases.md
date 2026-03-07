# EvoMap 测试用例文档

本文档定义了EvoMap核心功能的测试用例，涵盖RuntimeRepository、API Handlers和集成测试。

## 测试环境配置

```bash
# 启用测试特性
cargo test -p oris-execution-runtime --features "sqlite-persistence"

# 或使用完整特性
cargo test -p oris-runtime --all-features
```

## 1. Bounty 生命周期测试

### 1.1 创建 Bounty

```rust
#[test]
fn test_bounty_create() {
    // 创建 bounty
    let bounty = BountyRecord {
        bounty_id: "bounty-001".to_string(),
        title: "Fix memory leak in worker pool".to_string(),
        description: Some("Memory leak occurs when...".to_string()),
        reward: 1000,
        status: BountyStatus::Open,
        created_by: "user-001".to_string(),
        created_at_ms: Utc::now().timestamp_millis(),
        closed_at_ms: None,
        accepted_by: None,
        accepted_at_ms: None,
    };

    // 验证创建成功
    assert!(repository.upsert_bounty(&bounty).is_ok());

    // 验证可以查询
    let fetched = repository.get_bounty("bounty-001").unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().title, "Fix memory leak in worker pool");
}
```

### 1.2 接受 Bounty

```rust
#[test]
fn test_bounty_accept() {
    // 创建 bounty
    let bounty = create_test_bounty("bounty-002");
    repository.upsert_bounty(&bounty).unwrap();

    // 接受 bounty
    let result = repository.accept_bounty("bounty-002", "worker-001");

    assert!(result.is_ok());

    // 验证状态转换
    let fetched = repository.get_bounty("bounty-002").unwrap().unwrap();
    assert_eq!(fetched.status, BountyStatus::Accepted);
    assert_eq!(fetched.accepted_by, Some("worker-001".to_string()));
    assert!(fetched.accepted_at_ms.is_some());
}
```

### 1.3 关闭 Bounty

```rust
#[test]
fn test_bounty_close() {
    // 创建并接受 bounty
    let bounty = create_test_bounty("bounty-003");
    repository.upsert_bounty(&bounty).unwrap();
    repository.accept_bounty("bounty-003", "worker-001").unwrap();

    // 关闭 bounty
    let result = repository.close_bounty("bounty-003");

    assert!(result.is_ok());

    // 验证状态
    let fetched = repository.get_bounty("bounty-003").unwrap().unwrap();
    assert_eq!(fetched.status, BountyStatus::Closed);
    assert!(fetched.closed_at_ms.is_some());
}
```

### 1.4 列出 Bounties

```rust
#[test]
fn test_bounty_list() {
    // 创建多个 bounties
    for i in 0..5 {
        let bounty = create_test_bounty(&format!("bounty-{}", i));
        repository.upsert_bounty(&bounty).unwrap();
    }

    // 列出所有 open bounties
    let open_bounties = repository.list_bounties(Some("open"), 10).unwrap();
    assert_eq!(open_bounties.len(), 5);

    // 限制数量
    let limited = repository.list_bounties(Some("open"), 2).unwrap();
    assert_eq!(limited.len(), 2);
}
```

## 2. Worker 注册测试

### 2.1 注册 Worker

```rust
#[test]
fn test_worker_register() {
    let worker = WorkerRecord {
        worker_id: "worker-001".to_string(),
        domains: "execution,analysis".to_string(),
        max_load: 10,
        metadata_json: Some(r#"{"cpu": 4, "memory": "16GB"}"#.to_string()),
        registered_at_ms: Utc::now().timestamp_millis(),
        last_heartbeat_ms: None,
        status: "active".to_string(),
    };

    assert!(repository.register_worker(&worker).is_ok());

    let fetched = repository.get_worker("worker-001").unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().domains, "execution,analysis");
}
```

### 2.2 Worker 心跳

```rust
#[test]
fn test_worker_heartbeat() {
    // 注册 worker
    let worker = create_test_worker("worker-002");
    repository.register_worker(&worker).unwrap();

    // 发送心跳
    let now = Utc::now().timestamp_millis();
    let result = repository.heartbeat_worker("worker-002", now);

    assert!(result.is_ok());

    // 验证心跳时间更新
    let fetched = repository.get_worker("worker-002").unwrap().unwrap();
    assert_eq!(fetched.last_heartbeat_ms, Some(now));
}
```

### 2.3 列出 Workers

```rust
#[test]
fn test_worker_list() {
    // 注册多个 workers
    for i in 0..3 {
        let worker = create_test_worker(&format!("worker-list-{}", i));
        repository.register_worker(&worker).unwrap();
    }

    // 按 domain 过滤
    let execution_workers = repository.list_workers(Some("execution"), None, 10).unwrap();
    assert!(!execution_workers.is_empty());

    // 按状态过滤
    let active_workers = repository.list_workers(None, Some("active"), 10).unwrap();
    assert!(!active_workers.is_empty());
}
```

## 3. Recipe 管理测试

### 3.1 创建 Recipe

```rust
#[test]
fn test_recipe_create() {
    let recipe = RecipeRecord {
        recipe_id: "recipe-001".to_string(),
        name: "HTTP Request Handler".to_string(),
        description: Some("Standard HTTP handling gene".to_string()),
        gene_sequence_json: r#"[{"type": "http_request", "timeout": 30000}]"#.to_string(),
        author_id: "author-001".to_string(),
        forked_from: None,
        created_at_ms: Utc::now().timestamp_millis(),
        updated_at_ms: Utc::now().timestamp_millis(),
        is_public: true,
    };

    assert!(repository.create_recipe(&recipe).is_ok());

    let fetched = repository.get_recipe("recipe-001").unwrap();
    assert!(fetched.is_some());
}
```

### 3.2 Fork Recipe

```rust
#[test]
fn test_recipe_fork() {
    // 创建原始 recipe
    let original = create_test_recipe("recipe-002");
    repository.create_recipe(&original).unwrap();

    // Fork
    let forked = repository.fork_recipe("recipe-002", "recipe-002-fork", "author-002");

    assert!(forked.is_ok());
    assert!(forked.unwrap().is_some());

    // 验证 forked 记录
    let fetched = repository.get_recipe("recipe-002-fork").unwrap().unwrap();
    assert_eq!(fetched.author_id, "author-002");
    assert_eq!(fetched.forked_from, Some("recipe-002".to_string()));
}
```

### 3.3 列出 Recipes

```rust
#[test]
fn test_recipe_list() {
    // 创建多个 recipes
    for i in 0..5 {
        let recipe = create_test_recipe(&format!("recipe-list-{}", i));
        repository.create_recipe(&recipe).unwrap();
    }

    // 列出所有
    let all = repository.list_recipes(None, 10).unwrap();
    assert_eq!(all.len(), 5);

    // 按作者过滤
    let author_recipes = repository.list_recipes(Some("author-001"), 10).unwrap();
    // ...
}
```

## 4. Organism 测试

### 4.1 表达 Organism

```rust
#[test]
fn test_organism_express() {
    // 先创建 recipe
    let recipe = create_test_recipe("recipe-org-001");
    repository.create_recipe(&recipe).unwrap();

    // 表达 organism
    let organism = OrganismRecord {
        organism_id: "organism-001".to_string(),
        recipe_id: "recipe-org-001".to_string(),
        status: "running".to_string(),
        current_step: 0,
        total_steps: 5,
        created_at_ms: Utc::now().timestamp_millis(),
        completed_at_ms: None,
    };

    assert!(repository.express_organism(&organism).is_ok());

    let fetched = repository.get_organism("organism-001").unwrap();
    assert!(fetched.is_some());
}
```

### 4.2 更新 Organism 状态

```rust
#[test]
fn test_organism_update() {
    // 创建 organism
    let organism = create_test_organism("organism-002");
    repository.express_organism(&organism).unwrap();

    // 更新步骤
    let result = repository.update_organism("organism-002", 2, "running");
    assert!(result.is_ok());

    // 验证更新
    let fetched = repository.get_organism("organism-002").unwrap().unwrap();
    assert_eq!(fetched.current_step, 2);
}
```

## 5. Session 协作测试

### 5.1 创建 Session

```rust
#[test]
fn test_session_create() {
    let session = SessionRecord {
        session_id: "session-001".to_string(),
        session_type: "collaboration".to_string(),
        creator_id: "user-001".to_string(),
        status: "active".to_string(),
        created_at_ms: Utc::now().timestamp_millis(),
        ended_at_ms: None,
    };

    assert!(repository.create_session(&session).is_ok());

    let fetched = repository.get_session("session-001").unwrap();
    assert!(fetched.is_some());
}
```

### 5.2 添加消息

```rust
#[test]
fn test_session_message() {
    // 创建 session
    let session = create_test_session("session-002");
    repository.create_session(&session).unwrap();

    // 添加消息
    let message = SessionMessageRecord {
        message_id: "msg-001".to_string(),
        session_id: "session-002".to_string(),
        sender_id: "user-001".to_string(),
        content: "Let's discuss the proposal".to_string(),
        message_type: "text".to_string(),
        sent_at_ms: Utc::now().timestamp_millis(),
    };

    assert!(repository.add_session_message(&message).is_ok());

    // 获取历史
    let history = repository.get_session_history("session-002", 10).unwrap();
    assert_eq!(history.len(), 1);
}
```

## 6. Dispute 争议测试

### 6.1 开启 Dispute

```rust
#[test]
fn test_dispute_open() {
    // 创建 bounty
    let bounty = create_test_bounty("bounty-dispute-001");
    repository.upsert_bounty(&bounty).unwrap();

    // 开启争议
    let dispute = DisputeRecord {
        dispute_id: "dispute-001".to_string(),
        bounty_id: "bounty-dispute-001".to_string(),
        opened_by: "user-001".to_string(),
        status: DisputeStatus::Open,
        evidence_json: Some(r#"{"reason": "unfair reward"}"#.to_string()),
        resolution: None,
        resolved_by: None,
        resolved_at_ms: None,
        created_at_ms: Utc::now().timestamp_millis(),
    };

    assert!(repository.open_dispute(&dispute).is_ok());

    let fetched = repository.get_dispute("dispute-001").unwrap();
    assert!(fetched.is_some());
}
```

### 6.2 解决 Dispute

```rust
#[test]
fn test_dispute_resolve() {
    // 创建并开启 dispute
    let dispute = create_test_dispute("dispute-002");
    repository.open_dispute(&dispute).unwrap();

    // 解决 dispute
    let result = repository.resolve_dispute(
        "dispute-002",
        "Reward increased to 1500",
        "moderator-001"
    );

    assert!(result.is_ok());

    // 验证解决
    let fetched = repository.get_dispute("dispute-002").unwrap().unwrap();
    assert_eq!(fetched.status, DisputeStatus::Resolved);
    assert!(fetched.resolved_at_ms.is_some());
}
```

### 6.3 查询 Bounty 的 Disputes

```rust
#[test]
fn test_dispute_list_for_bounty() {
    // 创建 bounty
    let bounty = create_test_bounty("bounty-dispute-list");
    repository.upsert_bounty(&bounty).unwrap();

    // 创建多个 disputes
    for i in 0..3 {
        let dispute = create_test_dispute(&format!("dispute-list-{}", i));
        repository.open_dispute(&dispute).unwrap();
    }

    // 查询
    let disputes = repository.get_disputes_for_bounty("bounty-dispute-list").unwrap();
    assert_eq!(disputes.len(), 3);
}
```

## 7. Swarm 任务分解测试

### 7.1 创建 Swarm 分解

```rust
#[test]
fn test_swarm_decomposition() {
    let task = SwarmTaskRecord {
        parent_task_id: "task-001".to_string(),
        decomposition_json: r#"[{"subtask_id": "st-1", "assignee": "worker-1"}, {"subtask_id": "st-2", "assignee": "worker-2"}]"#.to_string(),
        proposer_id: "proposer-001".to_string(),
        proposer_reward_pct: 10,
        solver_reward_pct: 80,
        aggregator_reward_pct: 10,
        status: "pending".to_string(),
        created_at_ms: Utc::now().timestamp_millis(),
        completed_at_ms: None,
    };

    assert!(repository.upsert_swarm_decomposition(&task).is_ok());

    let fetched = repository.get_swarm_decomposition("task-001").unwrap();
    assert!(fetched.is_some());
}
```

## 8. API Handler 集成测试

### 8.1 Bounty API 端到端

```rust
#[tokio::test]
async fn test_bounty_api_e2e() {
    let app = test_app().await;

    // POST /evomap/bounties - 创建 bounty
    let response = app.post("/evomap/bounties")
        .json(&CreateBountyRequest {
            title: "Test bounty".to_string(),
            description: Some("Description".to_string()),
            reward: 1000,
        })
        .send()
        .await;

    assert_eq!(response.status(), 201);
    let bounty_id = response.json::<BountyResponse>().await.id;

    // GET /evomap/bounties/:id - 获取 bounty
    let response = app.get(&format!("/evomap/bounties/{}", bounty_id))
        .send()
        .await;

    assert_eq!(response.status(), 200);

    // POST /evomap/bounties/:id/accept - 接受 bounty
    let response = app.post(&format!("/evomap/bounties/{}/accept", bounty_id))
        .json(&AcceptBountyRequest { worker_id: "worker-001".to_string() })
        .send()
        .await;

    assert_eq!(response.status(), 200);
}
```

### 8.2 Worker API 端到端

```rust
#[tokio::test]
async fn test_worker_api_e2e() {
    let app = test_app().await;

    // POST /evomap/workers - 注册 worker
    let response = app.post("/evomap/workers")
        .json(&RegisterWorkerRequest {
            worker_id: "worker-test-001".to_string(),
            domains: vec!["execution".to_string()],
            max_load: 5,
            metadata: HashMap::new(),
        })
        .send()
        .await;

    assert_eq!(response.status(), 201);

    // GET /evomap/workers/:id - 获取 worker
    let response = app.get("/evomap/workers/worker-test-001")
        .send()
        .await;

    assert_eq!(response.status(), 200);

    // PUT /evomap/workers/:id/heartbeat - 发送心跳
    let response = app.put("/evomap/workers/worker-test-001/heartbeat")
        .send()
        .await;

    assert_eq!(response.status(), 200);
}
```

## 9. SQLite 与 PostgreSQL 兼容性测试

### 9.1 跨数据库一致性

```rust
#[test]
fn test_sqlite_postgres_consistency() {
    // 在 SQLite 中测试
    let sqlite_repo = create_sqlite_repository();
    test_bounty_lifecycle(&sqlite_repo);

    // 在 PostgreSQL 中测试
    let pg_repo = create_postgres_repository();
    test_bounty_lifecycle(&pg_repo);
}

fn test_bounty_lifecycle<R: RuntimeRepository>(repo: &R) {
    let bounty = create_test_bounty("consistency-001");
    repo.upsert_bounty(&bounty).unwrap();
    repo.accept_bounty("consistency-001", "worker-001").unwrap();
    repo.close_bounty("consistency-001").unwrap();

    let fetched = repo.get_bounty("consistency-001").unwrap().unwrap();
    assert_eq!(fetched.status, BountyStatus::Closed);
}
```

## 10. 辅助函数

```rust
fn create_test_bounty(id: &str) -> BountyRecord {
    BountyRecord {
        bounty_id: id.to_string(),
        title: format!("Test Bounty {}", id),
        description: Some("Test description".to_string()),
        reward: 1000,
        status: BountyStatus::Open,
        created_by: "test-user".to_string(),
        created_at_ms: Utc::now().timestamp_millis(),
        closed_at_ms: None,
        accepted_by: None,
        accepted_at_ms: None,
    }
}

fn create_test_worker(id: &str) -> WorkerRecord {
    WorkerRecord {
        worker_id: id.to_string(),
        domains: "execution".to_string(),
        max_load: 10,
        metadata_json: None,
        registered_at_ms: Utc::now().timestamp_millis(),
        last_heartbeat_ms: None,
        status: "active".to_string(),
    }
}

// ... 其他辅助函数
```

## 运行测试

```bash
# 运行所有 EvoMap 测试
cargo test -p oris-execution-runtime evomap

# 运行特定模块测试
cargo test -p oris-execution-runtime test_bounty
cargo test -p oris-execution-runtime test_worker

# 运行 SQLite 后端测试
cargo test -p oris-execution-runtime --features "sqlite-persistence"

# 运行 PostgreSQL 后端测试
cargo test -p oris-execution-runtime --features "postgres"

# 运行完整测试套件
cargo test --release --all-features
```
