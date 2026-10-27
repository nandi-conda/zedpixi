# Where the extension and zed-sync may resolve an environment

Audit of every place the extension (this repository's code) and the Zed
configuration generator can pick up an environment or a toolchain. The rule
they must all follow:

> The only source is `pixi info --json` for `<workspace root>/pixi.toml`.
> Never a parent or foreign manifest, an ambient pixi/Conda environment, the
> host `PATH`, `ELAN_HOME`, or a hardcoded toolchain pin.

This repository declares no Lean toolchain: `pixi.toml` has `rust-analyzer`,
`basedpyright`, `taplo` and `python` only, and no `lean`/`lake`. A
`lean4 = "==4.33.0"` pin belongs to a *different* Lean project's manifest, so
nothing in this repository may pull that toolchain in.

## Resolution chain (extension)

1. `src/pixi.rs::resolve` uses `worktree.root_path()` and requires
   `<root>/pixi.toml` to exist. There is no parent-directory search: a
   workspace whose root has no manifest is simply not a pixi project.
2. `pixi` is found with `worktree.which("pixi")` (the CLI, not a toolchain) and
   run as `pixi info --json --manifest-path <root>/pixi.toml`.
3. `parse_info` -> `verify_project` rejects any response whose
   `project_info.manifest_path` is not that same manifest. This is what stops
   a response for another project (or an ambient environment) from being used.
4. `select_environment` picks `default` (or the only environment) from that
   response only. Environment names and the prefix come from pixi.
5. `resolve_tool`/`resolve_tool_with` look only at `<prefix>/bin` (or
   `<prefix>/Scripts` on Windows). There is no host-`PATH` lookup, and no
   toolchain is inferred from dependencies, `lean-toolchain` or version pins.

`src/lib.rs` uses that chain for `get_dap_binary`; an explicit
`user_provided_debug_adapter_path` from the debug settings wins, otherwise the
adapter must exist in the environment. `src/bin/pixi_env_check.rs` (the native
`pixi-env-check`) goes through `parse_info` too, so the check fails on a
foreign response.

`src/pixi.rs` never reads `ELAN_HOME`, `CONDA_PREFIX`, `VIRTUAL_ENV` or
`PIXI_*` to find anything. `command_env` writes `VIRTUAL_ENV`,
`CONDA_PREFIX`, `PIXI_PROJECT_ROOT`, `PIXI_PROJECT_MANIFEST` and
`PIXI_ENVIRONMENT_NAME` for the child process, and puts `<prefix>/bin` before
the inherited `PATH` so that programs spawned by a server or debuggee still
find basic system tools. The inherited `PATH` is never used to resolve a
project tool.

Ambient environment variables do not change the result: `pixi info` with an
explicit `--manifest-path` ignores `PIXI_PROJECT_ROOT`,
`PIXI_PROJECT_MANIFEST` and `PIXI_ENVIRONMENT_NAME` (measured with pixi 0.83.0:
the reported `project_info.manifest_path` stayed the workspace manifest in all
three cases).

pixi reports the *canonical* manifest path, so a workspace opened through a
symbolic link has to be opened by its real path (Zed canonicalizes worktree
roots). `pixi-env-check` canonicalizes the manifest itself before comparing.
The wasm extension has no filesystem access to resolve symlinks, so it
compares the normalized paths it has.

## Regression tests

`cargo test` (in `src/pixi.rs`):

- `accepts_a_response_for_the_workspace_manifest`
- `rejects_a_foreign_project_manifest` -- a valid response for
  `/tmp/leancheck/pixi.toml` is refused for `/home/me/project/pixi.toml`.
- `rejects_a_response_without_a_project_manifest`
- `a_foreign_lean_pin_cannot_leak_in` -- another project's `lean4` pin and its
  `<prefix>/bin/lake` are not used by this workspace, which has no `lake`.
- `does_not_fall_back_to_the_host_path` (existing)
- `compares_paths_the_way_pixi_reports_them`

## zed-sync (`scripts/zed_pixi_sync.py`) -- findings for its owner

Not changed here; reported to the generator thread.

- `--manifest PATH` allowed a manifest outside `--root`. The generated `PIXI_*`
  block then mixed this project's root with another project's environment. Rule:
  the manifest is always `<root>/pixi.toml`.
- `select_environment` did not check `project_info.manifest_path`, so the
  generator had no equivalent of `verify_project`. The extension's rule should
  be mirrored so `zed-sync --check` reports a foreign response instead of
  writing it.
- `environment_vars` copied the caller's whole `PATH` after `<prefix>/bin`;
  entries of another pixi/Conda environment there are reachable by programs the
  server spawns. Filtering foreign env `bin` directories is stricter.
- The generator contains no hardcoded toolchain pin. It only maps tools that
  exist in the selected environment's prefix, so a Lean entry (if the catalog
  keeps one -- the generator owner decides that separately) can only come from
  `<prefix>/bin/lake` of the project's own manifest. The rule holds once the
  manifest identity is checked. The `==4.33.0` pin came from a scratch Lean
  project used only to measure the toolchain; it must not be presented as this
  repository's environment (see `STATUS.md` and `docs/lean4-lsp.md`).

The generator owner reported these implemented on thread/t_92339e0a
(manifest flag removed, `verify_project` mirrored, PATH filtered, activation
variables cleared for the `pixi` call).

## Ownership

`src/*` was audited and fixed in this thread. `scripts/*` and
`docs/zed-config.md` belong to the zed-sync generator thread;
`docs/lean4-lsp.md` belongs to the Lean 4 diagnosis thread. Findings were sent
to them; this document records what was checked.
