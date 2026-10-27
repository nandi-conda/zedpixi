# Lean 4 in Zed: `lean4-lsp` cannot find `lake`

Symptom, from the **Lean 4** extension (id `lean4`, version 0.1.0) in Zed:

```text
Failed to find lake in PATH or ELAN_HOME/default path. Enable
lsp.lean4-lsp.settings.elan_auto_install or configure
lsp.lean4-lsp.binary.path.
```

`pixi run lake` works in a terminal at the same time. The two facts are
consistent: `pixi run` activates the environment for that one process, and Zed
never runs it. This page explains the search the extension performs and the
fixes, in order of preference.

## What the extension does

The language server is not its own program. The extension starts `lake serve --`
and lets Lake run Lean's LSP server. Its command search, from
`owlx56/zed-lean4`, `src/lean4.rs`, is:

1. `lsp.lean4-lsp.binary.path`, when set. It is used as-is; `binary.arguments`
   replaces the extension's default arguments.
2. `worktree.which("lake")`, i.e. `lake` on the worktree shell environment's
   `PATH`.
3. `$ELAN_HOME/bin/lake` from the worktree shell environment, or
   `$HOME/.elan/bin/lake` when `ELAN_HOME` is not set.
4. If none exists, the error above; with
   `lsp.lean4-lsp.settings.elan_auto_install = true`, the extension downloads
   `elan-init` and installs the default toolchain into `$HOME/.elan` instead.

The extension has no pixi-specific code. It never reads `pixi.toml`, never runs
`pixi`, and never looks in `.pixi` or the pixi cache directory.

## How Zed builds the environment the extension sees

- At startup on Linux and macOS, Zed captures the environment of a **login and
  interactive** shell (`$SHELL -l -i -c ...`) and copies it into its own
  process environment (`crates/zed/src/main.rs`,
  `load_login_shell_environment`). On a desktop launch this replaces the small
  `PATH` the session manager provided.
- For each worktree, Zed spawns the user's shell in the **worktree directory**
  (`crates/project/src/environment.rs`, `worktree_environment`) and caches the
  result. That is the worktree shell environment. Running it in the project
  directory is what lets `direnv` and `pixi shell-hook` add project paths.
- `worktree.which(name)` resolves with `which::which_in(name, env.PATH,
  worktree_root)` against that shell environment
  (`crates/project/src/lsp_store.rs`, `LocalLspAdapterDelegate::which`). It does
  not use the process `PATH` and does not search any tool manager's registry.
- The language server process then gets `shell_env`, extended by the
  extension's command environment and by `lsp.<id>.binary.env`, with the
  worktree root as its working directory
  (`language_server_configuration` in the same file).

So a language server sees exactly the environment a fresh shell in the project
directory would see, plus anything configured under `lsp.<id>.binary.env`.
`pixi run` is not part of it.

## Why the pixi environment's `lake` never appears

`pixi run <command>` and `pixi shell-hook` prepend
`<prefix>/bin` (`<prefix>/Scripts` on Windows) to `PATH` for the process they
start. `pixi add` installs tools into that prefix; it does not create a shim in
`/usr/bin` and does not change the login shell. Nothing the shell in the
worktree inherits from the desktop session mentions the prefix, so:

- `worktree.which("lake")` searches a `PATH` with no pixi entry and finds
  nothing;
- `$ELAN_HOME`/`$HOME/.elan` has no `lake` when Lean is managed by pixi;
- the extension reaches its error.

If the project's `pixi.toml` does not install Lean, the error is simply
correct: there is no `lake` to find anywhere. In this repository, for example,
the environment contains only `rust-analyzer`, `basedpyright`, `taplo` and
`python`; `<prefix>/bin` has no `lake`, and `pixi run lake` fails with
`lake: command not found`. A working `pixi run lake` means the Lean project's
environment (or a pixi task) installs a Lean toolchain, for example

```toml
[dependencies]
lean4 = ">=4.33,<4.34"
```

`pixi run lake --version` then prints the Lake and Lean versions.

## Fixes, in order of preference

### 1. Use elan (recommended for Lean projects)

