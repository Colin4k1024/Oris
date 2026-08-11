import json
from pathlib import Path

from oris_sdk.experience_v1 import ExperienceBundleV1


def test_golden_fixture_round_trip_is_lossless():
    fixture = Path(__file__).parents[3] / "spec/experience/golden/experience-bundle-v1.json"
    source = json.loads(fixture.read_text())
    bundle = ExperienceBundleV1.from_dict(source)
    bundle.validate()
    assert bundle.to_dict() == source
