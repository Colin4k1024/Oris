# axum_service template

Use this template when you want an app-local Oris runtime behind HTTP APIs.

## What you get

- Axum server with `/healthz`
- Oris execution API router
- SQLite-backed durable graph execution
- Tracing bootstrap

## Suggested next edits

1. Replace the demo graph node with business nodes/tools.
2. Add auth middleware in front of operator endpoints.
3. Wire logs and traces to your observability stack.
