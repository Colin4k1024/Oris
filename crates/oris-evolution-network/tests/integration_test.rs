//! Integration tests for the oris-evolution-network crate.

use oris_evolution::{AssetState, Capsule, EnvFingerprint, EvolutionEvent, Gene, Outcome};
use oris_evolution_network::{
    sign_envelope, verify_envelope, EvolutionEnvelope, NetworkAsset, NodeKeypair,
    PeerRateLimitConfig, PeerRateLimiter, RevokeNotice,
};

fn sample_gene(id: &str) -> Gene {
    Gene {
        id: id.to_string(),
        signals: vec!["sig.test".to_string()],
        strategy: vec!["fix it".to_string()],
        validation: vec!["cargo test".to_string()],
        state: AssetState::Candidate,
        task_class_id: None,
    }
}

fn sample_capsule(id: &str, gene_id: &str) -> Capsule {
    Capsule {
        id: id.to_string(),
        gene_id: gene_id.to_string(),
        mutation_id: "mut-1".to_string(),
        run_id: "run-1".to_string(),
        diff_hash: "abc123".to_string(),
        confidence: 0.7,
        env: EnvFingerprint {
            rustc_version: "1.78.0".to_string(),
            cargo_lock_hash: "lockhash".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            os: "linux".to_string(),
        },
        outcome: Outcome {
            success: true,
            validation_profile: "default".to_string(),
            validation_duration_ms: 100,
            changed_files: vec!["src/lib.rs".to_string()],
            validator_hash: "vhash".to_string(),
            lines_changed: 10,
            replay_verified: false,
        },
        state: AssetState::Candidate,
    }
}

fn sample_event() -> EvolutionEvent {
    EvolutionEvent::MutationApplied {
        mutation_id: "mut-1".to_string(),
        patch_hash: "patch123".to_string(),
        changed_files: vec!["src/lib.rs".to_string()],
    }
}

#[test]
fn envelope_mixed_asset_types() {
    let assets = vec![
        NetworkAsset::Gene {
            gene: sample_gene("gene-1"),
        },
        NetworkAsset::Capsule {
            capsule: sample_capsule("cap-1", "gene-1"),
        },
        NetworkAsset::EvolutionEvent {
            event: sample_event(),
        },
    ];
    let envelope = EvolutionEnvelope::publish("node-a", assets);

    assert!(envelope.verify_content_hash());
    assert!(envelope.verify_manifest().is_ok());

    let manifest = envelope.manifest.as_ref().unwrap();
    assert_eq!(manifest.asset_ids.len(), 3);
    assert_eq!(manifest.sender_id, "node-a");
}

#[test]
fn verify_signature_wrong_key_fails() {
    let tmp_a = std::env::temp_dir().join(format!("oris-test-key-a-{}", std::process::id()));
    let tmp_b = std::env::temp_dir().join(format!("oris-test-key-b-{}", std::process::id()));

    let keypair_a = NodeKeypair::generate_at(&tmp_a).unwrap();
    let keypair_b = NodeKeypair::generate_at(&tmp_b).unwrap();

    let assets = vec![NetworkAsset::Gene {
        gene: sample_gene("gene-1"),
    }];
    let envelope = EvolutionEnvelope::publish("node-a", assets);
    let signed = sign_envelope(&keypair_a, &envelope);

    // Verify with wrong key should fail
    let result = verify_envelope(&keypair_b.public_key_hex(), &signed);
    assert!(result.is_err());

    let _ = std::fs::remove_file(&tmp_a);
    let _ = std::fs::remove_file(&tmp_b);
}

#[test]
fn verify_signature_tampered_envelope_fails() {
    let tmp = std::env::temp_dir().join(format!("oris-test-key-tamper-{}", std::process::id()));
    let keypair = NodeKeypair::generate_at(&tmp).unwrap();

    let assets = vec![NetworkAsset::Gene {
        gene: sample_gene("gene-1"),
    }];
    let envelope = EvolutionEnvelope::publish("node-a", assets);
    let mut signed = sign_envelope(&keypair, &envelope);

    // Tamper with the sender_id
    signed.sender_id = "attacker".to_string();

    let result = verify_envelope(&keypair.public_key_hex(), &signed);
    assert!(result.is_err());

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn revoke_notice_serde_roundtrip() {
    let notice = RevokeNotice {
        sender_id: "node-a".to_string(),
        asset_ids: vec!["gene:gene-1".to_string(), "capsule:cap-1".to_string()],
        reason: "security vulnerability".to_string(),
    };
    let json = serde_json::to_string(&notice).unwrap();
    let restored: RevokeNotice = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.sender_id, notice.sender_id);
    assert_eq!(restored.asset_ids, notice.asset_ids);
    assert_eq!(restored.reason, notice.reason);
}

#[test]
fn gossip_builder_missing_kind_returns_none() {
    use oris_evolution_network::gossip::GossipBuilder;
    let result = GossipBuilder::new("peer-1".to_string(), 1).build();
    assert!(result.is_none());
}

#[test]
fn peer_registry_add_remove() {
    use oris_evolution_network::gossip::{PeerConfig, PeerEndpoint, PeerRegistry};

    let config = PeerConfig {
        peers: vec![],
        heartbeat_interval_secs: 30,
        peer_timeout_secs: 10,
        gossip_fanout: 3,
    };
    let registry = PeerRegistry::new(config, "local".to_string());

    registry.add_peer(PeerEndpoint {
        peer_id: "peer-1".to_string(),
        endpoint: "http://peer-1:8080".to_string(),
        public_key: None,
    });
    registry.add_peer(PeerEndpoint {
        peer_id: "peer-2".to_string(),
        endpoint: "http://peer-2:8080".to_string(),
        public_key: None,
    });

    let peers = registry.get_active_peers();
    assert_eq!(peers.len(), 2);

    registry.remove_peer("peer-1");
    let peers = registry.get_active_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].peer_id, "peer-2");
}

#[test]
fn rate_limiter_blocks_after_capacity() {
    let config = PeerRateLimitConfig {
        max_capsules_per_hour: 2,
        window_secs: 60,
    };
    let limiter = PeerRateLimiter::new(config);

    assert!(limiter.check("peer-1"));
    assert!(limiter.check("peer-1"));
    assert!(!limiter.check("peer-1")); // blocked

    // Different peer should still be allowed
    assert!(limiter.check("peer-2"));
}

#[test]
fn end_to_end_signed_publish_verify() {
    let tmp = std::env::temp_dir().join(format!("oris-test-key-e2e-{}", std::process::id()));
    let keypair = NodeKeypair::generate_at(&tmp).unwrap();
    let pub_key = keypair.public_key_hex();

    let assets = vec![
        NetworkAsset::Gene {
            gene: sample_gene("gene-e2e"),
        },
        NetworkAsset::Capsule {
            capsule: sample_capsule("cap-e2e", "gene-e2e"),
        },
    ];

    let envelope = EvolutionEnvelope::publish("node-e2e", assets);
    assert!(envelope.verify_content_hash());
    assert!(envelope.verify_manifest().is_ok());

    let signed = sign_envelope(&keypair, &envelope);
    assert!(signed.signature.is_some());
    assert!(verify_envelope(&pub_key, &signed).is_ok());
    assert!(signed.verify_content_hash());

    let _ = std::fs::remove_file(&tmp);
}
