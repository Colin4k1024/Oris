from __future__ import annotations

from datetime import datetime, timezone

from oris_sdk.evolution import OrisEvolutionAdapter, SolidifyInput, ValidationResult, detect_signal
from oris_sdk.gene import Gene
from oris_sdk.store import LocalStore


def test_signal_fingerprint_is_stable() -> None:
    a = detect_signal("/tmp/a/main.py:12: NameError: Foo", task_class="python")
    b = detect_signal("/Users/me/app/main.py:99: NameError: Foo", task_class="python")
    assert a.fingerprint == b.fingerprint


def test_adapter_select_replay_solidify(tmp_path) -> None:
    store = LocalStore(str(tmp_path / "genes.db"))
    try:
        signal = detect_signal("NameError: Foo", task_class="python")
        now = datetime.now(timezone.utc)
        store.save(
            Gene(
                gene_id="gene-1",
                name="Fix missing name",
                task_class="python",
                confidence=0.92,
                strategy={"steps": ["define the missing name", "rerun tests"]},
                signals={"fingerprint": signal.fingerprint, "error_type": signal.error_type},
                source="local",
                created_at=now,
                updated_at=now,
            )
        )

        adapter = OrisEvolutionAdapter(store)
        candidates = adapter.select(signal)
        assert len(candidates) == 1
        decision = adapter.replay(candidates[0])
        assert decision.mode == "suggest"
        assert decision.instructions == ["define the missing name", "rerun tests"]

        gene = adapter.solidify(
            SolidifyInput(
                signal=adapter.detect("NameError: Bar", task_class="python"),
                solution="define Bar",
                validation=ValidationResult(passed=True, evidence="pytest passed"),
                steps=["define Bar", "pytest"],
            )
        )
        assert gene.gene_id
        assert adapter.status().genes_count == 2
    finally:
        store.close()

