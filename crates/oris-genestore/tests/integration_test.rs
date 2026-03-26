//! Integration tests for oris-genestore

use chrono::Utc;
use uuid::Uuid;

use oris_genestore::{Capsule, Gene, GeneQuery, GeneStore, SqliteGeneStore};

fn make_gene_with_all_fields(id: Uuid) -> Gene {
    Gene {
        id,
        name: "test-gene".into(),
        description: "A fully-populated test gene".into(),
        tags: vec!["rust".into(), "compiler".into(), "memory".into()],
        template: "fix: {description}".into(),
        preconditions: vec!["tests pass".into(), "clippy clean".into()],
        validation_steps: vec!["cargo test".into(), "cargo fmt".into()],
        confidence: 0.85,
        use_count: 42,
        success_count: 38,
        quality_score: 0.92,
        created_at: Utc::now(),
        last_used_at: Some(Utc::now()),
        last_boosted_at: Some(Utc::now()),
    }
}

fn make_capsule_with_all_fields(id: Uuid, gene_id: Uuid) -> Capsule {
    Capsule {
        id,
        gene_id,
        content: "fn fix() { todo!() }".into(),
        env_fingerprint: "linux-x86_64-6.1".into(),
        quality_score: 0.88,
        confidence: 0.90,
        use_count: 10,
        success_count: 9,
        last_replay_run_id: Some(Uuid::new_v4()),
        created_at: Utc::now(),
        last_used_at: Some(Utc::now()),
    }
}

#[tokio::test]
async fn gene_full_field_roundtrip() {
    let store = SqliteGeneStore::open(":memory:").unwrap();
    let gene = make_gene_with_all_fields(Uuid::new_v4());
    let id = gene.id;

    store.upsert_gene(&gene).await.unwrap();

    let fetched = store
        .get_gene(id)
        .await
        .unwrap()
        .expect("gene should exist after upsert");

    assert_eq!(fetched.id, gene.id);
    assert_eq!(fetched.name, gene.name);
    assert_eq!(fetched.description, gene.description);
    assert_eq!(fetched.tags, gene.tags);
    assert_eq!(fetched.template, gene.template);
    assert_eq!(fetched.preconditions, gene.preconditions);
    assert_eq!(fetched.validation_steps, gene.validation_steps);
    assert!((fetched.confidence - gene.confidence).abs() < 1e-6);
    assert_eq!(fetched.use_count, gene.use_count);
    assert_eq!(fetched.success_count, gene.success_count);
    assert!((fetched.quality_score - gene.quality_score).abs() < 1e-6);
    assert_eq!(fetched.created_at, gene.created_at);
    assert_eq!(fetched.last_used_at, gene.last_used_at);
    assert_eq!(fetched.last_boosted_at, gene.last_boosted_at);
}

