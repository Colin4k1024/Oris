from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path

from oris_sdk import LocalStore, OrisEvolutionAdapter, SolidifyInput, ValidationResult, detect_signal
from oris_sdk.gene import Gene
from oris_sdk.langchain import replay_message


def main() -> None:
    db_path = Path("langchain_evolution_demo.db")
    store = LocalStore(str(db_path))
    try:
        signal = detect_signal("tool failed: missing city parameter", task_class="langchain-tool")
        now = datetime.now(timezone.utc)
        store.save(
            Gene(
                gene_id="gene-langchain-missing-city",
                name="Recover missing city parameter",
                task_class="langchain-tool",
                confidence=0.93,
                strategy={
                    "rationale": "The weather tool fails when the city field is absent.",
                    "steps": [
                        "inspect the tool arguments",
                        "ask the model to provide a city value",
                        "retry the tool call with the completed argument",
                    ],
                },
                signals={"fingerprint": signal.fingerprint, "error_type": signal.error_type},
                source="local",
                created_at=now,
                updated_at=now,
            )
        )

        adapter = OrisEvolutionAdapter(store)
        detected = adapter.detect(
            "tool failed: missing city parameter",
            task_class="langchain-tool",
            context={"framework": "langchain", "tool": "get_weather"},
        )
        candidates = adapter.select(detected)
        if not candidates:
            print("no reusable Oris experience found")
            return

        decision = adapter.replay(candidates[0])
        print(replay_message(decision.instructions))

        adapter.solidify(
            SolidifyInput(
                signal=detected,
                solution="retry get_weather after filling the city argument",
                validation=ValidationResult(passed=True, evidence="demo retry succeeded"),
                steps=["fill missing city argument", "retry get_weather"],
            )
        )
        status = adapter.status()
        print(f"genes: {status.genes_count} avg_confidence: {status.avg_confidence:.2f}")
    finally:
        store.close()


if __name__ == "__main__":
    main()

