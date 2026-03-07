# v0.21.0 - EvoMap Feature Expansion

`oris-runtime` v0.21.0 expands EvoMap ecosystem with complete RuntimeRepository implementations, peer discovery, automation feedback loops, and comprehensive E2E test coverage.

## What's in this release

### RuntimeRepository Enhancements (#136, #137)
- Added Recipe and Organism CRUD operations to SQLite RuntimeRepository
- Implemented PostgreSQL RuntimeRepository with all CRUD methods:
  - Worker registration (upsert_worker_registration, get_worker_registration, count_active_claims_for_worker)
  - Dispute management (create_dispute, get_dispute, append_dispute_evidence, resolve_dispute, settle_bounty_via_dispute)
  - Recipe operations (create_recipe, get_recipe)
  - Organism operations (create_organism, get_organism, update_organism_status)
  - Session persistence (upsert_a2a_session, get_active_a2a_session)
- Updated API handlers (evomap_recipe_create, evomap_recipe_get, evomap_recipe_fork, evomap_organism_express, evomap_organism_get) to use RuntimeRepository

### Peer Discovery & Gossip (#138)
- Added `gossip` module to oris-evolution-network crate:
  - PeerRegistry for managing known peers with health monitoring
  - PeerConfig for static peer list configuration
  - GossipMessage and GossipKind for event propagation
  - GossipBuilder for creating gossip messages

### Automatic Publishing Gate (#139)
- Added `publish_gate` module to oris-orchestrator:
  - PublishGate for automatic publishing of promoted assets
  - PublishGateConfig for configurable publish targets and retry behavior
  - PublishTarget for HTTP/IPFS endpoints
  - Exponential backoff retry mechanism
  - Publish status tracking and history

### Evolver Automation (#140)
- Added `evolver` module to oris-evolution crate:
  - EvolutionSignal and SignalType for extracting signals from feedback
  - MutationProposal and MutationRiskLevel for mutation generation
  - EvolverConfig for configuration
  - EvolverAutomation engine for the feedback loop
  - SignalBuilder for creating signals
  - ValidationResult for proposal validation

### E2E Test Coverage (#141)
- Added comprehensive tests:
  - recipe_crud_lifecycle: create, get, list operations
  - organism_lifecycle: create, get, update status, complete operations
  - recipe_fork_lifecycle: forking recipes with forked_from
- Total SQLite tests: 44 (increased from 41)

## Validation

- cargo fmt --all -- --check
- cargo build --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
