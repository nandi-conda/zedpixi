#!/usr/bin/env python3
"""Write Zed project settings that run built-in tools from a pixi environment.

Zed extensions cannot change the environment of Zed's built-in language
servers, debug adapters, tasks or formatters, and they cannot write files.
This script does the routing instead: it resolves the pixi environment of the
project it is run in, then writes project-local Zed configuration that points
the (built-in) tools at the binaries in that environment. Debug adapters the
extension declares itself (`pixi-gdb`, `pixi-lldb`) need none of this.

    python3 scripts/zed_pixi_sync.py            # write the configuration
    python3 scripts/zed_pixi_sync.py --check    # verify it is current

Run it from the project root, or pass --root. The generated files are
machine-specific because Zed requires absolute paths; add them to your
.gitignore:

    /.zed/settings.json

Usage:
    zed_pixi_sync.py [--root DIR] [--manifest PATH] [--environment NAME]
                     [--pixi PATH] [--check] [--quiet]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

MANIFEST_FILE = "pixi.toml"
DEFAULT_ENVIRONMENT = "default"
SETTINGS_FILE = Path(".zed") / "settings.json"
TASKS_FILE = Path(".zed") / "tasks.json"


class SyncError(Exception):
    """A problem the user has to fix (missing manifest, pixi, environment)."""


@dataclass(frozen=True)
class Lsp:
    """A language server Zed ships, and the binaries a pixi package installs."""

    id: str
    binaries: tuple[tuple[str, tuple[str, ...]], ...]


@dataclass(frozen=True)
class Dap:
    """A debug adapter Zed ships, and the binaries a pixi package installs.

    `module` names the Python module the adapter runs from; when set, the
    adapter is only used if the environment's interpreter can import it.
    """

    adapter: str
    binaries: tuple[tuple[str, tuple[str, ...]], ...]
    module: str | None = None


@dataclass(frozen=True)
class Formatter:
    """An external formatter, keyed by the Zed language name."""

    languages: tuple[str, ...]
    binary: str
    arguments: tuple[str, ...]


# The language servers Zed runs itself. A pixi dependency that provides one of
# these binaries makes Zed use the copy from the environment.
#
# `id` is Zed's language server id, which is also what the `lsp` section of
# settings.json uses. `arguments` are the LSP-over-stdio arguments the server
# needs; Zed normally appends them itself, but an explicit binary path replaces
# Zed's whole launch recipe, so they have to be repeated here.
LANGUAGE_SERVERS: tuple[Lsp, ...] = (
    Lsp("rust-analyzer", (("rust-analyzer", ()),)),
    Lsp("basedpyright", (("basedpyright-langserver", ("--stdio",)),)),
    Lsp("taplo", (("taplo", ("lsp", "stdio")),)),
    Lsp("yaml-language-server", (("yaml-language-server", ("--stdio",)),)),
    Lsp("typescript-language-server", (("typescript-language-server", ("--stdio",)),)),
    Lsp("gopls", (("gopls", ()),)),
    Lsp("clangd", (("clangd", ()),)),
    Lsp("ruff", (("ruff", ("server",)),)),
    # Zed's `lean4` extension starts `lake serve --`; a conda-forge `lean4`
    # dependency installs that `lake` (with `lean` next to it) in the env.
    # `lean --server` is not the server the extension launches.
    Lsp("lean4-lsp", (("lake", ("serve", "--")),)),
)

# Debug adapters. `binary` is the adapter, not the debugger. Zed's `dap`
# settings accept `binary`, `args` and `env` for adapters it ships.
DEBUG_ADAPTERS: tuple[Dap, ...] = (
    Dap("CodeLLDB", (("lldb-dap", ()), ("lldb-vscode", ()))),
    Dap("Debugpy", (("python", ("-m", "debugpy.adapter")),), module="debugpy"),
    Dap("Delve", (("dlv", ("dap",)),)),
)

# External formatters have no `env` setting, so they are only used when the
# environment installs a runnable binary with an absolute interpreter in its
# shebang (conda shims do). A formatter is only written when the project does
# not already configure one for that language.
EXTERNAL_FORMATTERS: tuple[Formatter, ...] = (
    Formatter(
        ("JavaScript", "TypeScript", "TSX", "JSX", "JSON", "CSS", "HTML"),
        "prettier",
        ("--stdin-filepath", "{buffer_path}"),
    ),
    Formatter(("Lua",), "stylua", ("-",)),
)


def which_pixi(explicit: str | None) -> str:
    if explicit:
        return explicit
    found = shutil.which("pixi")
    if not found:
        raise SyncError(
            "pixi was not found on PATH. Install pixi, or pass --pixi PATH."
        )
    return found


def pixi_info(pixi: str, manifest: Path) -> dict:
    try:
        completed = subprocess.run(
            [pixi, "info", "--json", "--manifest-path", str(manifest)],
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        raise SyncError(f"could not run {pixi!r}: {error}") from error
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise SyncError(
            f"`{pixi} info --json --manifest-path {manifest}` failed "
            f"(exit {completed.returncode}): {detail}"
        )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise SyncError(f"could not parse `pixi info --json` output: {error}") from error


def select_environment(info: dict, requested: str | None) -> tuple[str, str]:
    """Pick (name, prefix) from `pixi info --json`, the way the extension does."""
    environments = info.get("environments_info")
    if not isinstance(environments, list) or not environments:
        raise SyncError("`pixi info --json` did not contain `environments_info`")
    names = [env.get("name") for env in environments if isinstance(env, dict)]

    if requested is not None:
        selected = next(
            (env for env in environments if env.get("name") == requested), None
        )
        if selected is None:
            raise SyncError(
                f"this project has no {requested!r} environment; "
                f"available environments: {', '.join(str(n) for n in names)}"
            )
    else:
        selected = next(
            (env for env in environments if env.get("name") == DEFAULT_ENVIRONMENT),
            None,
        )
        if selected is None and len(environments) == 1:
            selected = environments[0]
        if selected is None:
            raise SyncError(
                f"this project has no `{DEFAULT_ENVIRONMENT}` pixi environment; "
                f"available environments: {', '.join(str(n) for n in names)}. "
                "Pass --environment NAME."
            )

    name = selected.get("name") or DEFAULT_ENVIRONMENT
    prefix = selected.get("prefix")
    if not isinstance(prefix, str) or not prefix:
        raise SyncError(
            f"pixi environment {name!r} has no prefix; run `pixi install` first"
        )
    if not Path(prefix).is_dir():
        raise SyncError(
            f"pixi environment {name!r} points at {prefix}, which does not exist; "
            "run `pixi install` first"
        )
    return name, prefix


def bin_dir(prefix: str, windows: bool) -> str:
    return os.path.join(prefix, "Scripts" if windows else "bin")


def binary_path(prefix: str, binary: str, windows: bool) -> str:
    """The environment path for `binary`, or "" when it is not installed.

    Mirrors the extension's candidate rules: `Scripts/` first on Windows (where
    console scripts live), then `bin/`, with the usual executable suffixes.
    """
    root = bin_dir(prefix, windows)
    if windows:
        for directory in ("Scripts", "bin"):
            base = os.path.join(prefix, directory)
            for suffix in (".exe", ".cmd", ".bat", ""):
                path = os.path.join(base, binary + suffix)
                if os.path.isfile(path):
                    return path
        return ""
    path = os.path.join(root, binary)
    if not os.path.isfile(path) or (os.name != "nt" and not os.access(path, os.X_OK)):
        return ""
    return path


def python_imports(python: str, module: str) -> bool:
    try:
        completed = subprocess.run(
            [python, "-c", f"import {module}"],
            check=False,
            capture_output=True,
            timeout=60,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return completed.returncode == 0


def environment_vars(
    name: str, prefix: str, root: Path, manifest: Path, windows: bool
) -> dict[str, str]:
    """Environment for a tool started from the pixi environment.

    PATH starts with the environment's bin directory, so programs the tool
    spawns (python, node, rustfmt, ...) resolve inside the environment first.
    The rest of PATH is the one this script saw; regenerate after changing your
    shell PATH.
    """
    separator = ";" if windows else ":"
    binpath = bin_dir(prefix, windows)
    inherited = [
        entry
        for entry in os.environ.get("PATH", "").split(separator)
        if entry and os.path.normcase(os.path.abspath(entry)) != os.path.normcase(binpath)
    ]
    return {
        "PATH": separator.join([binpath, *inherited]),
        "VIRTUAL_ENV": prefix,
        "CONDA_PREFIX": prefix,
        "PIXI_PROJECT_ROOT": str(root),
        "PIXI_PROJECT_MANIFEST": str(manifest),
        "PIXI_ENVIRONMENT_NAME": name,
    }


def discover(
    prefix: str, windows: bool, env: dict[str, str]
) -> tuple[dict[str, dict], dict[str, dict], dict[str, dict]]:
    """Build the managed `lsp`, `dap` and `languages` settings fragments."""
    lsp: dict[str, dict] = {}
    for server in LANGUAGE_SERVERS:
        for binary, arguments in server.binaries:
            path = binary_path(prefix, binary, windows)
            if path:
                binary_settings = {
                    "path": path,
                    "ignore_system_version": True,
                    "env": dict(env),
                }
                if arguments:
                    binary_settings["arguments"] = list(arguments)
                lsp[server.id] = {"binary": binary_settings}
                break

    dap: dict[str, dict] = {}
    for adapter in DEBUG_ADAPTERS:
        for binary, arguments in adapter.binaries:
            path = binary_path(prefix, binary, windows)
            if not path:
                continue
            if adapter.module and not python_imports(path, adapter.module):
                continue
            settings = {"binary": path, "env": dict(env)}
            if arguments:
                settings["args"] = list(arguments)
            dap[adapter.adapter] = settings
            break

    languages: dict[str, dict] = {}
    for formatter in EXTERNAL_FORMATTERS:
        path = binary_path(prefix, formatter.binary, windows)
        if not path:
            continue
        for language in formatter.languages:
            languages.setdefault(
                language,
                {
                    "formatter": {
                        "external": {
                            "command": path,
                            "arguments": list(formatter.arguments),
                        }
                    }
                },
            )
    return lsp, dap, languages


def merge_env(base: dict, extra: dict) -> dict:
    merged = dict(base)
    merged.update(extra)
    return merged


def managed_settings(
    name: str,
    prefix: str,
    root: Path,
    manifest: Path,
    windows: bool,
    existing: dict,
) -> dict:
    """Return the generated settings, merged into `existing` where needed."""
    env = environment_vars(name, prefix, root, manifest, windows)
    lsp, dap, languages = discover(prefix, windows, env)

    settings = json.loads(json.dumps(existing))  # deep copy

    managed_lsp = settings.setdefault("lsp", {}) if lsp or "lsp" in settings else {}
    for server_id, fragment in lsp.items():
        current = managed_lsp.get(server_id)
        if not isinstance(current, dict):
            managed_lsp[server_id] = fragment
            continue
        binary = current.get("binary")
        if isinstance(binary, dict):
            binary.update(fragment["binary"])
        else:
            current["binary"] = fragment["binary"]

    managed_dap = settings.setdefault("dap", {}) if dap or "dap" in settings else {}
    for adapter, fragment in dap.items():
        current = managed_dap.get(adapter)
        if not isinstance(current, dict):
            managed_dap[adapter] = fragment
            continue
        current.update(fragment)

    terminal = settings.setdefault("terminal", {})
    terminal["env"] = merge_env(
        terminal.get("env") if isinstance(terminal.get("env"), dict) else {}, env
    )

    if languages:
        managed_languages = settings.setdefault("languages", {})
        for language, fragment in languages.items():
            current = managed_languages.get(language)
            if not isinstance(current, dict):
                managed_languages[language] = fragment
                continue
            # Respect a formatter the project already chose.
            current.setdefault("formatter", fragment["formatter"])

    return settings


def read_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SyncError(f"could not read {path}: {error}") from error
    if not isinstance(value, dict):
        raise SyncError(f"{path} must contain a JSON object")
    return value


def pixi_tasks(manifest: Path) -> list[str]:
    try:
        text = manifest.read_text(encoding="utf-8")
    except OSError as error:
        raise SyncError(f"could not read {manifest}: {error}") from error
    try:
        document = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise SyncError(f"could not parse {manifest}: {error}") from error
    tasks = document.get("tasks", {})
    return sorted(tasks) if isinstance(tasks, dict) else []


def managed_tasks(root: Path, pixi: str, commands: list[str]) -> list[dict]:
    """Zed tasks for every pixi task, plus one task that regenerates this file.

    `pixi` is a bare name when it is on PATH (portable), otherwise the absolute
    path this run used. `pixi run` activates the environment itself, so the
    tasks need no explicit env.
    """
    command = "pixi" if os.path.basename(pixi) == "pixi" else pixi
    tasks = [
        {
            "label": f"pixi: {name}",
            "command": command,
            "args": ["run", name],
            "cwd": str(root),
            "use_new_terminal": False,
            "allow_concurrent_runs": False,
        }
        for name in commands
    ]
    sync = root / "scripts" / "zed_pixi_sync.py"
    if sync.exists():
        tasks.append(
            {
                "label": "pixi: sync Zed config",
                "command": "python3",
                "args": [str(sync), "--root", str(root)],
                "cwd": str(root),
            }
        )
    return tasks


def merge_tasks(existing: list[dict], generated: list[dict]) -> list[dict]:
    """Replace generated labels, keep everything the user added."""
    managed = {task["label"] for task in generated}
    kept = [
        task
        for task in existing
        if isinstance(task, dict) and task.get("label") not in managed
    ]
    return kept + generated


def write_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def _binary_problems(settings: dict, windows: bool) -> list[str]:
    problems: list[str] = []
    for server_id, entry in settings.get("lsp", {}).items():
        binary = entry.get("binary", {}) if isinstance(entry, dict) else {}
        path = binary.get("path")
        if isinstance(path, str) and not os.path.isfile(path):
            problems.append(f"lsp.{server_id}.binary.path does not exist: {path}")
        elif isinstance(path, str) and not windows and not os.access(path, os.X_OK):
            problems.append(f"lsp.{server_id}.binary.path is not executable: {path}")
    for adapter, entry in settings.get("dap", {}).items():
        path = entry.get("binary") if isinstance(entry, dict) else None
        if isinstance(path, str) and not os.path.isfile(path):
            problems.append(f"dap.{adapter}.binary does not exist: {path}")
    return problems


def contains(expected, actual) -> bool:
    """Whether every managed value in `expected` is present in `actual`."""
    if isinstance(expected, dict):
        return isinstance(actual, dict) and all(
            key in actual and contains(value, actual[key])
            for key, value in expected.items()
        )
    if isinstance(expected, list):
        return expected == actual
    return expected == actual


def check(
    root: Path, settings: dict, tasks: list[dict], windows: bool, quiet: bool
) -> int:
    problems: list[str] = []
    on_disk = read_json(root / SETTINGS_FILE)

    for section in ("lsp", "dap"):
        for key, fragment in settings.get(section, {}).items():
            actual = on_disk.get(section, {}).get(key)
            if not contains(fragment, actual):
                problems.append(
                    f"{section}.{key} is stale in {SETTINGS_FILE}; "
                    "run `python3 scripts/zed_pixi_sync.py`"
                )
    terminal_env = on_disk.get("terminal", {}).get("env", {})
    for key, value in settings.get("terminal", {}).get("env", {}).items():
        if terminal_env.get(key) != value:
            problems.append(
                f"terminal.env.{key} is stale in {SETTINGS_FILE}; "
                "run `python3 scripts/zed_pixi_sync.py`"
            )

    on_disk_tasks = root / TASKS_FILE
    if on_disk_tasks.exists():
        try:
            existing_tasks = json.loads(on_disk_tasks.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            problems.append(f"could not read {TASKS_FILE}: {error}")
            existing_tasks = []
        labelled = {
            task.get("label"): task
            for task in existing_tasks
            if isinstance(task, dict)
        }
        for task in tasks:
            if not contains(task, labelled.get(task["label"])):
                problems.append(
                    f"task {task['label']!r} is stale in {TASKS_FILE}; "
                    "run `python3 scripts/zed_pixi_sync.py`"
                )
    else:
        problems.append(f"{TASKS_FILE} is missing; run `python3 scripts/zed_pixi_sync.py`")

    problems.extend(_binary_problems(settings, windows))

    if problems:
        for problem in problems:
            print(f"zed-pixi: {problem}", file=sys.stderr)
        return 1
    if not quiet:
        print(f"zed-pixi: {SETTINGS_FILE} and {TASKS_FILE} are current")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", default=".", help="project root (default: .)")
    parser.add_argument("--manifest", default=None, help="pixi manifest (default: <root>/pixi.toml)")
    parser.add_argument("--environment", default=None, help="pixi environment name")
    parser.add_argument("--pixi", default=None, help="path to the pixi executable")
    parser.add_argument("--check", action="store_true", help="verify instead of writing")
    parser.add_argument("--quiet", action="store_true", help="print only problems")
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    manifest = Path(args.manifest).resolve() if args.manifest else root / MANIFEST_FILE
    windows = os.name == "nt"

    try:
        if not manifest.is_file():
            raise SyncError(f"no {manifest.name} at {manifest}; nothing to sync")
        pixi = which_pixi(args.pixi)
        info = pixi_info(pixi, manifest)
        name, prefix = select_environment(info, args.environment)

        existing_settings = read_json(root / SETTINGS_FILE)
        settings = managed_settings(name, prefix, root, manifest, windows, existing_settings)
        tasks = managed_tasks(root, pixi, pixi_tasks(manifest))

        if args.check:
            return check(root, settings, tasks, windows, args.quiet)

        write_json(root / SETTINGS_FILE, settings)
        existing_tasks_path = root / TASKS_FILE
        existing_tasks: list[dict] = []
        if existing_tasks_path.exists():
            try:
                loaded = json.loads(existing_tasks_path.read_text(encoding="utf-8"))
                if isinstance(loaded, list):
                    existing_tasks = loaded
            except (OSError, json.JSONDecodeError):
                pass
        write_json(existing_tasks_path, merge_tasks(existing_tasks, tasks))

        if not args.quiet:
            servers = ", ".join(sorted(settings.get("lsp", {}))) or "none"
            adapters = ", ".join(sorted(settings.get("dap", {}))) or "none"
            print(f"zed-pixi: environment {name!r} at {prefix}")
            print(f"zed-pixi: language servers: {servers}")
            print(f"zed-pixi: debug adapters: {adapters}")
            print(f"zed-pixi: wrote {SETTINGS_FILE} and {TASKS_FILE}")
        return 0
    except SyncError as error:
        print(f"zed-pixi: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
