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

and what `scripts/zed_pixi_sync.py` writes instead: an `lsp.lean4-lsp` entry
with `binary.path` at the environment's `lake` and `arguments: ["serve", "--"]`.
The generator emits it whenever the selected environment contains
`<prefix>/bin/lake`, so a project that runs `pixi add lean4` gets it.

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

## The configuration to generate (in the project that declares `lean4`)

Only for the user's Lean project whose own pixi manifest declares `lean4` --
never for this repository. The prefix is taken from that project's
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

## Other ways to make the extension find `lake`

The generated entry above is the pixi-native fix. These alternatives cover
setups the generator does not: an elan-managed toolchain, a prefix that moves,
or a project that already loads its environment through direnv.

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

`scripts/zed_pixi_sync.py` now emits the entry above when the selected
environment contains `<prefix>/bin/lake`; in the Lean project, where
`pixi add lean4` put the toolchain, that is what makes the server start. That is
the complete fix for a Lean project: `pixi add lean4`, run the generator in
that project, reload Zed. The generated files stay machine-specific and are not
committed by this repository. `lake` then serves with the environment's own
`lean`; `lean-toolchain` does not override it (see the measured table above), so
the pixi dependency is what the server runs.

The rule for any fix is therefore: resolve `lake` against the user's own
project pixi environment -- the manifest that actually declares `lean4` -- never
against this repository's environment or Zed's captured shell PATH. The code
and generator changes are being audited by new threads; this file is background
on the runtime resolution order and the measured behavior of the conda-forge
`lean4` binaries.

## Verification

Scratch project `/tmp/leancheck` with `[dependencies] lean4 = "==4.33.0"`
(throwaway, not this repository):

- `pixi install` with pixi 0.83.0 put `lean` and `lake` in the prefix;
  `lean --version` printed 4.33.0, `lake --version` printed
  `5.0.0-src+5da8a13 (Lean version 4.33.0)`, and `env -i PATH=/usr/bin:/bin
  lake --version` printed the same, so the binaries need no pixi `PATH`.
- `python3 scripts/zed_pixi_sync.py` wrote exactly the `lsp.lean4-lsp` entry
  above (`"arguments": ["serve", "--"]`, no `ELAN_HOME`), and
  `zed-sync-check` accepted the result.
- Starting that `lake serve --` with `PATH=/usr/bin:/bin` and sending an LSP
  `initialize` returned `{"serverInfo": {"name": "Lean 4 Server", "version":
  "0.3.0"}}` with `hoverProvider` and the other capabilities, so the generated
  entry starts a working server.
- The wrapper described above, started the same way with `serve --`, also
  answered `initialize` and wrote nothing to stdout besides the LSP frames.
- The host has no `lake`, `lean` or elan (`command -v` finds none, no
  `~/.elan`), so without the entry the extension can only fail.

Measured environment here (my sandbox): pixi 0.81.0; this project's env provides
rust-analyzer, basedpyright 1.40.2, taplo, python; prefix from
`pixi info --json`. Nothing system-wide provides `lean`, `lake` or `elan`
(`/usr/bin` has none, no `~/.elan`).
