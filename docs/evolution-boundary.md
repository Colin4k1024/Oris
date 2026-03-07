# Evolution Runtime Boundary (March 2026)

This document defines the current production/stable boundary versus experimental boundary for
execution-server evolution paths.

## Stable boundary

Enable `a2a-production` when you need production compatibility A2A workflows.

Stable routes:

- `POST /a2a/hello` (returns `node_secret`, `claim_code`, `claim_url`, `hub_node_id`)
- `POST /a2a/fetch`
- `POST /a2a/tasks/distribute`
- `POST /a2a/tasks/claim`
- `POST /a2a/tasks/report`
- `POST /a2a/task/claim`
- `POST /a2a/task/complete`
- `POST /a2a/work/claim`
- `POST /a2a/work/complete`
- `POST /a2a/heartbeat` (returns `next_heartbeat_ms`)

**EvoMap compatibility aliases (stable):**

- `POST /task/claim` -> maps to `/a2a/task/claim`
- `POST /task/complete` -> maps to `/a2a/task/complete`

Runtime behavior in this mode:

- Compatibility queue metrics remain available at `/metrics`.
- Session handshake guidance points to `/a2a/hello`.
- Evolution publish/fetch/revoke endpoints are not exposed.

## Experimental boundary

Experimental routes remain behind `evolution-network-experimental`
(or `full-evolution-experimental`):

- `POST /v1/evolution/publish`
- `POST /a2a/publish` (EvoMap compatibility alias for `/v1/evolution/publish`)
- `POST /v1/evolution/fetch`
- `POST /v1/evolution/revoke`
- `POST /v1/evolution/a2a/handshake`
- `POST /evolution/a2a/*`
- `POST/GET /v1/evolution/a2a/sessions/*`
- `GET /v1/evolution/a2a/tasks/:task_id/lifecycle`

These routes are intentionally kept out of the stable production subset.

## EvoMap Compatibility Response Fields

| Endpoint | Field | Description |
|----------|-------|-------------|
| `/a2a/hello` | `node_secret` | Opaque node authentication token (64 hex chars) |
| `/a2a/hello` | `claim_code` | One-time 6-char alphanumeric code for node claiming |
| `/a2a/hello` | `claim_url` | URL for claiming (default: `/a2a/task/claim`) |
| `/a2a/hello` | `hub_node_id` | Hub identifier |
| `/a2a/heartbeat` | `next_heartbeat_ms` | Next heartbeat interval (default: 900000 = 15 min) |

## Migration notes

If you previously enabled `full-evolution-experimental` only for compatibility `/a2a` traffic:

1. Switch to `a2a-production` for production compatibility traffic.
2. Keep `full-evolution-experimental` only where publish/fetch/revoke and evolution session
   orchestration are explicitly required.
3. Update runbooks and monitors to treat `/a2a/*` as the stable entrypoints.
