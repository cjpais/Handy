import json
import subprocess
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

import release_manifest as rm


class VersionTests(unittest.TestCase):
    def test_grows_with_the_run_number(self):
        versions = [rm.make_version(n) for n in (1, 2, 9, 10, 100)]
        self.assertEqual(versions[0], "0.10.1")
        parsed = [tuple(map(int, v.split("."))) for v in versions]
        self.assertEqual(parsed, sorted(parsed))

    def test_is_newer_than_the_fork_builds(self):
        self.assertGreater(tuple(map(int, rm.make_version(1).split("."))), (0, 9, 8))

    def test_rejects_zero(self):
        with self.assertRaises(ValueError):
            rm.make_version(0)


class ChangelogTests(unittest.TestCase):
    def test_bullets_and_blank_lines(self):
        self.assertEqual(rm.format_changelog(["feat: a", "", "  fix: b "]), "- feat: a\n- fix: b")

    def test_empty(self):
        self.assertEqual(rm.format_changelog([]), "No user-visible changes.")

    def test_long_lists_are_cut(self):
        text = rm.format_changelog([f"c{i}" for i in range(rm.MAX_NOTE_LINES + 5)])
        self.assertEqual(len(text.splitlines()), rm.MAX_NOTE_LINES + 1)
        self.assertTrue(text.endswith("...and 5 more"))

    def test_subjects_since_a_tag(self):
        with tempfile.TemporaryDirectory() as d:
            def git(*a):
                subprocess.run(["git", "-C", d, *a], check=True, capture_output=True)
            git("init", "-q")
            git("config", "user.email", "t@example.com")
            git("config", "user.name", "t")
            for i, msg in enumerate(["one", "two"]):
                Path(d, f"f{i}").write_text(msg)
                git("add", "-A")
                git("commit", "-q", "-m", msg)
            git("tag", "v0.10.1")
            Path(d, "f2").write_text("x")
            git("add", "-A")
            git("commit", "-q", "-m", "feat: three")
            self.assertEqual(rm.commit_subjects("v0.10.1", d), ["feat: three"])
            self.assertEqual(rm.commit_subjects(None, d), ["feat: three", "two", "one"])


class ManifestTests(unittest.TestCase):
    def test_shape_matches_the_tauri_updater(self):
        now = datetime(2026, 10, 18, 9, 30, 5, tzinfo=timezone.utc)
        m = rm.build_manifest(
            "0.10.7", "- feat: x", "c2lnbmF0dXJl\n", "https://example.com/Voxa.app.tar.gz", now
        )
        self.assertEqual(m["version"], "0.10.7")
        self.assertEqual(m["pub_date"], "2026-10-18T09:30:05Z")
        self.assertEqual(set(m["platforms"]), {"darwin-aarch64", "darwin-aarch64-app"})
        for entry in m["platforms"].values():
            self.assertEqual(entry["signature"], "c2lnbmF0dXJl")
            self.assertTrue(entry["url"].endswith("Voxa.app.tar.gz"))
        json.dumps(m)  # serializable

    def test_cli_writes_a_valid_file(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "notes.md").write_text("- привет\n", encoding="utf-8")
            (d / "a.sig").write_text("sig==\n")
            rm.main([
                "manifest", "--version", "0.10.2", "--notes", str(d / "notes.md"),
                "--signature", str(d / "a.sig"), "--url", "https://x/y.tar.gz",
                "--out", str(d / "latest.json"),
            ])
            data = json.loads((d / "latest.json").read_text(encoding="utf-8"))
            self.assertEqual(data["notes"], "- привет")
            self.assertEqual(data["platforms"]["darwin-aarch64"]["signature"], "sig==")


if __name__ == "__main__":
    unittest.main()
