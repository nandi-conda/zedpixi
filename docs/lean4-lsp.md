# Lean 4 language server through the pixi environment

Scope: this repository (`p_58768d1b`) declares no `lean4` dependency on any
branch -- its environment has `rust-analyzer`, `basedpyright`, `taplo` and
`python` only, and no `lean` or `lake`. Nothing below is a claim about this
repository's environment. The pixi toolchain measurements come from a
throwaway pixi project that I created and that declared conda-forge `lean4`;
they say what a *user's own* Lean project gets, nothing else. The rule for any
fix is: the Lean 4 extension must resolve `lake` against the user's own project
pixi environment -- the manifest that actually declares `lean4` -- and not
against this repository's environment or Zed's captured shell PATH.

Measured on this machine on 2026-10-27. Purpose: explain why Zed's Lean 4
extension (id `lean4`, server id `lean4-lsp`, version 0.1.0 /
`owlx56/zed-lean4@90d4b7c376`, the commit published as 0.1.0) reports

    Failed to find lake in PATH or ELAN_HOME/default path. Enable
    lsp.lean4-lsp.settings.elan_auto_install or configure
    lsp.lean4-lsp.binary.path.

and what a Lean project gets: an `lsp.lean4-lsp` entry with `binary.path` at
that project's environment `lake` and `arguments: ["serve", "--"]`.
`scripts/zed_pixi_sync.py` writes that entry when the selected environment of
the project's own pixi manifest installs `<prefix>/bin/lake` (see below); this repository's environment
has no `lean4`, so zed-sync here writes no such entry.

The generator leaves a marker-free hand-written `lean4-lsp` entry alone: it is
kept byte-identical and `--check` skips it, so an elan/`lean-toolchain` project
keeps its toolchain. A generated entry carries `PIXI_PROJECT_ROOT` in its
`env`, is updated when the prefix changes, and is pruned when the environment
no longer installs `lake`. Every `<...>` below is a placeholder for a literal
value; JSON expands no variables, and nothing rewrites the absolute paths of a
hand-written entry after a prefix change.

## What the generator emits

For a project whose own environment installs `<prefix>/bin/lake`, zed-sync
writes `lsp.lean4-lsp` with:

- `binary.path`: `<prefix>/bin/lake`
- `binary.arguments`: `["serve", "--"]`
- `binary.ignore_system_version`: `true`
- `binary.env`: `PATH` (with `<prefix>/bin` first), `VIRTUAL_ENV`,
  `CONDA_PREFIX`, `PIXI_PROJECT_ROOT`, `PIXI_PROJECT_MANIFEST`,
  `PIXI_ENVIRONMENT_NAME`

It writes no `ELAN_HOME` and no `elan_auto_install`. `zed-sync-check` reports
the entry stale when the prefix or environment name changes, and flags a
`binary.path` whose `lake` no longer exists. Because the entry carries the
`PIXI_*` markers, the generator owns it: it is updated on prefix changes and
pruned when the environment stops installing `lake`.

## Manual entry (minimal)

Get the Lake path from the project's own environment:

    pixi run which lake

Then put this in the project's `.zed/settings.json` (`<lake>` is the absolute
path that command prints, for example `<prefix>/bin/lake`):

```json
{
  "lsp": {
    "lean4-lsp": {
      "binary": {
        "path": "<lake>",
        "arguments": ["serve", "--"]
      }
    }
  }
}
```

Both keys matter: `path` must be `lake`, not `lean`, and `arguments` must be
`["serve", "--"]`. With no arguments `lake` prints its usage and exits, so the
server never starts. No `env` block is needed: Lake finds the matching `lean`
next to itself. A full environment block for the manual entry is described
below.

JSON does **not** expand variables or placeholders. A literal `"$PATH"`,
`"$HOME"` or `<prefix>` in `binary.env`/`path` is passed through unchanged and
will break the launch. If you do write `env`, use absolute literal values, or
leave `env` out.

## Why the error happens

The extension resolves `lake` in this order:

