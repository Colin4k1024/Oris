# LangChain Evolution Agent Quickstart

This example shows the Oris evolution core and the message shape used by the LangChain middleware:

1. Seed a local Oris gene store.
2. Detect a tool failure.
3. Select a matching reusable gene.
4. Replay the gene as safe instructions.
5. Solidify a successful retry as a new gene.

It does not require a real LLM, LangChain installation, or remote Oris service.

```bash
cd sdks/python
PYTHONPATH=src python examples/langchain_evolution_agent/main.py
```

Run the command from the Python environment where the SDK dependencies are installed.

In a real LangChain agent, install `oris-rt-sdk[langchain]` and pass the middleware returned by `create_oris_middleware(adapter)` into `create_agent(..., middleware=[...])`.

## Adding Self-Deposition to a LangChain Agent

Attach Oris middleware when creating the agent:

```python
from langchain.agents import create_agent
from oris_sdk import LocalStore, OrisEvolutionAdapter, SolidifyInput, ValidationResult
from oris_sdk.langchain import create_oris_middleware

store = LocalStore("oris_genes.db")
adapter = OrisEvolutionAdapter(store)

agent = create_agent(
    model="openai:gpt-5",
    tools=[weather_tool],
    middleware=[create_oris_middleware(adapter, task_class="langchain-tool")],
)
```

The middleware gives the agent experience reuse:

1. A tool call fails.
2. `create_oris_middleware` converts the exception into an Oris signal.
3. Oris selects a matching Gene from the local store.
4. The middleware returns a `ToolMessage` with replay instructions.
5. The agent can retry with the recovered strategy.

After the retry succeeds, persist the new experience:

```python
adapter.solidify(
    SolidifyInput(
        signal=detected_signal,
        solution="retry the weather tool after filling the missing city argument",
        validation=ValidationResult(passed=True, evidence="tool retry succeeded"),
        steps=["fill the missing city argument", "retry the tool"],
    )
)
```

That `solidify` call is the self-deposition step: the agent turns a solved failure into a reusable Gene for future runs.
