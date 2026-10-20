"""Release safeguards: no test performs a network request, tag push or package publication."""

import hashlib
import importlib.util
import contextlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.output = contextlib.redirect_stdout(io.StringIO())
        self.output.__enter__()
        self.addCleanup(self.output.__exit__, None, None, None)

    def test_remote_actions_are_previews_without_execute(self):
        with patch.object(release, "run") as run:
            release.tag(False)
            release.publish_npm(False)
            release.smoke(False)
            run.assert_not_called()

    def test_stable_version_rejects_prereleases_and_command_fragments(self):
        for value in ("v1.2.3", "01.2.3", "1.2.3-beta.1", "1.2.3; echo nope", "1.2"):
            with self.assertRaises(ValueError):
                release.stable_version(value)

    def test_version_update_synchronizes_all_channels_without_writing(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ("Cargo.toml", "Cargo.lock"):
                shutil.copyfile(release.ROOT / name, root / name)
            for directory in ("npm", "plugins"):
                shutil.copytree(release.ROOT / directory, root / directory)
            with patch.object(release, "ROOT", root):
                old = release.version()
                changes = release.version_updates("99.0.0")
                self.assertEqual(release.version(), old)
                manifest = tomllib.loads(changes["Cargo.toml"])
                self.assertEqual(manifest["workspace"]["package"]["version"], "99.0.0")
                for name in release.CRATES[:-1]:
                    self.assertEqual(manifest["workspace"]["dependencies"][name]["version"], "99.0.0")
                lock = tomllib.loads(changes["Cargo.lock"])
                for package in lock["package"]:
                    if package["name"] in release.CRATES:
                        self.assertEqual(package["version"], "99.0.0")
                self.assertIn("[profile.dist]", changes["Cargo.toml"])
                for rel, text in changes.items():
                    if rel.endswith("package.json") or rel.endswith("plugin.json"):
                        self.assertEqual(json.loads(text)["version"], "99.0.0")
                    if rel.endswith(".mcp.json"):
                        server = json.loads(text)["mcpServers"]["guildsmith"]
                        self.assertEqual(server["args"], ["-y", "guildsmith@99.0.0", "mcp"])
                        before = json.loads((root / rel).read_text())["mcpServers"]["guildsmith"]
                        self.assertEqual(server.get("env"), before.get("env"))
                        self.assertEqual(server.get("env_vars"), before.get("env_vars"))
                launcher = json.loads(changes["npm/guildsmith/package.json"])
                self.assertEqual(set(launcher["optionalDependencies"].values()), {"99.0.0"})
                with self.assertRaises(ValueError):
                    release.version_updates(old)

    def test_checksum_failure_stops_staging(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / "binary.tar.xz"
            checksum = Path(temp) / "binary.tar.xz.sha256"
            archive.write_bytes(b"release bytes")
            checksum.write_text(hashlib.sha256(b"release bytes").hexdigest() + "  binary.tar.xz\n")
            release.verify_archive(archive, checksum)
            archive.write_bytes(b"changed bytes")
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                release.verify_archive(archive, checksum)

    def test_npm_network_and_auth_failures_do_not_mean_missing(self):
        for code in ("E401", "E403", "EAI_AGAIN"):
            response = subprocess.CompletedProcess([], 1, json.dumps({"error": {"code": code}}), "")
            with patch.object(release.subprocess, "run", return_value=response):
                with self.assertRaises(ValueError):
                    release.npm_exists("guildsmith")
        response = subprocess.CompletedProcess([], 1, '{"error":{"code":"E404"}}', "")
        with patch.object(release.subprocess, "run", return_value=response):
            self.assertFalse(release.npm_exists("guildsmith"))

    def test_tag_refuses_dirty_tree(self):
        with patch.object(release, "run", return_value=" M Cargo.toml") as run:
            with self.assertRaisesRegex(ValueError, "working tree"):
                release.tag(True)
            self.assertEqual(run.call_count, 1)

    def test_tag_refuses_existing_tag_and_missing_successful_ci(self):
        for existing in (True, False):
            def fake_run(*args, **kwargs):
                if args[:2] == ("git", "status"):
                    return ""
                if args[:2] == ("git", "rev-parse"):
                    return "abc123"
                if args[:2] == ("git", "ls-remote"):
                    return "abc123\trefs/heads/main" if args[-1] == "refs/heads/main" else ""
                if args[:3] == ("git", "tag", "--list"):
                    return "v0.1.0" if existing else ""
                if args[:3] == ("gh", "run", "list"):
                    return "[]"
                self.fail(f"Unexpected mutation: {args}")
            with patch.object(release, "run", side_effect=fake_run), patch.object(release, "check_versions"):
                with self.assertRaisesRegex(ValueError, "already exists" if existing else "successful push CI"):
                    release.tag(True)

    def test_manual_publish_keeps_platform_order_and_skips_existing_versions(self):
        packages = [(name, Path(f"/prepared/{name}.tgz")) for name in release.PACKAGES]
        with (
            patch.object(release, "stage_npm", return_value=packages),
            patch.object(release, "npm_exists", side_effect=[True, False, False, False, False, False]),
            patch.object(release, "run") as run,
        ):
            release.publish_npm(True)
            publications = [call.args for call in run.call_args_list if call.args[:2] == ("npm", "publish")]
            self.assertEqual([args[2] for args in publications], [path for _, path in packages[1:]])
            self.assertNotIn("--provenance", publications[0])


if __name__ == "__main__":
    unittest.main()