1. `lsp.lean4-lsp.binary.path` from project settings;
2. `worktree.which("lake")`, i.e. `lake` on the *Zed worktree shell env* PATH
   (a login + interactive shell captured in the worktree directory);
3. `$ELAN_HOME/bin/lake`, with `ELAN_HOME` from that same shell env, else
   `$HOME/.elan/bin/lake`;
4. `lsp.lean4-lsp.settings.elan_auto_install` (default false), which downloads
   elan into `ELAN_HOME`.

None of 1-3 is true when Lean comes from pixi: `pixi run` prepends
`<prefix>/bin` only to the child process, it never changes the shell PATH that
Zed captures, and no elan is installed. So the extension throws; nothing is
wrong with Lake itself.

## What a project that declares `lean4` gets (scratch test, not this repo)

A project that itself declares `lean4` from conda-forge gets a complete
standalone toolchain (verified with pixi 0.81.0 and re-checked with 0.83.0 in
throwaway projects, not in this repository):

| package | version |
|---|---|
| `lean4` (conda-forge) | 4.33.0 (`4.32.2` also exists) |
| `lake` | real ELF binary at `<prefix>/bin/lake`, reports `Lake version 5.0.0-src+5da8a13 (Lean version 4.33.0)` |
| `lean` | real ELF binary at `<prefix>/bin/lean`, reports `Lean (version 4.33.0, commit 5da8a13c...)` |

- `lake` is **not** an elan proxy. It is a separate native binary.
- There is **no elan** in the environment, and conda-forge has no `elan`
  package. elan is a standalone GitHub release, not a pixi package.
- Both binaries are linked with the env's `lib/lean` through RPATH
  (`$ORIGIN/../lib/lean`), so they are self-contained: `lake` finds its sibling
  `lean` next to itself. Verified: `lake build` and `lake serve --` work with a
  bare environment (`PATH=/usr/bin:/bin`, no pixi bin, no ELAN_HOME).
- `lake` ignores `lean-toolchain` version pins: it always uses the `lean` next
  to itself (tested with `v4.33.0` and `v4.28.0`; both invoked the env's 4.33.0
  `lean`). The pixi dependency is the source of truth for the Lean version.

The extension metadata `lsp.lean4-lsp.binary` is handled by Zed itself: when
`binary.path` is set, Zed starts that path with `binary.arguments` (default: no
arguments) and the environment `worktree.shell_env() + binary.env`. It does not
call the extension's own command recipe. Consequences:

- `arguments` must be written explicitly; with no arguments `lake` prints its
  usage and exits, so no language server starts.
- `binary.env` **is** applied by Zed even though extension 0.1.0 passes
  `env: vec![]` in its own code path.

## Manual configuration (fallback)

Use this when the project does not declare `lean4` in its pixi manifest, or
when it wants a different toolchain (elan, a `lean-toolchain` pin). A
marker-free hand-written entry wins over a generated one, so this is also how
such a project keeps its own toolchain. Add it to the project's
`.zed/settings.json` by hand, with the prefix taken from that project's
`pixi info --json` -> `environments_info[].prefix`:

```json
{
  "lsp": {
    "lean4-lsp": {
      "binary": {
        "path": "<prefix>/bin/lake",
        "arguments": ["serve", "--"],
        "env": {
          "PATH": "<prefix>/bin:<inherited PATH>",
          "VIRTUAL_ENV": "<prefix>",
          "CONDA_PREFIX": "<prefix>"
        }
      }
    }
  }
}
```

Rules:

- Point `path` at `bin/lake`, **not** at `bin/lean`. The server process is
  `lake serve --`; `lean --server` is not what the extension looks for.
- Write `arguments: ["serve", "--"]`. Without them the LSP never starts.
  (`--` separates Lake options from Lean server options; the extension's own
  default recipe uses exactly these two.)
- If the worktree root is a parent of the Lake project, use
  `["--dir", "<project dir>", "serve", "--"]`. Extension 0.1.0 has no
  `lake_project_dir` setting; only `binary.arguments` reaches `lake`.
