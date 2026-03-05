import importlib.util
import pathlib
import unittest


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

        changed, missing = module.sync_rows(rows, issue_map)

        self.assertEqual(changed, 1)
        self.assertEqual(missing, [])
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

        changed, missing = module.sync_rows(rows, issue_map)

        self.assertEqual(changed, 1)
        self.assertEqual(missing, [])
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

        changed, missing = module.sync_rows(rows, {})

        self.assertEqual(changed, 0)
        self.assertEqual(missing, ["999"])
        self.assertEqual(rows[0]["issue_state"], "OPEN")
        self.assertEqual(rows[0]["roadmap_status"], "active")


if __name__ == "__main__":
    unittest.main()
