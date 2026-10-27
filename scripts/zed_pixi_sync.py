#!/usr/bin/env python3
"""Write Zed project settings that run built-in tools from a pixi environment.

Zed extensions cannot change the environment of Zed's built-in language
servers, debug adapters, tasks or formatters, and they cannot write files.
This script does the routing instead: it resolves the pixi environment of the
workspace root's own `pixi.toml`, then writes project-local Zed configuration
that points the (built-in) tools at the binaries in that environment. Debug
adapters the extension declares itself (`pixi-gdb`, `pixi-lldb`) need none of
this. The generator never uses an ambient pixi project, a parent manifest,
another prefix or a hardcoded toolchain: `pixi info` must describe the very
manifest that is passed, or the script refuses to write.

    python3 scripts/zed_pixi_sync.py            # write the configuration
    python3 scripts/zed_pixi_sync.py --check    # verify it is current

Run it from the project root, or pass --root. The generated files are
machine-specific because Zed requires absolute paths; add them to your
.gitignore:

    /.zed/settings.json

Usage:
    zed_pixi_sync.py [--root DIR] [--environment NAME]
                     [--pixi PATH] [--check] [--quiet]

The manifest is always <root>/pixi.toml. There is no option to point the
generator at a different manifest, because the generated files belong to the
workspace root.
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
    """A language server with a Zed settings id, and its pixi binaries."""

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


# The language servers the generator configures. A pixi dependency that
# provides one of these binaries makes Zed use the copy from the environment.
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
    # Zed's Lean 4 extension (server id `lean4-lsp`) starts `lake serve --`; a
    # pixi `lean4` package installs that `lake`. The extension's own recipe
    # looks for `lake` on the worktree shell PATH or in ELAN_HOME, never in the
    # pixi environment, so the explicit path below is what makes it work.
    Lsp("lean4-lsp", (("lake", ("serve", "--")),)),
)

# Servers for which a hand-written entry wins over the environment copy. A
# project that configures one of these by hand (an elan toolchain, a
# lean-toolchain pin) has chosen its toolchain deliberately; silently pointing
# it at the pixi copy would change which Lean version it runs. Only entries
# without the generated markers are kept; a generated entry is still updated
# and pruned like every other.
HAND_WRITTEN_WINS = frozenset({"lean4-lsp"})

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


def clean_env() -> dict[str, str]:
    """The process environment with every activation variable removed.

    pixi is asked about a project with only `--manifest-path` deciding which
    one. Dropping the variables an activated shell exports means an ambient
    pixi or conda project cannot influence the answer.
    """
    dropped = {
        "PIXI_PROJECT_ROOT",
        "PIXI_PROJECT_MANIFEST",
        "PIXI_WORKSPACE_ROOT",
        "PIXI_WORKSPACE_MANIFEST",
        "PIXI_ENVIRONMENT_NAME",
        "CONDA_PREFIX",
        "CONDA_DEFAULT_ENV",
        "VIRTUAL_ENV",
    }
    return {key: value for key, value in os.environ.items() if key not in dropped}


def pixi_info(pixi: str, manifest: Path) -> dict:
    try:
        completed = subprocess.run(
            [pixi, "info", "--json", "--manifest-path", str(manifest)],
            check=False,
            capture_output=True,
            text=True,
            cwd=str(manifest.parent),
            env=clean_env(),
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


def verify_project(info: dict, manifest: Path) -> None:
    """Refuse output that `pixi` did not describe for `manifest` itself.

    `pixi info` exits 0 and reports `project_info: null` when there is no
    project, and a pixi shim or a changed CLI can answer for a different
    manifest. Either case would make the generated settings describe a foreign
    project, so both are errors here.
    """
    project = info.get("project_info")
    if not isinstance(project, dict):
        raise SyncError(
            f"`pixi info` described no project for {manifest}; "
            "the generator only writes settings for the workspace root's own "
            f"{MANIFEST_FILE}"
        )
    reported = project.get("manifest_path")
    if not isinstance(reported, str) or not reported:
        raise SyncError(
            "`pixi info` did not report project_info.manifest_path; "
            "cannot tell which project the environment belongs to"
        )
    if Path(reported).resolve() != manifest.resolve():
        raise SyncError(
            f"`pixi info` described the project at {reported}, not {manifest}; "
            "refusing to write settings for a foreign pixi project"
        )


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


def is_environment_dir(entry: str) -> bool:
    """Whether a PATH entry is the executable directory of a conda/pixi env.

    Every conda and pixi prefix contains `conda-meta` next to its `bin` (or
    `Scripts`) directory, including a global or tool environment that happens
    to be activated in the shell that runs this script.
    """
    return (Path(entry).parent / "conda-meta").is_dir()


def system_path(chosen_bin: str, windows: bool) -> list[str]:
    """The PATH entries usable for basic system tools, environments removed.

    Starts from the PATH this script was run with, then drops every entry that
    is the bin directory of a conda/pixi environment (and the `bin` of
    `CONDA_PREFIX`/`VIRTUAL_ENV`), as well as the chosen environment's own
    directory. A foreign or ambient environment therefore never leaks into the
    generated settings; what remains is only used to find basic system tools,
    never a tool the generator configures.
    """
    separator = ";" if windows else ":"
    chosen = os.path.normcase(os.path.abspath(chosen_bin))
    ambient_prefixes = {
        os.path.normcase(os.path.abspath(value))
        for key in ("CONDA_PREFIX", "VIRTUAL_ENV")
        if (value := os.environ.get(key))
    }
    entries: list[str] = []
    seen = {chosen}
    for entry in os.environ.get("PATH", "").split(separator):
        if not entry:
            continue
        absolute = os.path.normcase(os.path.abspath(entry))
        if absolute in seen or is_environment_dir(entry):
            continue
        prefix = os.path.dirname(os.path.abspath(entry).rstrip("/\\"))
        if os.path.normcase(prefix) in ambient_prefixes:
            continue
        seen.add(absolute)
        entries.append(entry)
    return entries


def environment_vars(
    name: str, prefix: str, root: Path, manifest: Path, windows: bool
) -> dict[str, str]:
    """Environment for a tool started from the pixi environment.

    PATH starts with the environment's own executable directory, so programs
    the tool spawns (python, node, rustfmt, ...) resolve inside the selected
    environment first. The rest is the system part of the PATH this script saw,
    with every conda/pixi environment directory removed: no other project's
    environment is ever put on it.
    """
    separator = ";" if windows else ":"
    binpath = bin_dir(prefix, windows)
    inherited = system_path(binpath, windows)
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


def entry_env(entry) -> dict:
    """The `binary.env` block of an `lsp` or `dap` entry, or {}."""
    binary = entry.get("binary") if isinstance(entry, dict) else None
    env = binary.get("env") if isinstance(binary, dict) else None
    return env if isinstance(env, dict) else {}


def is_generated(entry) -> bool:
    """Whether an entry carries the marker every generated entry gets.

    The marker declares ownership: the generator may rewrite or remove such an
    entry. A hand-written entry must not copy the `PIXI_*` keys from the
    generated examples, or it will be treated as generated and pruned when the
    selected environment does not provide that tool.
    """
    return "PIXI_PROJECT_ROOT" in entry_env(entry)


def is_within(path: str, directory: str) -> bool:
    """Whether `path` is `directory` or below it."""
    try:
        return os.path.commonpath(
            [os.path.abspath(path), os.path.abspath(directory)]
        ) == os.path.abspath(directory)
    except ValueError:
        return False


def _lsp_path(entry) -> str | None:
    binary = entry.get("binary") if isinstance(entry, dict) else None
    path = binary.get("path") if isinstance(binary, dict) else None
    return path if isinstance(path, str) else None


def _dap_path(entry) -> str | None:
    path = entry.get("binary") if isinstance(entry, dict) else None
    return path if isinstance(path, str) else None


def prune_generated(
    section: dict,
    discovered: dict,
    root: Path,
    manifest: Path,
    name: str,
    prefix: str,
    path_of,
) -> None:
    """Drop entries this generator wrote for another project or environment.

    Only entries with the generated marker are touched; anything the user
    configured by hand is left alone.
    """
    for key in list(section):
        entry = section[key]
        if not is_generated(entry):
            continue
        env = entry_env(entry)
        path = path_of(entry)
        foreign = (
            env.get("PIXI_PROJECT_ROOT") != str(root)
            or env.get("PIXI_PROJECT_MANIFEST") != str(manifest)
            or env.get("PIXI_ENVIRONMENT_NAME") != name
            or (path is not None and path != "" and not is_within(path, prefix))
        )
        if foreign or key not in discovered:
            del section[key]


def prune_languages(languages: dict, discovered: dict, prefix: str) -> None:
    """Drop generated formatter commands that came from another environment."""
    for language in list(languages):
        if language in discovered:
            continue
        entry = languages[language]
        formatter = entry.get("formatter") if isinstance(entry, dict) else None
        external = formatter.get("external") if isinstance(formatter, dict) else None
        command = external.get("command") if isinstance(external, dict) else None
        if not isinstance(command, str) or not os.path.isabs(command):
            continue
        own_prefix = is_within(command, prefix)
        other_env = is_environment_dir(os.path.dirname(command))
        if own_prefix or other_env:
            del languages[language]


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

    existing_lsp = settings.get("lsp")
    if isinstance(existing_lsp, dict):
        prune_generated(existing_lsp, lsp, root, manifest, name, prefix, _lsp_path)
    existing_dap = settings.get("dap")
    if isinstance(existing_dap, dict):
        prune_generated(existing_dap, dap, root, manifest, name, prefix, _dap_path)

    managed_lsp = settings.setdefault("lsp", {}) if lsp or "lsp" in settings else {}
    for server_id, fragment in lsp.items():
        current = managed_lsp.get(server_id)
        if not isinstance(current, dict):
            managed_lsp[server_id] = fragment
            continue
        if server_id in HAND_WRITTEN_WINS and not is_generated(current):
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

    existing_languages = settings.get("languages")
    if isinstance(existing_languages, dict):
        prune_languages(existing_languages, languages, prefix)

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


def managed_tasks(
    root: Path, pixi: str, prefix: str, windows: bool, commands: list[str]
) -> list[dict]:
    """Zed tasks for every pixi task, plus one task that regenerates this file.

    `pixi` is a bare name when it is on PATH (portable), otherwise the absolute
    path this run used. `pixi run` activates the environment itself, so the
    tasks need no explicit env. The sync task runs the environment's own
    interpreter, never a `python3` from the host PATH; when the environment has
    no interpreter the task is omitted.
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
    python = binary_path(prefix, "python", windows)
    if sync.exists() and python:
        tasks.append(
            {
                "label": "pixi: sync Zed config",
                "command": python,
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


RUN_HINT = "run `python3 scripts/zed_pixi_sync.py`"


def _project_problems(
    on_disk: dict,
    settings: dict,
    root: Path,
    manifest: Path,
    name: str,
    prefix: str,
    windows: bool,
) -> list[str]:
    """Problems with the generated entries that are on disk.

    Every entry this generator wrote has the project markers in its `env`
    block. An entry whose markers name another project, another environment or
    a path outside the current prefix is a leftover from a foreign or stale
    environment and must be reported, not silently accepted.
    """
    problems: list[str] = []
    expected_markers = {
        "PIXI_PROJECT_ROOT": str(root),
        "PIXI_PROJECT_MANIFEST": str(manifest),
        "PIXI_ENVIRONMENT_NAME": name,
    }
    for section in ("lsp", "dap"):
        entries = on_disk.get(section)
        if not isinstance(entries, dict):
            continue
        path_of = _lsp_path if section == "lsp" else _dap_path
        for key, entry in entries.items():
            if not is_generated(entry):
                continue
            env = entry_env(entry)
            foreign = False
            for marker, expected in expected_markers.items():
                actual = env.get(marker)
                if actual != expected:
                    foreign = True
                    problems.append(
                        f"{section}.{key}.binary.env.{marker} is {actual!r}, not "
                        f"{expected!r}: the entry was written for a foreign or "
                        f"stale pixi environment; {RUN_HINT}"
                    )
            path = path_of(entry)
            if not isinstance(path, str) or not path:
                continue
            if not is_within(path, prefix):
                foreign = True
                problems.append(
                    f"{section}.{key} points at {path}, outside this project's "
                    f"pixi environment ({prefix}): stale or foreign "
                    f"environment; {RUN_HINT}"
                )
            if not os.path.isfile(path):
                problems.append(f"{section}.{key} binary does not exist: {path}")
            elif not windows and not os.access(path, os.X_OK):
                problems.append(f"{section}.{key} binary is not executable: {path}")
            elif not foreign and key not in settings.get(section, {}):
                problems.append(
                    f"{section}.{key} is a stale generated entry: this "
                    f"environment no longer provides it; {RUN_HINT}"
                )
    return problems


def _language_problems(on_disk: dict, prefix: str, windows: bool) -> list[str]:
    """Problems with formatter commands the generator may have written."""
    problems: list[str] = []
    languages = on_disk.get("languages")
    if not isinstance(languages, dict):
        return problems
    for language, entry in languages.items():
        formatter = entry.get("formatter") if isinstance(entry, dict) else None
        external = formatter.get("external") if isinstance(formatter, dict) else None
        command = external.get("command") if isinstance(external, dict) else None
        if not isinstance(command, str) or not os.path.isabs(command):
            continue
        own_prefix = is_within(command, prefix)
        other_environment = is_environment_dir(os.path.dirname(command))
        if not own_prefix and not other_environment:
            continue
        label = f"languages.{language}.formatter.external.command"
        if not own_prefix:
            problems.append(
                f"{label} points at {command}, outside this project's pixi "
                f"environment ({prefix}): stale or foreign environment; {RUN_HINT}"
            )
        if not os.path.isfile(command):
            problems.append(f"{label} does not exist: {command}")
        elif not windows and not os.access(command, os.X_OK):
            problems.append(f"{label} is not executable: {command}")
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
    root: Path,
    settings: dict,
    tasks: list[dict],
    manifest: Path,
    name: str,
    prefix: str,
    windows: bool,
    quiet: bool,
) -> int:
    problems: list[str] = []
    on_disk = read_json(root / SETTINGS_FILE)

    for section in ("lsp", "dap"):
        for key, fragment in settings.get(section, {}).items():
            actual = on_disk.get(section, {}).get(key)
            if (
                section == "lsp"
                and key in HAND_WRITTEN_WINS
                and isinstance(actual, dict)
                and not is_generated(actual)
            ):
                # The project chose this toolchain by hand; the environment
                # copy is not what Zed runs, so it cannot be stale.
                continue
            if not contains(fragment, actual):
                problems.append(
                    f"{section}.{key} is stale in {SETTINGS_FILE}; {RUN_HINT}"
                )
    terminal_env = on_disk.get("terminal", {}).get("env", {})
    for key, value in settings.get("terminal", {}).get("env", {}).items():
        if terminal_env.get(key) != value:
            problems.append(
                f"terminal.env.{key} is stale in {SETTINGS_FILE}; {RUN_HINT}"
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
                    f"task {task['label']!r} is stale in {TASKS_FILE}; {RUN_HINT}"
                )
    else:
        problems.append(f"{TASKS_FILE} is missing; {RUN_HINT}")

    problems.extend(
        _project_problems(on_disk, settings, root, manifest, name, prefix, windows)
    )
    problems.extend(_language_problems(on_disk, prefix, windows))

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
    parser.add_argument("--environment", default=None, help="pixi environment name")
    parser.add_argument("--pixi", default=None, help="path to the pixi executable")
    parser.add_argument("--check", action="store_true", help="verify instead of writing")
    parser.add_argument("--quiet", action="store_true", help="print only problems")
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    manifest = root / MANIFEST_FILE
    windows = os.name == "nt"

    try:
        if not manifest.is_file():
            raise SyncError(f"no {manifest.name} at {manifest}; nothing to sync")
        pixi = which_pixi(args.pixi)
        info = pixi_info(pixi, manifest)
        verify_project(info, manifest)
        name, prefix = select_environment(info, args.environment)

        existing_settings = read_json(root / SETTINGS_FILE)
        settings = managed_settings(name, prefix, root, manifest, windows, existing_settings)
        tasks = managed_tasks(root, pixi, prefix, windows, pixi_tasks(manifest))

        if args.check:
            return check(
                root, settings, tasks, manifest, name, prefix, windows, args.quiet
            )

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