#[tokio::test]
async fn capsule_full_field_roundtrip() {
    let store = SqliteGeneStore::open(":memory:").unwrap();

    let gene = Gene {
        id: Uuid::new_v4(),
        name: "parent-gene".into(),
        description: "parent".into(),
        tags: vec![],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.80,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    store.upsert_gene(&gene).await.unwrap();

    let capsule = make_capsule_with_all_fields(Uuid::new_v4(), gene.id);
    let cid = capsule.id;

    store.upsert_capsule(&capsule).await.unwrap();

    let fetched = store
        .get_capsule(cid)
        .await
        .unwrap()
        .expect("capsule should exist after upsert");

    assert_eq!(fetched.id, capsule.id);
    assert_eq!(fetched.gene_id, capsule.gene_id);
    assert_eq!(fetched.content, capsule.content);
    assert_eq!(fetched.env_fingerprint, capsule.env_fingerprint);
    assert!((fetched.quality_score - capsule.quality_score).abs() < 1e-6);
    assert!((fetched.confidence - capsule.confidence).abs() < 1e-6);
    assert_eq!(fetched.use_count, capsule.use_count);
    assert_eq!(fetched.success_count, capsule.success_count);
    assert_eq!(fetched.last_replay_run_id, capsule.last_replay_run_id);
    assert_eq!(fetched.created_at, capsule.created_at);
    assert_eq!(fetched.last_used_at, capsule.last_used_at);
}

#[tokio::test]
async fn capsule_upsert_idempotent() {
    let store = SqliteGeneStore::open(":memory:").unwrap();

    let gene = Gene {
        id: Uuid::new_v4(),
        name: "parent".into(),
        description: "p".into(),
        tags: vec![],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.80,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    store.upsert_gene(&gene).await.unwrap();

    let capsule_id = Uuid::new_v4();
    let capsule = Capsule {
        id: capsule_id,
        gene_id: gene.id,
        content: "original".into(),
        env_fingerprint: "env1".into(),
        quality_score: 0.75,
        confidence: 0.80,
        use_count: 5,
        success_count: 4,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };

    store.upsert_capsule(&capsule).await.unwrap();

    let updated = Capsule {
        id: capsule_id,
        gene_id: gene.id,
        content: "modified".into(),
        env_fingerprint: "env2".into(),
        quality_score: 0.90,
        confidence: 0.95,
        use_count: 10,
        success_count: 9,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };
    store.upsert_capsule(&updated).await.unwrap();

    let capsules = store.capsules_for_gene(gene.id).await.unwrap();
    assert_eq!(
        capsules.len(),
        1,
        "should have exactly one capsule after upsert"
    );
    assert_eq!(capsules[0].id, capsule_id);
    assert_eq!(capsules[0].content, "modified");
    assert!((capsules[0].confidence - 0.95).abs() < 1e-6);
}

#[tokio::test]
async fn capsules_for_gene_ordered_by_confidence() {
    let store = SqliteGeneStore::open(":memory:").unwrap();

    let gene = Gene {
        id: Uuid::new_v4(),
        name: "parent".into(),
        description: "p".into(),
        tags: vec![],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.80,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    store.upsert_gene(&gene).await.unwrap();

    let c1 = Capsule {
        id: Uuid::new_v4(),
        gene_id: gene.id,
        content: "low".into(),
        env_fingerprint: "env".into(),
        quality_score: 0.5,
        confidence: 0.60,
        use_count: 0,
        success_count: 0,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };
    let c2 = Capsule {
        id: Uuid::new_v4(),
        gene_id: gene.id,
        content: "high".into(),
        env_fingerprint: "env".into(),
        quality_score: 0.9,
        confidence: 0.95,
        use_count: 0,
        success_count: 0,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };
    let c3 = Capsule {
        id: Uuid::new_v4(),
        gene_id: gene.id,
        content: "mid".into(),
        env_fingerprint: "env".into(),
        quality_score: 0.7,
        confidence: 0.75,
        use_count: 0,
        success_count: 0,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };

    store.upsert_capsule(&c1).await.unwrap();
    store.upsert_capsule(&c3).await.unwrap();
    store.upsert_capsule(&c2).await.unwrap();

    let capsules = store.capsules_for_gene(gene.id).await.unwrap();
    assert_eq!(capsules.len(), 3);
    assert!((capsules[0].confidence - 0.95).abs() < 1e-6);
    assert!((capsules[1].confidence - 0.75).abs() < 1e-6);
    assert!((capsules[2].confidence - 0.60).abs() < 1e-6);
}

#[tokio::test]
async fn delete_gene_cascades_capsules() {
    let store = SqliteGeneStore::open(":memory:").unwrap();

    let gene = Gene {
        id: Uuid::new_v4(),
        name: "parent".into(),
        description: "p".into(),
        tags: vec![],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.80,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    store.upsert_gene(&gene).await.unwrap();

    let cap1 = Capsule {
        id: Uuid::new_v4(),
        gene_id: gene.id,
        content: "cap1".into(),
        env_fingerprint: "env".into(),
        quality_score: 0.8,
        confidence: 0.85,
        use_count: 0,
        success_count: 0,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };
    let cap2 = Capsule {
        id: Uuid::new_v4(),
        gene_id: gene.id,
        content: "cap2".into(),
        env_fingerprint: "env".into(),
        quality_score: 0.7,
        confidence: 0.75,
        use_count: 0,
        success_count: 0,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };
    store.upsert_capsule(&cap1).await.unwrap();
    store.upsert_capsule(&cap2).await.unwrap();

    assert!(store.get_capsule(cap1.id).await.unwrap().is_some());
    assert!(store.get_capsule(cap2.id).await.unwrap().is_some());

    store.delete_gene(gene.id).await.unwrap();

    assert!(store.get_gene(gene.id).await.unwrap().is_none());
    assert!(store.get_capsule(cap1.id).await.unwrap().is_none());
    assert!(store.get_capsule(cap2.id).await.unwrap().is_none());
}

#[tokio::test]
async fn search_genes_multi_tag_intersection() {
    let store = SqliteGeneStore::open(":memory:").unwrap();

    let gene_rust_memory = Gene {
        id: Uuid::new_v4(),
        name: "rust-memory".into(),
        description: "rust memory issue".into(),
        tags: vec!["rust".into(), "memory".into(), "compiler".into()],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.80,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    let gene_rust_async = Gene {
        id: Uuid::new_v4(),
        name: "rust-async".into(),
        description: "rust async issue".into(),
        tags: vec!["rust".into(), "async".into()],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.75,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    let gene_python = Gene {
        id: Uuid::new_v4(),
        name: "python-unsafe".into(),
        description: "python issue".into(),
        tags: vec!["python".into(), "memory".into()],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.70,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };

    store.upsert_gene(&gene_rust_memory).await.unwrap();
    store.upsert_gene(&gene_rust_async).await.unwrap();
    store.upsert_gene(&gene_python).await.unwrap();

    let query = GeneQuery {
        problem_description: "memory safety".into(),
        required_tags: vec!["rust".into(), "memory".into()],
        min_confidence: 0.0,
        limit: 10,
    };
    let results = store.search_genes(&query).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].gene.id, gene_rust_memory.id);

    let query_rust_only = GeneQuery {
        problem_description: "rust".into(),
        required_tags: vec!["rust".into()],
        min_confidence: 0.0,
        limit: 10,
    };
    let results2 = store.search_genes(&query_rust_only).await.unwrap();
    assert_eq!(results2.len(), 2);
}

#[tokio::test]
async fn decay_all_affects_capsules() {
    let store = SqliteGeneStore::open(":memory:").unwrap();

    let gene = Gene {
        id: Uuid::new_v4(),
        name: "parent".into(),
        description: "p".into(),
        tags: vec![],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.80,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    store.upsert_gene(&gene).await.unwrap();

    let capsule = Capsule {
        id: Uuid::new_v4(),
        gene_id: gene.id,
        content: "cap".into(),
        env_fingerprint: "env".into(),
        quality_score: 0.8,
        confidence: 0.90,
        use_count: 0,
        success_count: 0,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };
    store.upsert_capsule(&capsule).await.unwrap();

    store.decay_all().await.unwrap();

    let fetched = store
        .get_capsule(capsule.id)
        .await
        .unwrap()
        .expect("capsule should still exist after decay");
    assert!(
        fetched.confidence < capsule.confidence,
        "capsule confidence should decrease after decay"
    );
}

#[tokio::test]
async fn record_capsule_outcome_failure_decreases_confidence() {
    let store = SqliteGeneStore::open(":memory:").unwrap();

    let gene = Gene {
        id: Uuid::new_v4(),
        name: "parent".into(),
        description: "p".into(),
        tags: vec![],
        template: "".into(),
        preconditions: vec![],
        validation_steps: vec![],
        confidence: 0.80,
        use_count: 0,
        success_count: 0,
        quality_score: 0.0,
        created_at: Utc::now(),
        last_used_at: None,
        last_boosted_at: None,
    };
    store.upsert_gene(&gene).await.unwrap();

    let capsule = Capsule {
        id: Uuid::new_v4(),
        gene_id: gene.id,
        content: "cap".into(),
        env_fingerprint: "env".into(),
        quality_score: 0.8,
        confidence: 0.90,
        use_count: 5,
        success_count: 5,
        last_replay_run_id: None,
        created_at: Utc::now(),
        last_used_at: None,
    };
    store.upsert_capsule(&capsule).await.unwrap();

    store
        .record_capsule_outcome(capsule.id, false, None)
        .await
        .unwrap();

    let fetched = store
        .get_capsule(capsule.id)
        .await
        .unwrap()
        .expect("capsule should exist after recording outcome");

    assert_eq!(fetched.use_count, capsule.use_count + 1);
    assert_eq!(fetched.success_count, capsule.success_count);
    assert!(
        fetched.confidence < capsule.confidence,
        "confidence should decrease after failed outcome"
    );
}
