# Lean 4 language server through the pixi environment

Measured on this machine on 2026-10-27. Purpose: explain why Zed's Lean 4
extension (id `lean4`, server id `lean4-lsp`, version 0.1.0 /
`blackbird314/zed-lean4@22f5e1e`) reports

    Failed to find lake in PATH or ELAN_HOME/default path. Enable
    lsp.lean4-lsp.settings.elan_auto_install or configure
    lsp.lean4-lsp.binary.path.

and what `scripts/zed_pixi_sync.py` has to write instead. The generator itself
is owned elsewhere; this file records the findings only.

## Minimal interim fix

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
next to itself. A full environment block and the generator that should emit it
are described below.

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

## What a pixi Lean environment contains

A project that declares `lean4` from conda-forge gets a complete standalone
toolchain (verified with pixi 0.81.0 in a scratch project):

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

## The configuration to generate

For a Lean project whose pixi manifest declares `lean4`, with the prefix taken
from `pixi info --json` -> `environments_info[].prefix`:

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
          "CONDA_PREFIX": "<prefix>",
          "PIXI_PROJECT_ROOT": "<project root>",
          "PIXI_PROJECT_MANIFEST": "<project root>/pixi.toml",
          "PIXI_ENVIRONMENT_NAME": "<environment>"
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
- Only emit this entry when `<prefix>/bin/lake` exists, exactly like the other
  language servers in the generator.
- Keep the usual environment block (PATH with the env's `bin` first). It is not
  strictly required for Lake (it locates `lean` itself), but it keeps programs
  started by Lake inside the environment. It also matches the other servers.
- Do **not** invent an `ELAN_HOME`. Nothing in this setup reads it. `ELAN_HOME`
  only matters on the elan route: there `lake` must be `$ELAN_HOME/bin/lake`
  (an elan proxy) and the default is `$HOME/.elan`. Setting `ELAN_HOME` in the
  generated settings cannot fix the extension's own lookup, because step 3
  reads the *shell* environment, not `lsp.*.binary.env`. Leave it unset.
- Do not use `elan_auto_install` for pixi projects: it downloads a second,
  unmanaged toolchain into `$HOME/.elan` and bypasses the pixi env.

## Current state of this repository

`pixi.toml` here (the Zed pixi extension itself) has no `lean4` dependency, so
its environment has no `lean` and no `lake`, `pixi run zed-sync` writes no
`lsp.lean4-lsp` entry, and the generated `.zed/settings.json` cannot fix a Lean
project. The fix belongs to the Lean project's own pixi manifest plus the
generator's detection: `scripts/zed_pixi_sync.py` has to see `lean4` in that
project's environment and add the entry above.

Measured environment here: pixi 0.81.0; this project's env provides
rust-analyzer, basedpyright 1.40.2, taplo, python; prefix from
`pixi info --json`. Nothing system-wide provides `lean`, `lake` or `elan`
(`/usr/bin` has none, no `~/.elan`).