Lean projects pin their compiler in `lean-toolchain`, and Mathlib and other
dependencies expect that exact toolchain. elan is the tool that manages those
toolchains, and the extension is built around it:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh | sh -s -- -y
```

The installer creates `~/.elan/bin/lake`, which the extension finds in step 3
above. No Zed settings are needed, and `lake` downloads the toolchain named by
`lean-toolchain` on first use. Keep pixi for the rest of the toolchain if you
like; the two do not conflict.

If elan lives outside `$HOME/.elan`, make `ELAN_HOME` visible to the **worktree
shell environment** (shell profile, `.envrc`, or the session environment). An
`ELAN_HOME` entry in `lsp.lean4-lsp.binary.env` is too late: the extension
already read `shell_env` by then.

### 2. Point `binary.path` at the pixi environment's `lake`

This keeps Lean inside the pixi environment, but ties the project to the one
Lean version the environment provides (conda-forge currently ships Lean 4.33).
Resolve the prefix with pixi:

```sh
pixi info --json | jq -r '.environments_info[].prefix'
# -> /path/to/cache/envs/<project>-<hash>/envs/default
```

Then in `.zed/settings.json`:

```json
{
  "lsp": {
    "lean4-lsp": {
      "binary": {
        "path": "<prefix>/bin/lake",
        "arguments": ["serve", "--"],
        "env": {
          "PATH": "<prefix>/bin:<rest of PATH>"
        }
      }
    }
  }
}
```

Two details matter:

- When `binary.path` is set, the extension returns early and its own
  `serve --` arguments are **not** added. Without `"arguments"` the server is
  started as a bare `lake` and Zed's LSP handshake fails.
- `arguments` and `env` are applied by Zed to the command the extension
  returns, so this is the supported way to add the environment's `bin`
  directory for anything Lake spawns. The path must be absolute and executable.

If the Zed worktree is a parent of the Lean project, also set
`lsp.lean4-lsp.settings.lake_project_dir` (relative to the worktree root) so the
extension starts `lake --dir <project> serve --`. With `binary.path` set, put
`"--dir", "<absolute project dir>"` before `"serve", "--"` in `arguments`
instead.

### 3. Use a wrapper script that runs lake through pixi

Useful when the prefix moves (the cache directory includes a hash) or when the
project expects a pixi task to set more state. Create an executable script
outside the repository, for example `~/.local/bin/lean4-lsp-pixi`:

```sh
#!/bin/sh
exec /usr/bin/pixi --no-progress run --manifest-path /path/to/project/pixi.toml lake "$@"
```

Use the absolute path that `command -v pixi` prints. Then:

```json
{
  "lsp": {
    "lean4-lsp": {
      "binary": {
        "path": "/home/you/.local/bin/lean4-lsp-pixi",
        "arguments": ["serve", "--"]
      }
    }
  }
}
```

`pixi run` is quiet on stdout, so it does not corrupt the LSP stream. It does
re-check the environment on each launch, which is slower than options 1 and 2.

### 4. `elan_auto_install`, or a direnv shell environment

- `lsp.lean4-lsp.settings.elan_auto_install: true` makes the extension install
  elan and a stable toolchain into `$HOME/.elan` when it finds no `lake`. It
  needs network access and puts Lean outside pixi, which contradicts a
  pixi-only setup; prefer option 1 when you control the machine.
- A `.envrc` with `eval "$(pixi shell-hook)"` (and Zed's direnv support
  enabled) puts `<prefix>/bin` on the worktree shell environment's `PATH`. Then
  `worktree.which("lake")` succeeds without any `lean4-lsp` settings. This also
  helps every other Zed tool that looks on `PATH`, and is the mechanism
  described in [zed-config.md](zed-config.md#alternative-the-project-shell-environment).

The pixi extension itself cannot fix this. It registers no language servers,
and Zed's extension API has no hook that sets the environment of a built-in or
another extension's language server (see the main README). Its environment
resolution (`pixi info --json --manifest-path`) feeds the generated
`.zed/settings.json`, tasks and the extension's own debug adapters; it cannot
inject `PATH` into a server that the Lean 4 extension declares. The earlier
`/pixi-env` assistant command of this extension only printed the resolved
environment and has been removed.

## What was verified

In a scratch pixi project with `lean4 = ">=4.33,<4.34"`:

- the environment prefix contains `bin/lake`, `bin/lean` and the rest of the
  toolchain; `pixi run lake --version` printed
  `Lake version 5.0.0-src+5da8a13 (Lean version 4.33.0)`;
- an interactive login shell in the project directory had no `lake` on `PATH`,
  which is what the extension's `worktree.which("lake")` searches;
- `<prefix>/bin/lake serve --`, started in the project directory with a bare
  `PATH` of `/usr/bin:/bin`, answered an LSP `initialize` request. Option 2
  therefore works without `binary.env`; adding `PATH` is still the safe choice
  for tools Lake starts;
- the wrapper from option 3, started the same way, also answered `initialize`
  and wrote nothing to stdout besides the LSP frames.

## See also

- [Running Zed's built-in tools from the pixi environment](zed-config.md)
- [Troubleshooting a failed Zed dev-extension install](zed-install-troubleshooting.md)
