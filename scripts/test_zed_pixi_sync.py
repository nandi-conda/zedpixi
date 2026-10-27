#!/usr/bin/env python3
"""Tests for zed_pixi_sync.py. Run: python3 -m unittest discover -s scripts -p 'test_*.py'"""

from __future__ import annotations

import contextlib
import io
import json
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).parent))
import zed_pixi_sync as sync  # noqa: E402


def make_executable(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


class TemporaryProject(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.prefix = self.root / "env"
        (self.prefix / "bin").mkdir(parents=True)
        self.manifest = self.root / "pixi.toml"
        self.manifest.write_text(
            '[workspace]\nname = "demo"\n\n[tasks.build]\ncmd = "true"\n',
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def info(self, name: str = "default", prefix: Path | None = None) -> dict:
        return {
            "environments_info": [
                {"name": name, "prefix": str(prefix or self.prefix)}
            ]
        }


class SelectEnvironmentTests(TemporaryProject):
    def test_default_environment(self) -> None:
        info = {
            "environments_info": [
                {"name": "test", "prefix": str(self.prefix)},
                {"name": "default", "prefix": str(self.prefix)},
            ]
        }
        self.assertEqual(
            sync.select_environment(info, None), ("default", str(self.prefix))
        )

    def test_only_environment(self) -> None:
        self.assertEqual(
            sync.select_environment(self.info("dev"), None), ("dev", str(self.prefix))
        )

    def test_requested_environment(self) -> None:
        info = {
            "environments_info": [
                {"name": "default", "prefix": str(self.prefix)},
                {"name": "docs", "prefix": str(self.prefix)},
            ]
        }
        self.assertEqual(
            sync.select_environment(info, "docs"), ("docs", str(self.prefix))
        )

    def test_ambiguous_project_is_an_error(self) -> None:
        info = {
            "environments_info": [
                {"name": "test", "prefix": str(self.prefix)},
                {"name": "docs", "prefix": str(self.prefix)},
            ]
        }
        with self.assertRaisesRegex(sync.SyncError, "test, docs"):
            sync.select_environment(info, None)

    def test_missing_prefix_is_an_error(self) -> None:
        info = self.info(prefix=self.root / "gone")
        with self.assertRaisesRegex(sync.SyncError, "pixi install"):
            sync.select_environment(info, None)


class BinaryPathTests(TemporaryProject):
    def test_existing_executable(self) -> None:
        make_executable(self.prefix / "bin" / "rust-analyzer")
        self.assertEqual(
            sync.binary_path(str(self.prefix), "rust-analyzer", False),
            str(self.prefix / "bin" / "rust-analyzer"),
        )

    def test_missing_binary(self) -> None:
        self.assertEqual(sync.binary_path(str(self.prefix), "taplo", False), "")

    def test_non_executable_binary(self) -> None:
        path = self.prefix / "bin" / "taplo"
        path.write_text("", encoding="utf-8")
        self.assertEqual(sync.binary_path(str(self.prefix), "taplo", False), "")


class EnvironmentTests(TemporaryProject):
    def test_path_starts_with_environment_bin(self) -> None:
        env_bin = str(self.prefix / "bin")
        with mock.patch.dict(os.environ, {"PATH": f"/host/bin:{env_bin}:/other"}):
            env = sync.environment_vars(
                "default", str(self.prefix), self.root, self.manifest, False
            )
        self.assertEqual(env["PATH"].split(":"), [env_bin, "/host/bin", "/other"])
        self.assertEqual(env["VIRTUAL_ENV"], str(self.prefix))
        self.assertEqual(env["CONDA_PREFIX"], str(self.prefix))
        self.assertEqual(env["PIXI_PROJECT_ROOT"], str(self.root))
        self.assertEqual(env["PIXI_PROJECT_MANIFEST"], str(self.manifest))
        self.assertEqual(env["PIXI_ENVIRONMENT_NAME"], "default")


class DiscoverTests(TemporaryProject):
    def test_only_installed_servers_are_returned(self) -> None:
        for binary in ("rust-analyzer", "basedpyright-langserver", "taplo"):
            make_executable(self.prefix / "bin" / binary)
        env = {"PATH": "x"}
        lsp, dap, languages = sync.discover(str(self.prefix), False, env)
        self.assertEqual(
            sorted(lsp), ["basedpyright", "rust-analyzer", "taplo"]
        )
        self.assertEqual(lsp["basedpyright"]["binary"]["arguments"], ["--stdio"])
        self.assertEqual(lsp["taplo"]["binary"]["arguments"], ["lsp", "stdio"])
        self.assertTrue(lsp["rust-analyzer"]["binary"]["ignore_system_version"])
        self.assertEqual(lsp["taplo"]["binary"]["env"], env)
        self.assertEqual(dap, {})
        self.assertEqual(languages, {})

    def test_debug_adapter_uses_adapter_binary(self) -> None:
        make_executable(self.prefix / "bin" / "lldb-dap")
        _, dap, _ = sync.discover(str(self.prefix), False, {"PATH": "x"})
        self.assertEqual(dap["CodeLLDB"]["binary"], str(self.prefix / "bin" / "lldb-dap"))

    def test_debugpy_requires_the_module(self) -> None:
        make_executable(self.prefix / "bin" / "python")
        with mock.patch.object(sync, "python_imports", return_value=False):
            _, dap, _ = sync.discover(str(self.prefix), False, {})
        self.assertEqual(dap, {})
        with mock.patch.object(sync, "python_imports", return_value=True):
            _, dap, _ = sync.discover(str(self.prefix), False, {})
        self.assertIn("Debugpy", dap)
        self.assertEqual(dap["Debugpy"]["args"], ["-m", "debugpy.adapter"])

    def test_formatter_is_added_for_installed_binary(self) -> None:
        make_executable(self.prefix / "bin" / "prettier")
        _, _, languages = sync.discover(str(self.prefix), False, {})
        self.assertIn("TypeScript", languages)
        self.assertTrue(
            languages["TypeScript"]["formatter"]["external"]["command"].endswith(
                "prettier"
            )
        )


class SettingsTests(TemporaryProject):
    def test_merge_keeps_user_settings_and_respects_formatter(self) -> None:
        make_executable(self.prefix / "bin" / "rust-analyzer")
        make_executable(self.prefix / "bin" / "prettier")
        existing = {
            "theme": "One Dark",
            "lsp": {
                "rust-analyzer": {
                    "settings": {"check": {"command": "clippy"}},
                    "binary": {"pre_release": True},
                },
                "other": {"binary": {"path": "/opt/other"}},
            },
            "languages": {"TypeScript": {"formatter": "language_server"}},
        }
        settings = sync.managed_settings(
            "default", str(self.prefix), self.root, self.manifest, False, existing
        )
        self.assertEqual(settings["theme"], "One Dark")
        self.assertEqual(
            settings["lsp"]["rust-analyzer"]["settings"],
            {"check": {"command": "clippy"}},
        )
        self.assertTrue(settings["lsp"]["rust-analyzer"]["binary"]["pre_release"])
        self.assertEqual(
            settings["lsp"]["rust-analyzer"]["binary"]["path"],
            str(self.prefix / "bin" / "rust-analyzer"),
        )
        self.assertEqual(settings["lsp"]["other"], {"binary": {"path": "/opt/other"}})
        self.assertEqual(
            settings["languages"]["TypeScript"]["formatter"], "language_server"
        )
        self.assertEqual(settings["terminal"]["env"]["VIRTUAL_ENV"], str(self.prefix))

    def test_does_not_add_unmanaged_sections(self) -> None:
        settings = sync.managed_settings(
            "default", str(self.prefix), self.root, self.manifest, False, {}
        )
        self.assertNotIn("dap", settings)
        self.assertNotIn("languages", settings)


class TaskTests(TemporaryProject):
    def test_tasks_come_from_the_manifest(self) -> None:
        (self.root / "scripts").mkdir()
        (self.root / "scripts" / "zed_pixi_sync.py").write_text("", encoding="utf-8")
        tasks = sync.managed_tasks(self.root, "pixi", sync.pixi_tasks(self.manifest))
        self.assertEqual([task["label"] for task in tasks], ["pixi: build", "pixi: sync Zed config"])
        self.assertEqual(tasks[0]["args"], ["run", "build"])
        self.assertEqual(tasks[1]["command"], "python3")

    def test_merge_tasks_keeps_user_tasks(self) -> None:
        generated = [{"label": "pixi: build", "command": "pixi"}]
        existing = [
            {"label": "pixi: build", "command": "old"},
            {"label": "my task", "command": "echo"},
        ]
        merged = sync.merge_tasks(existing, generated)
        self.assertEqual(
            merged,
            [{"label": "my task", "command": "echo"}, {"label": "pixi: build", "command": "pixi"}],
        )


class EndToEndTests(TemporaryProject):
    def setUp(self) -> None:
        super().setUp()
        make_executable(self.prefix / "bin" / "rust-analyzer")
        self.pixi = self.root / "fake-pixi"
        self.pixi.write_text(
            "#!/bin/sh\ncat <<'JSON'\n"
            + json.dumps(self.info())
            + "\nJSON\n",
            encoding="utf-8",
        )
        self.pixi.chmod(0o755)

    def run_main(self, *arguments: str) -> tuple[int, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = sync.main(
                [
                    "--root",
                    str(self.root),
                    "--pixi",
                    str(self.pixi),
                    *arguments,
                ]
            )
        return code, out.getvalue() + err.getvalue()

    def test_writes_then_checks(self) -> None:
        code, output = self.run_main()
        self.assertEqual(code, 0, output)
        settings = json.loads((self.root / ".zed" / "settings.json").read_text())
        self.assertIn("rust-analyzer", settings["lsp"])
        self.assertTrue((self.root / ".zed" / "tasks.json").exists())

        code, output = self.run_main("--check")
        self.assertEqual(code, 0, output)

    def test_check_reports_stale_settings(self) -> None:
        self.assertEqual(self.run_main()[0], 0)
        settings_path = self.root / ".zed" / "settings.json"
        settings = json.loads(settings_path.read_text())
        settings["lsp"]["rust-analyzer"]["binary"]["path"] = "/gone/rust-analyzer"
        settings_path.write_text(json.dumps(settings), encoding="utf-8")
        code, output = self.run_main("--check")
        self.assertEqual(code, 1)
        self.assertIn("stale", output)

    def test_check_ignores_user_added_keys(self) -> None:
        self.assertEqual(self.run_main()[0], 0)
        settings_path = self.root / ".zed" / "settings.json"
        settings = json.loads(settings_path.read_text())
        settings["theme"] = "One Dark"
        settings["lsp"]["rust-analyzer"]["settings"] = {"check": {"command": "clippy"}}
        settings_path.write_text(json.dumps(settings), encoding="utf-8")
        code, output = self.run_main("--check")
        self.assertEqual(code, 0, output)

    def test_check_reports_missing_binary(self) -> None:
        self.assertEqual(self.run_main()[0], 0)
        (self.prefix / "bin" / "rust-analyzer").unlink()
        code, output = self.run_main("--check")
        self.assertEqual(code, 1)
        self.assertIn("does not exist", output)

    def test_missing_manifest_is_an_error(self) -> None:
        self.manifest.unlink()
        code, output = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("no pixi.toml", output)


if __name__ == "__main__":
    unittest.main()
