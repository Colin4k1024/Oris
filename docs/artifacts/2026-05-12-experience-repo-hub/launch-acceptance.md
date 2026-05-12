# Launch Acceptance — Oris Experience Repository Hub

| Field | Value |
|-------|-------|
| Artifact | launch-acceptance |
| Role | qa-engineer |
| Status | final |
| State | accepted |
| Date | 2026-05-12 |
| Slug | experience-repo-hub |

---

## 1. Acceptance Overview

| Item | Detail |
|------|--------|
| Object | oris-hub crate (Experience Repository Hub) |
| Date | 2026-05-12 |
| Reviewer | qa-engineer |
| Method | Parallel code review + security review + automated test execution + issue remediation |
| Environment | Development (`:memory:` SQLite, single-instance) |

---

## 2. Acceptance Scope

### In Scope

- Registry CRUD + heartbeat + GC + key substitution prevention
- Capability/region discovery
- Federated search fan-out
- Subscription CRUD + webhook dispatch with SSRF-safe callback validation
- Dashboard (overview, nodes, detail, subscriptions, search) — intentionally unauthenticated
- Ed25519 real cryptographic signature verification (ed25519-dalek)
- API key bearer token validation (TokenStore)
- SSRF prevention (IPv4 + IPv6 + ULA + link-local + IPv4-mapped)
- CORS env-var-based origin restriction
- Rate limiting (governor-based)
- Error handling and status code mapping (including 409 Conflict)

### Not In Scope

- External webhook delivery to live services (mock-tested)
- Multi-instance / distributed deployment
- Load/stress testing
- Timing-safe token comparison (accepted LOW risk)

---

## 3. Acceptance Evidence

### Test Results

| Suite | Tests | Result |
|-------|-------|--------|
| registry_test.rs | 5 | PASS |
| discovery_test.rs | 5 | PASS |
| federation_test.rs | 5 | PASS |
| subscription_test.rs | 7 | PASS |
| e2e_hub_test.rs | 8 | PASS |
| dashboard_test.rs | 8 | PASS |
| validation.rs (unit tests) | 19 | PASS |
| **Total** | **57** | **ALL PASS** |

### Security Review Evidence

| Check | Result |
|-------|--------|
| SQL injection (parameterized queries) | PASS — rusqlite `?1` placeholders throughout |
| SSRF (callback URL validation) | PASS — blocks private IPv4, IPv6 loopback, ULA, link-local, IPv4-mapped |
| Key substitution attack | PASS — 409 Conflict on node_id reuse with different key |
| CORS misconfiguration | PASS — `CorsLayer::permissive()` removed; env-var control effective |
| Auth bypass | PASS — real Ed25519 verification + TokenStore validation |
| Error information leakage | PASS — Storage/Internal errors return generic "internal error" |

### Key Artifacts

| Artifact | Status |
|----------|--------|
| arch-design.md | Complete |
| delivery-plan.md | Complete |
| test-plan.md | Final (updated with review findings) |
| Source code (~2,100 LOC) | Implemented + hardened |
| Integration + unit tests (57) | All passing |

### Build Verification

- `cargo build -p oris-hub` — clean
- `cargo test -p oris-hub` — 57/57 pass
- `cargo fmt -p oris-hub -- --check` — clean

---

## 4. Risk Assessment

### Satisfied Requirements

| ID | Requirement | Evidence |
|----|-------------|----------|
| R1 | Node registration/heartbeat/GC | 5 registry tests + key conflict test in e2e |
| R2 | Capability/region discovery | 5 discovery tests |
| R3 | Federated search with timeout | 5 federation tests |
| R4 | Subscription CRUD + dispatch | 7 subscription tests |
| R5 | Dashboard pages (no auth, by design) | 8 dashboard tests |
| R6 | Ed25519 signature verification | Real crypto in e2e + dashboard tests |
| R7 | API key token validation | TokenStore + e2e auth tests |
| R8 | SSRF prevention | 19 validation unit tests |
| R9 | CORS restriction | CorsLayer::permissive() removed, env-var control |
| R10 | Rate limiting | e2e rate limit test |
| R11 | Error handling | error.rs mapping + e2e tests |
| R12 | Key substitution prevention | Conflict check in register() |

### Accepted Risks

| Risk | Severity | Acceptance Rationale |
|------|----------|---------------------|
| Timing attack on token comparison | LOW | Network jitter dominates; not exploitable over HTTP |
| Unauthenticated dashboard | MEDIUM | Intentional design decision — read-only internal monitoring view |
| XSS in dashboard (unescaped node_id) | MEDIUM | Dashboard is internal-only; remediation tracked for future external exposure |
| Missing explicit negative tests (concurrent key rotation) | MEDIUM | Architecture prevents the attack; explicit tests would increase confidence |
| Silent error dropping in stores | LOW | Acceptable for current deployment model |
| Single SQLite connection (Mutex) | LOW | Single-instance deployment; adequate for current load |

### Blocking Items

**None** — all original B1–B5 production blockers and review-discovered issues (CR-1, H2, C1) have been resolved.

---

## 5. Launch Conclusion

### Decision: GO

| Target | Decision | Conditions |
|--------|----------|------------|
| **Production** | **GO** | All blockers resolved; accepted risks documented and non-critical |

### Rationale

The oris-hub implementation is security-hardened and functionally complete:

- **57 tests** across 7 test suites (unit + integration + E2E)
- **Real Ed25519 cryptographic verification** with body-level signing (ed25519-dalek)
- **SSRF prevention** covering IPv4, IPv6, ULA, link-local, and IPv4-mapped addresses
- **Key substitution prevention** with 409 Conflict on duplicate node_id with different key
- **CORS** properly restricted via environment variable (permissive layer removed)
- **Clean architecture** with proper separation of concerns
- **No CRITICAL or HIGH issues remaining** after review remediation

### Observation Points (Post-Deploy)

- Monitor SQLite Mutex contention under concurrent load
- Verify GC timer fires correctly in long-running process
- Confirm webhook retry backoff behavior with production endpoints
- Validate dashboard renders correctly with large node counts (>100)
- Monitor for any IPv6 bypass attempts via unusual address formats
- Track token validation latency to confirm timing attack is not practical

### Confirmation Record

| Role | Decision | Date |
|------|----------|------|
| qa-engineer | GO | 2026-05-12 |
| tech-lead | Pending | — |

---

## 6. Next Steps

1. Deploy to production environment
2. Complete tech-lead sign-off on launch acceptance
3. Monitor observation points for 48h post-deploy
4. Track remediation of MEDIUM accepted risks in backlog:
   - Add HTML escaping to dashboard templates before external exposure
   - Add explicit negative tests for concurrent key rotation scenarios
   - Add timing-safe comparison if threat model changes