- Add this entry only when `<prefix>/bin/lake` exists; verify the path with
  `pixi run which lake` in that project. Leave out the `PIXI_*` markers (see
  the environment block below): a marker-free entry is treated as hand-written,
  kept byte-identical and skipped by `--check`, which is what keeps an
  elan/`lean-toolchain` project on its own toolchain. The generated entry is
  the other route: zed-sync writes it when the project's environment installs
  `lake`, updates it on prefix changes, and prunes it when `lake` disappears.
- Derive the prefix from the *selected project's own* `pixi info --json`
  response, never from another manifest. The extension now enforces this
  (commit cf6909c): `project_info.manifest_path` must equal the workspace
  manifest, so a foreign manifest or prefix -- and hence a foreign
  `lean`/`lake` -- is rejected before use. See
  [env-resolution-audit.md](env-resolution-audit.md).
- Keep the environment block from the example (PATH with the env's `bin`
  first). It is not strictly required for Lake (it locates `lean` itself), but
  it keeps programs started by Lake inside the environment and matches what
  `pixi run` sets. Leave out `PIXI_PROJECT_ROOT`, `PIXI_PROJECT_MANIFEST` and
  `PIXI_ENVIRONMENT_NAME`: those markers declare generator ownership, so an
  entry carrying them is updated or pruned by `zed-sync` instead of being left
  alone.
- Do **not** invent an `ELAN_HOME`. Nothing in this setup reads it. `ELAN_HOME`
  only matters on the elan route: there `lake` must be `$ELAN_HOME/bin/lake`
  (an elan proxy) and the default is `$HOME/.elan`. Setting `ELAN_HOME` in
  `lsp.lean4-lsp.binary.env` cannot fix the extension's own lookup, because
  step 3 reads the *shell* environment, not `lsp.*.binary.env`. Leave it
  unset.
- Do not use `elan_auto_install` for pixi projects: it downloads a second,
  unmanaged toolchain into `$HOME/.elan` and bypasses the pixi env.

## Other ways to make the extension find `lake`

The manual entry above is the pixi-native fix. These alternatives cover setups
it does not: an elan-managed toolchain, a prefix that moves, or a project that
already loads its environment through direnv.

### elan

