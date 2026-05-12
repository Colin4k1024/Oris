# Closeout Summary — Oris Experience Repository Hub

| Field | Value |
|-------|-------|
| Artifact | closeout-summary |
| Role | tech-lead |
| Status | final |
| State | closed |
| Date | 2026-05-12 |
| Slug | experience-repo-hub |

---

## 1. Closeout Object

| Item | Detail |
|------|--------|
| Task | experience-repo-hub |
| Artifacts | prd.md, requirement-challenge.md, arch-design.md, delivery-plan.md, test-plan.md, launch-acceptance.md |
| Phases completed | intake → plan → execute → review → closeout |
| Closeout role | tech-lead |
| Observation window | N/A (dev-first delivery; production observation deferred to deploy) |

---

## 2. Result Assessment

### Goal Achievement

| Goal | Status | Evidence |
|------|--------|----------|
| Node registry (register/heartbeat/deregister/GC) | ACHIEVED | 5 registry tests + e2e tests |
| Capability/region discovery | ACHIEVED | 5 discovery tests |
| Federated search with timeout | ACHIEVED | 5 federation tests |
| Subscription CRUD + webhook dispatch | ACHIEVED | 7 subscription tests |
| Web dashboard (read-only) | ACHIEVED | 8 dashboard tests |
| Ed25519 cryptographic auth | ACHIEVED | Real ed25519-dalek verification in e2e + dashboard tests |
| API key token validation | ACHIEVED | TokenStore with e2e auth tests |
| SSRF prevention | ACHIEVED | 19 validation unit tests (IPv4 + IPv6 + ULA + link-local + mapped) |
| CORS restriction | ACHIEVED | Env-var-based CorsLayer (permissive override removed) |
| Rate limiting | ACHIEVED | Governor-based, e2e verified |

### Current Status: CLOSED

All planned deliverables complete. 57 tests pass. No outstanding CRITICAL/HIGH issues. Launch acceptance: GO.

---

## 3. Observation Window Conclusion

No production deployment has occurred yet. This closeout covers the development delivery and security hardening cycle. Post-deploy observation points are documented in `launch-acceptance.md` section 5.

---

## 4. Residual Risk Disposition

| Risk | Severity | Disposition | Owner | Next Action |
|------|----------|-------------|-------|-------------|
| Timing attack on token `==` comparison | LOW | **Accepted** | — | Monitor; upgrade to constant-time if threat model changes |
| Unauthenticated dashboard | MEDIUM | **Accepted (design decision)** | tech-lead | Add auth layer before exposing to untrusted networks |
| XSS in dashboard templates | MEDIUM | **Deferred** | backend-engineer | Add HTML escaping before external user exposure |
| Missing negative tests (concurrent key rotation) | MEDIUM | **Deferred** | qa-engineer | Add in next testing sprint |
| Single SQLite connection (Mutex) | LOW | **Accepted** | — | Adequate for single-instance; revisit if scaling |
| Silent error dropping in stores | LOW | **Deferred** | backend-engineer | Add tracing before high-traffic deployment |

---

## 5. Backlog Writeback

| Priority | Item | Trigger | Suggested Phase |
|----------|------|---------|-----------------|
| P2 | Add HTML escaping to dashboard templates | Before external exposure | Next iteration |
| P2 | Add explicit negative tests for key rotation / concurrent upsert | Next testing sprint | v0.5.0 |
| P2 | Add tracing for silent error paths in stores | Before high-traffic deployment | v0.5.0 |
| P3 | Timing-safe token comparison | If threat model changes | As-needed |
| P3 | Dashboard authentication layer | If exposed to untrusted networks | As-needed |

---

## 6. Knowledge Consolidation

### Lessons Learned

#### 1. Tower middleware layer ordering determines CORS behavior
**Scenario**: `CorsLayer::permissive()` applied as outer layer in server.rs silently overrode the env-var-restricted CORS set in `build_router()`.
**Root cause**: Tower middleware stack processes outermost layer first; permissive CORS at the outermost position wins regardless of inner configuration.
**Lesson**: Never apply `CorsLayer::permissive()` in production server setup. CORS must be configured exactly once, at the correct layer position. Review middleware ordering as part of security review.

#### 2. IPv4-only SSRF checks are insufficient when IPv6 is accepted
**Scenario**: `is_private_ip` only blocked `::1` for IPv6, allowing `::ffff:10.0.0.1` (IPv4-mapped), `fc00::1` (ULA), and `fe80::1` (link-local) to bypass all private IP checks.
**Root cause**: Incremental implementation focused on IPv4 first without completing IPv6 coverage.
**Lesson**: SSRF validation must cover IPv6 ULA (fc00::/7), link-local (fe80::/10), and IPv4-mapped (::ffff:0:0/96) in addition to loopback. Use a comprehensive check from day one.

#### 3. `ON CONFLICT DO UPDATE` enables key substitution attacks
**Scenario**: `INSERT ... ON CONFLICT(node_id) DO UPDATE SET public_key = excluded.public_key` allowed any caller to overwrite an existing node's key by re-registering with the same node_id.
**Root cause**: SQLite upsert pattern chosen for convenience (idempotent registration) without considering the security implication of updating identity-critical fields.
**Lesson**: Identity fields (public keys, credentials) must never be in the `DO UPDATE SET` clause. Use a pre-check + conditional insert/reject pattern for registration endpoints.

---

## 7. Task Closure

| Field | Value |
|-------|-------|
| Final status | **CLOSED** |
| All blockers resolved | Yes (B1–B5 + CR-1, H2, C1) |
| Backlog synced | Yes (5 items written back) |
| Lessons synced | Yes (3 lessons documented) |
| Next owner | devops-engineer (for production deployment) |
| Re-open trigger | Production deployment failure or post-deploy observation finding |
