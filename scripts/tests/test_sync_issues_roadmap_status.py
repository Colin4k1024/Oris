import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock


SCRIPT_PATH = pathlib.Path(__file__).resolve().parents[1] / "sync_issues_roadmap_status.py"


def load_sync_module():
    spec = importlib.util.spec_from_file_location("sync_issues_roadmap_status", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec is not None and spec.loader is not None
    spec.loader.exec_module(module)
    return module


class SyncIssuesRoadmapStatusTests(unittest.TestCase):
    def test_sync_rows_updates_issue_state_and_roadmap_status(self):
        module = load_sync_module()
        rows = [
            {
                "title": "[EVO]",
                "issue_number": "86",
                "issue_state": "OPEN",
                "issue_url": "https://github.com/Colin4k1024/Oris/issues/86",
                "roadmap_status": "active",
            }
        ]
        issue_map = {
            "86": {
                "state": "CLOSED",
                "url": "https://github.com/Colin4k1024/Oris/issues/86",
            }
        }

        changed, missing, ambiguous_titles = module.sync_rows(rows, issue_map)

        self.assertEqual(changed, 1)
        self.assertEqual(missing, [])
        self.assertEqual(ambiguous_titles, [])
        self.assertEqual(rows[0]["issue_state"], "CLOSED")
        self.assertEqual(rows[0]["roadmap_status"], "archived")

    def test_sync_rows_keeps_open_issue_active(self):
        module = load_sync_module()
        rows = [
            {
                "title": "[EVO]",
                "issue_number": "87",
                "issue_state": "CLOSED",
                "issue_url": "https://github.com/Colin4k1024/Oris/issues/87",
                "roadmap_status": "archived",
            }
        ]
        issue_map = {
            "87": {
                "state": "OPEN",
                "url": "https://github.com/Colin4k1024/Oris/issues/87",
            }
        }

        changed, missing, ambiguous_titles = module.sync_rows(rows, issue_map)

        self.assertEqual(changed, 1)
        self.assertEqual(missing, [])
        self.assertEqual(ambiguous_titles, [])
        self.assertEqual(rows[0]["issue_state"], "OPEN")
        self.assertEqual(rows[0]["roadmap_status"], "active")

    def test_sync_rows_reports_missing_issue_number(self):
        module = load_sync_module()
        rows = [
            {
                "title": "[EVO]",
                "issue_number": "999",
                "issue_state": "OPEN",
                "issue_url": "https://github.com/Colin4k1024/Oris/issues/999",
                "roadmap_status": "active",
            }
        ]

        changed, missing, ambiguous_titles = module.sync_rows(rows, {})

        self.assertEqual(changed, 0)
        self.assertEqual(missing, ["999"])
        self.assertEqual(ambiguous_titles, [])
        self.assertEqual(rows[0]["issue_state"], "OPEN")
        self.assertEqual(rows[0]["roadmap_status"], "active")

    def test_sync_rows_backfills_issue_number_by_title(self):
        module = load_sync_module()
        rows = [
            {
                "title": "[EVMAP-01]",
                "roadmap_track": "evomap-alignment",
                "issue_number": "",
                "issue_state": "PLANNED",
                "issue_url": "",
                "roadmap_status": "planned",
            }
        ]
        issue_map = {
            "110": {
                "state": "OPEN",
                "url": "https://github.com/Colin4k1024/Oris/issues/110",
            }
        }
        issues_by_title = {
            "[EVMAP-01]": [
                {
                    "number": "110",
                    "state": "OPEN",
                    "title": "[EVMAP-01]",
                    "url": "https://github.com/Colin4k1024/Oris/issues/110",
                }
            ]
        }

        changed, missing, ambiguous_titles = module.sync_rows(
            rows,
            issue_map,
            issues_by_title=issues_by_title,
            only_track="evomap-alignment",
            backfill_by_title=True,
        )

        self.assertEqual(changed, 1)
        self.assertEqual(missing, [])
        self.assertEqual(ambiguous_titles, [])
        self.assertEqual(rows[0]["issue_number"], "110")
        self.assertEqual(rows[0]["issue_state"], "OPEN")
        self.assertEqual(rows[0]["roadmap_status"], "active")

    def test_sync_rows_skips_ambiguous_backfill_title(self):
        module = load_sync_module()
        rows = [
            {
                "title": "[EVMAP-02]",
                "roadmap_track": "evomap-alignment",
                "issue_number": "",
                "issue_state": "PLANNED",
                "issue_url": "",
                "roadmap_status": "planned",
            }
        ]
        issue_map = {}
        issues_by_title = {
            "[EVMAP-02]": [
                {"number": "111", "state": "OPEN", "title": "[EVMAP-02]", "url": "u1"},
                {"number": "211", "state": "OPEN", "title": "[EVMAP-02]", "url": "u2"},
            ]
        }

        changed, missing, ambiguous_titles = module.sync_rows(
            rows,
            issue_map,
            issues_by_title=issues_by_title,
            only_track="evomap-alignment",
            backfill_by_title=True,
        )

        self.assertEqual(changed, 0)
        self.assertEqual(missing, [])
        self.assertEqual(ambiguous_titles, ["[EVMAP-02]"])
        self.assertEqual(rows[0]["issue_number"], "")
        self.assertEqual(rows[0]["issue_state"], "PLANNED")

    def test_sync_rows_respects_track_filter(self):
        module = load_sync_module()
        rows = [
            {
                "title": "[EVMAP-03]",
                "roadmap_track": "evomap-alignment",
                "issue_number": "112",
                "issue_state": "PLANNED",
                "issue_url": "",
                "roadmap_status": "planned",
            },
            {
                "title": "[OTHER-01]",
                "roadmap_track": "runtime-v1",
                "issue_number": "21",
                "issue_state": "OPEN",
                "issue_url": "https://example.test/21",
                "roadmap_status": "active",
            },
        ]
        issue_map = {
            "112": {"state": "OPEN", "url": "https://github.com/Colin4k1024/Oris/issues/112"},
            "21": {"state": "CLOSED", "url": "https://example.test/21"},
        }

        changed, missing, ambiguous_titles = module.sync_rows(
            rows,
            issue_map,
            only_track="evomap-alignment",
        )

        self.assertEqual(changed, 1)
        self.assertEqual(missing, [])
        self.assertEqual(ambiguous_titles, [])
        self.assertEqual(rows[0]["issue_state"], "OPEN")
        self.assertEqual(rows[1]["issue_state"], "OPEN")

    def test_main_dry_run_does_not_write_csv(self):
        module = load_sync_module()
        with tempfile.TemporaryDirectory() as tmp_dir:
            csv_path = pathlib.Path(tmp_dir) / "issues.csv"
            csv_path.write_text(
                '"title","body","labels","milestone","roadmap_track","roadmap_status","issue_number","issue_state","issue_url"\n'
                '"[EVMAP-01]","","","","evomap-alignment","planned","","PLANNED",""\n',
                encoding="utf-8",
            )
            before = csv_path.read_text(encoding="utf-8")

            with mock.patch.object(module, "resolve_repo", return_value="Colin4k1024/Oris"), mock.patch.object(
                module,
                "fetch_issues",
                return_value=[
                    {
                        "number": "110",
                        "state": "OPEN",
                        "title": "[EVMAP-01]",
                        "url": "https://github.com/Colin4k1024/Oris/issues/110",
                    }
                ],
            ):
                code = module.main(
                    [
                        "--csv",
                        str(csv_path),
                        "--repo",
                        "Colin4k1024/Oris",
                        "--track",
                        "evomap-alignment",
                        "--dry-run",
                    ]
                )

            after = csv_path.read_text(encoding="utf-8")
            self.assertEqual(code, 0)
            self.assertEqual(before, after)


if __name__ == "__main__":
    unittest.main()