If the project has to follow `lean-toolchain` (Mathlib and most existing Lean
projects), use elan instead of the pixi `lean4` package:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh | sh -s -- -y
```

The installer creates `~/.elan/bin/lake`. The extension finds it in step 3
without any `lean4-lsp` settings, and Lake downloads the toolchain named by
`lean-toolchain`. If elan lives elsewhere, set `ELAN_HOME` in the worktree
shell environment (shell profile, `.envrc`, desktop session), not in
`lsp.lean4-lsp.binary.env`: the extension reads `ELAN_HOME` from
`worktree.shell_env()` before Zed merges `binary.env`.

### Wrapper script through `pixi run`

Use this when the prefix changes (the cache path includes a hash) or a pixi
task must run first. Create an executable script outside the repository:

```sh
#!/bin/sh
exec /usr/bin/pixi --no-progress run --manifest-path /path/to/project/pixi.toml lake "$@"
```

Use the absolute path printed by `command -v pixi`, set it as
`lsp.lean4-lsp.binary.path`, and keep `"arguments": ["serve", "--"]`.
`pixi run` writes nothing to stdout, so the LSP stream stays valid; each launch
re-checks the environment, which is slower than a direct `binary.path`.

### direnv and `pixi shell-hook`

A `.envrc` with `eval "$(pixi shell-hook)"`, with Zed's direnv support enabled,
puts `<prefix>/bin` on the worktree shell environment's `PATH`. Then
`worktree.which("lake")` succeeds and no `lean4-lsp` settings are needed. This
is the mechanism in
[zed-config.md](zed-config.md#alternative-the-project-shell-environment), and
it also helps other tools that only look on `PATH`.

`elan_auto_install` remains a last resort: it installs a second, unmanaged
toolchain into `~/.elan`, outside pixi.

## Current state of this repository (corrected scope)

The answer to "does Zed use the pixi-pinned Lean?" is **no, not by itself**.
`pixi.toml` here (the Zed pixi extension itself) has no `lean4` dependency on
any branch -- only `rust-analyzer`, `basedpyright`, `taplo` and `python` -- so
its environment has no `lean` and no `lake`, and `pixi run zed-sync` here writes
no `lsp.lean4-lsp` entry. Any statement that this repository's environment
provides `lean`/`lake` is wrong: the measured `lean4` pin belongs to a separate
throwaway Lean project's manifest, not to this repository.

A Lean project that does pin `lean4 = "==4.33.0"` still reaches Zed only
through the resolution order above: the extension looks at
`lsp.lean4-lsp.binary.path`, the worktree shell `PATH` and elan, never at the
pixi prefix. `pixi run` only changes `PATH` for the process it starts.

The generator names `lean4-lsp` in its server catalog, but it emits the entry
only from the project's own pixi environment and only when that environment
installs `<prefix>/bin/lake`. This repository's manifest has no `lean4`, so
`pixi run zed-sync` here writes no `lean4-lsp` entry. Settings files stay
machine-specific and are not committed by this repository. `lake` then serves
with the environment's own `lean`; `lean-toolchain` does not override it (see
the measured table above), so the pixi dependency is what the server runs. A
marker-free hand-written entry takes precedence and is left alone.

The rule for any fix is therefore: resolve `lake` against the user's own
project pixi environment -- the manifest that actually declares `lean4` -- never
against this repository's environment or Zed's captured shell PATH. The
resolver has to bind the pixi environment to the workspace's own manifest;
`docs/zed-config.md` describes the same rule for the files it generates.

## Verification

Scratch project `/tmp/leancheck` with `[dependencies] lean4 = "==4.33.0"`
(throwaway, not this repository):

- `pixi install` with pixi 0.83.0 put `lean` and `lake` in the prefix;
  `lean --version` printed 4.33.0, `lake --version` printed
  `5.0.0-src+5da8a13 (Lean version 4.33.0)`, and `env -i PATH=/usr/bin:/bin
  lake --version` printed the same, so the binaries need no pixi `PATH`.
- `python3 scripts/zed_pixi_sync.py` in that project emitted `lsp.lean4-lsp`
  with `binary.path = <prefix>/bin/lake`, `arguments: ["serve", "--"]`,
  `ignore_system_version: true`, and the
  `PATH`/`VIRTUAL_ENV`/`CONDA_PREFIX`/`PIXI_*` env block; `--check` reported it
  current. Starting exactly that path/args/env answered an LSP `initialize`.
- A marker-free hand-written `lsp.lean4-lsp` entry survived `zed-sync`
  byte-identical, and `--check` exited 0. `pixi run zed-sync-test` runs 49
  tests.
- The manual `lsp.lean4-lsp` entry above (`"arguments": ["serve", "--"]`, no
  `ELAN_HOME`) was written into that project's `.zed/settings.json`; the
  generator was not used for it.
- Starting that `lake serve --` with `PATH=/usr/bin:/bin` and sending an LSP
  `initialize` returned `{"serverInfo": {"name": "Lean 4 Server", "version":
  "0.3.0"}}` with `hoverProvider` and the other capabilities, so the manual
  entry starts a working server.
- The wrapper described above, started the same way with `serve --`, also
  answered `initialize` and wrote nothing to stdout besides the LSP frames.
- The host has no `lake`, `lean` or elan (`command -v` finds none, no
  `~/.elan`), so without the entry the extension can only fail.

Measured environment here (my sandbox): pixi 0.81.0; this project's env provides
rust-analyzer, basedpyright 1.40.2, taplo, python; prefix from
`pixi info --json`. Nothing system-wide provides `lean`, `lake` or `elan`
(`/usr/bin` has none, no `~/.elan`).
