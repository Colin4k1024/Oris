# operator_cli template

Use this template when you need a small operations CLI for Oris jobs.

## What you get

- `run`
- `list`
- `inspect`
- `resume`
- `replay`
- `cancel`

All commands call execution server APIs and print JSON responses.

## Suggested next edits

1. Add authentication headers/tokens for production control plane.
2. Add output formats (`--format json|table`) for operator workflows.
3. Wrap common incident actions into higher-level subcommands.
