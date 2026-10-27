# Zed pixi extension — consolidated status

## 2026-10-27: Lean 4 language server entry in the generator (superseded)

The generated `lean4-lsp` entry and its version pin are gone
(removed in `0c9065b`, "Generate Zed config only from the workspace root's own
pixi env"). `scripts/zed_pixi_sync.py` configures only the servers listed in
[`docs/zed-config.md`](docs/zed-config.md); there is no `lean4` entry and no
version pin. The Lean measurements below came from a throwaway Lean project,
not from this repository's environment. The supported route for a Lean project
is the manual `lsp.lean4-lsp.binary.path = <project env>/bin/lake` recipe with
`arguments: ["serve", "--"]` in [`docs/lean4-lsp.md`](docs/lean4-lsp.md).

The rest of this section is kept for history:

This repository declares no `lean4` dependency on any branch (only
`rust-analyzer`, `basedpyright`, `taplo` and `python`), so its own environment
has no `lean` and no `lake`. The Lean toolchain used to verify the entry below
came from a separate throwaway Lean project, not from this repository.

`scripts/zed_pixi_sync.py` now knows `lean4-lsp`: when the selected
environment contains `<prefix>/bin/lake`, it writes

```json
"lean4-lsp": {"binary": {"path": "<prefix>/bin/lake",
  "arguments": ["serve", "--"], "env": {...}}}
```

That is the only way Zed's built-in `lean4` extension finds a pixi `lean4`
toolchain; it never reads the pixi prefix itself (resolution order and
measurements in `docs/lean4-lsp.md`). Two tests were added: `lake` produces the
entry with `serve --`, and `lean` without `lake` produces none.

Verified: `python3 -m unittest discover -s scripts -p 'test_*.py'` 24/24, and a
scratch `lean4 = "==4.33.0"` project under pixi 0.83.0 generates the entry,
passes `--check`, and the generated `lake serve --` answers an LSP
`initialize` with `Lean 4 Server 0.3.0` under `PATH=/usr/bin:/bin`.

## 2026-10-27: debug adapters and the capability audit

Addition on top of the no-custom-LSP shape (main `d910020`). No language
server, context server or slash command is added.

- `src/dap.rs` and `src/lib.rs`: the extension now implements
  `get_dap_binary`, `dap_request_kind` and `dap_config_to_scenario`.
  `pixi-gdb` (`gdb -i=dap`) and `pixi-lldb` (`lldb-dap`) are resolved with
  `pixi::resolve_tool` and receive `pixi::command_env`, so debug adapters are
  routed without generated settings and with no host fallback. Both are
  stdio adapters; port-based ones (`debugpy`, `dlv dap`) are left to the
  generated `dap.<adapter>` settings because the extension API has no port
  allocator.
- `debug_adapter_schemas/pixi-gdb.json` and `pixi-lldb.json` are required:
  `ExtensionDapAdapter` reads the schema at registration and silently drops an
  adapter whose file is missing.
- `extension.toml`: `[debug_adapters.pixi-gdb]` / `[debug_adapters.pixi-lldb]`,
  plus a capability audit in comments. The audit records that the only
  extension-spawned processes are `pixi info` (absolute path, fixed arguments)
  and the executable test (`/bin/sh -c **`, PowerShell `-Command *`); absolute
  `<prefix>/bin` paths are permitted as the trailing argument of those tests.
  No blanket `command = "*", args = ["**"]` is granted, and `process:exec` does
  not gate Zed-started processes (generated config, debug adapters).
- `pixi-env-check --shadow`: a self-contained check that builds a throwaway
  prefix and host `bin` that both contain `pixi-shadow-tool`, and asserts the
  environment copy is resolved and runs while a host-only tool is not resolved.

Verified: `cargo test --features native-check` 16/16 (11 existing + 5 new DAP
tests), `cargo build --release --target wasm32-wasip2` OK, and
`pixi-env-check --shadow . rust-analyzer taplo` printed
`shadow check passed` with both tools resolved under `<prefix>/bin`.

## 2026-10-27: direction change — no custom language servers

The extension no longer registers any language server (or context server or
slash command). `src/servers.rs` and all `[language_servers.*]` entries are
gone; `src/lib.rs` is a minimal `zed::Extension` and `src/pixi.rs` holds the
strict environment surface (`resolve`, `PixiEnv`, `select_environment`,
`candidate_paths`, `env_bin_dir`, `is_executable`, `command_env`, `merge_env`,
`resolve_tool`/`resolve_tool_with`, with no host-`PATH` fallback). The native
`pixi-lsp-check` binary is replaced by `pixi-env-check` (`pixi run env-check`),
which prints the resolved environment and `PATH` and fails when a requested
tool is not in the environment.

Why: `zed_extension_api` 0.7 exposes no hook to set `PATH` or the environment of
Zed's built-in language servers, debug adapters, tasks or formatters. The
workable route is project configuration (`.zed/settings.json` `lsp.<id>.binary`
with `path`/`env`, `dap`, formatter and linter paths, `.zed/tasks.json` with
`pixi run`). A separate thread owns a generator for those files; this thread
owns `src/*`, `extension.toml`, `pixi.toml` and `README.md`.

Verified in this thread: `cargo test` 11/11, `cargo build --release --target
wasm32-wasip2` OK (215 KB wasm), `pixi run env-check` resolves rust-analyzer,
basedpyright-langserver and taplo from the project environment, prints `python`
as shadowing the host copy, and exits nonzero for a host-only tool.

The sections below describe the earlier custom-language-server design. They are
kept for history; the code and `README.md` now follow the section above.


As of 2026-10-27 13:47 PDT, updated after the main check and the capability fix.
Source of truth: git refs in `.repos/p_58768d1b` and the thread transcripts.

## Main check: main has the work

**Yes. `origin/main` is `39a44d1` and it contains the extension.** The tracked
tree has 13 files: `Cargo.toml`, `Cargo.lock`, `extension.toml`, `pixi.toml`,
`pixi.lock`, `src/lib.rs`, `src/pixi.rs`, `src/servers.rs`,
`src/bin/pixi_lsp_check.rs`, `README.md`, `STATUS.md`, `.gitattributes`,
`.gitignore`. No file under `target/` is tracked. For all of those files except
`STATUS.md`, main's content is byte-identical to `ca055a4`; the report came from
`b7b2bb7`. The `b7db55e`/`ee55437` line on `thread/t_b13a9378` is **not** on main.

One defect was present on main. `src/pixi.rs` runs pixi from the absolute path
returned by `worktree.which("pixi")`, but `extension.toml` granted
`process:exec` for `command = "pixi"`. Zed's `ProcessExecCapability::allows`
compares the command exactly (or `*`), so the call was rejected at runtime.
`ee55437` proposed the fix, and t_cb5730e5 landed it on
`thread/t_cb5730e5`: `command = "*"` with args unchanged. That commit is not on
main yet; it lands with that thread's branch. Running the bare name `pixi`
(letting the host resolve it) would also have fixed it.

Build verification, by t_cb5730e5 in its sandbox with rustup 1.99.1 and
`wasm32-wasip2`, on the `39a44d1` tree: `cargo test` 7/7 passed, `cargo build
--release --target wasm32-wasip2` produced a 344 KB release wasm, and native
discovery found rust-analyzer, basedpyright-langserver and taplo in the pixi
environment. `pixi run test` also passes with the rustup toolchain on `PATH`.

## The public repository

`nandi-conda/zedpixi` on GitHub was created at 20:34 UTC and was empty when this
thread started: no commits, no README, no license. There was no existing
implementation to reuse, so the extension here is written from scratch. The
repository's `main` now mirrors the project's `origin/main`; it does not have
the capability fix until this branch lands.

## Current refs (verified with git)

| ref | commit | content |
|---|---|---|
| `origin/main` | `39a44d1` | extension implementation + strict root-manifest refactor + this report |
| `thread/t_9d1500ac` | `4a5036c` | original extension implementation (+ committed `target/`) |
| `thread/t_4d07a756` | `4a5036c` | monitor thread, no changes |
| `thread/t_b92466fa` | `4a5036c` | status thread, no changes |
| `thread/t_564eba78` | `ca055a4` | `4a5036c` + strict root-manifest env refactor, capabilities, tasks, `.gitignore` |
| `thread/t_b13a9378` | `ee55437` | same feature set, rewritten from `c1b21d2` (does not descend from `4a5036c`); first carried the capability fix |
| `thread/t_cb5730e5` | `665f153` | `f34fa18` + the capability fix (`command = "*"`) |
| `thread/t_bd59ce08` | `87da5fe` | `4a5036c` + capabilities, pixi tasks, `.zed/tasks.json`, `scripts/zed-pixi.sh`, `/target` ignore + untrack |

**Main history.** `origin/main` was `c1b21d2`, then `4a5036c` from 13:31 to
13:37, then reset to `c1b21d2`, then `b7b2bb7` at 13:43, then `39a44d1` at 13:52
(visible in `git reflog show origin/main`). `39a44d1` descends from `4a5036c` and
carries the `ca055a4` source refactor. `b7db55e` and `ee55437` do not contain
`4a5036c`; they are the separate rewrite.

## What works

- `4a5036c` has the real implementation:
  - `src/servers.rs`: 7 server specs with ordered binaries and LSP args
    (rust-analyzer; basedpyright/pyright/pylsp/ruff; taplo; yaml-ls; typescript-ls;
    gopls; clangd) and `pixi add` hints.
  - `src/pixi.rs`: environment resolution via `pixi info --json`, environment
    selection, `<prefix>/bin` candidate paths, executable checks, env construction.
  - `src/lib.rs`: Zed `Extension` impl, language server command, settings
    passthrough, binary/args/env overrides.
  - `src/bin/pixi_lsp_check.rs`: native discovery checker behind `native-check`.
  - 9 unit tests.
- Build/test was verified in scratch copy `/tmp/zbuild` by t_bd59ce08, using
  rustup stable 1.99.1 + `wasm32-wasip2` on the `4a5036c` tree plus the
  `process:exec` capabilities: release wasm build OK, `cargo test` 9/9 OK, and
  native discovery found rust-analyzer, basedpyright-langserver and taplo in the
  installed pixi environment.
- `4a5036c` committed `target/` (2514 files, ~1.26 GiB of blobs). The later
  commits (`87da5fe`, `ca055a4`, `b7db55e`) drop it and ignore `/target`.

## What is blocked, and why

1. **t_9d1500ac kernel is dead (host-side).** Every REPL call fails in ~27 ms:
   `PRIME_AGENT_KERNEL_PYTHON points to a Python missing a current
   prime-agent-runtime ... : /home/nandi/.orchestrate/workers/w1/.team/t_9d1500ac.kernel`.
   That path is not an interpreter; it is a leftover per-thread override from an
   older orchestrate build. t_73de86c6 confirmed the diagnosis and that nothing
   inside the sandbox can change it; t_4d07a756 confirmed that respawn does not
   help. `bash`, `edit` and all programmatic tools are unreachable in that thread.
2. **A second host-side failure hit three worker threads.** t_73de86c6, t_b13a9378,
   t_bd59ce08 and t_564eba78 all ended in
   `prime-agent exited (1): This session is currently open in another Rust build
   of Prime Agent (active in pid 411789 / 413013 / 412128 / 419566)` — a runtime
   session-lease conflict between prime-agent daemons/windows, not a code problem.
   It also cut t_564eba78's turn short while it was mid-refactor.
3. **Build validation is not reproducible in most sandboxes.** No `cargo`/`rustc`/
   `rustup` in the thread sandboxes by default. conda-forge rust 1.99 lacks
   `rust-std-wasm32-wasip2`, so each thread must install rustup with the
   `wasm32-wasip2` target in its own HOME (verified: `cargo build --release
   --target wasm32-wasip2` succeeds on the `4a5036c` tree in ~41 s). There is no
   Zed binary in the sandbox, so actually loading the dev extension is untested.
4. **The refactor is landed and verified; one capability fix is still in flight.**
   `ca055a4`'s content is on main as `39a44d1`, and t_cb5730e5 ran `cargo test`
   7/7, the release wasm build and native discovery against that tree. Main's
   `extension.toml` still grants `command = "pixi"` while the code runs an
   absolute path, which Zed rejects; the fix is `e64dfa7` on
   `thread/t_cb5730e5` and lands with that branch. `87da5fe` (t_bd59ce08) adds
   the capabilities, tasks, helper script and `/target` cleanup but contains no
   build of the refactored code.

## Branch inventory

- **4a5036c** — working baseline, but: reads optional `.zed/pixi.json`, guesses
  `.pixi/envs/<env>` when `pixi` is missing, falls back to servers on `$PATH`,
  has no `[[capabilities]]` (Zed will reject `pixi info`, `/bin/sh -c test -x` and
  PowerShell checks at runtime), no pixi tasks, README refers to `pixi run build`
  that does not exist, and `target/` is committed.
- **ca055a4 (t_564eba78)** — `4a5036c` + strict `<root>/pixi.toml`, explicit error
  when pixi is missing, `pixi info --json --manifest-path`, default/only-env
  selection, env activation (`PATH`, `VIRTUAL_ENV`, `CONDA_PREFIX`, `PIXI_*`), 7
  tests, capabilities (`pixi`, `/bin/sh`, `powershell`), tasks
  `build/check/test/native-check`, `/target` ignored and untracked. Still keeps a
  last-resort `$PATH` lookup for server binaries. No helper script or `.zed/tasks.json`.
- **b7db55e (t_b13a9378)** — independent rewrite with strict root manifest, no
  `$PATH` fallback at all, 12 tests, capabilities, `setup/build/check/test/discover`
  tasks, `/target` ignored, untracked `target/` never present. Conflicts heavily
  with `ca055a4` because it starts from `c1b21d2`.
- **87da5fe (t_bd59ce08)** — `4a5036c` + capabilities, pixi tasks
  (`build/check/test/discover/zed-tasks/zed-status`), `.zed/tasks.json` (101 lines),
  `scripts/zed-pixi.sh` (340 lines: servers/install/add/update/status/tasks/settings),
  `.gitignore` with `/target` and `/extension.wasm`, removes `target/` from the
  index. Best base for the non-source deliverables.

## Next steps, in order

1. **Fix the host-side kernel override (only a human can do it).**
   Repoint `PRIME_AGENT_KERNEL_PYTHON` for this project to
   `/home/nandi/Projects/orchestrate/runner/.pixi/envs/kernel/bin/python` (or
   delete the stale per-thread override at
   `/home/nandi/.orchestrate/workers/w1/.team/t_9d1500ac.kernel`), then restart the
   runner. Alternative: retire t_9d1500ac and open a fresh thread id. Verify with a
   trivial `python_repl` call.
2. **Clear the session-lease conflicts.** Stop the other prime-agent window/build
   or kill the stale holders (`411789`, `412128`, `413013`, `419566`) before
   restarting the failed threads; otherwise they fail again at startup.
3. **Landing is done.** Main has the `ca055a4` line as `39a44d1`, and its build
   and tests are verified. Still open: land `e64dfa7`'s `command = "*"` capability
   fix with t_cb5730e5's branch, decide whether to keep the `$PATH` server
   fallback that main has, and optionally bring over `87da5fe`'s
   `.zed/tasks.json` and `scripts/zed-pixi.sh`.
4. **Remaining validation:** try **Install Dev Extension** in a real Zed and
   enable one server. There is no Zed binary in the sandboxes, so loading the
   extension is still untested.
5. **Land only source, docs, tasks and script; never `target/`.** Check
   `git ls-files target | wc -l` is 0 before landing, and keep `/extension.wasm`
   ignored.
