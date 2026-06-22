# Eino Evolution Agent Quickstart

This example shows the Oris evolution core and Eino ToolsNode middleware:

1. Seed a local Oris gene store.
2. Detect a tool failure.
3. Select a matching reusable gene.
4. Replay the gene as safe instructions.
5. Solidify a successful retry as a new gene.

It does not require a real LLM or remote Oris service. The Eino dependency is used only for the `compose.ToolInput` / `compose.ToolOutput` middleware boundary.

```bash
cd sdks/go/examples/eino_evolution_agent
go run .
```

Use `einoadapter.ToolMiddleware(adapter, cfg)` in an Eino ToolsNode configuration, or wrap an invokable tool endpoint directly with `einoadapter.InvokableToolMiddleware`.

## Adding Self-Deposition to an Eino Agent

In a real Eino agent, attach the Oris middleware to the ToolsNode:

```go
adapter := evolution.NewAdapter(store, evolution.ReplayPolicy{
    MinConfidence: 0.7,
    Mode:          evolution.ReplayModeSuggest,
})

toolsNode, _ := compose.NewToolNode(ctx, &compose.ToolsNodeConfig{
    Tools: []tool.BaseTool{weatherTool},
    ToolCallMiddlewares: []compose.ToolMiddleware{
        einoadapter.ToolMiddleware(adapter, einoadapter.Config{
            TaskClass: "eino-tool",
        }),
    },
})
```

The middleware gives the agent experience reuse:

1. A tool call fails.
2. `einoadapter` turns the error into an Oris `EvolutionSignal`.
3. Oris selects a matching Gene from the local store.
4. The middleware returns replay instructions as the tool result, so the agent can retry with a better action.

After the retry succeeds, call `adapter.Solidify(...)` with validation evidence such as "tool retry succeeded" or "tests passed". That call is the self-deposition step: the agent writes the new successful pattern back into the Oris store as reusable experience.
