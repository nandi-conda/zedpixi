# Pixi environment for Zed

A [Zed](https://zed.dev) extension that makes a
[pixi](https://pixi.sh) workspace known to Zed.

## What the extension does

When the workspace root contains `pixi.toml`, the extension resolves that
project's pixi environment with:

```sh
pixi info --json --manifest-path <workspace root>/pixi.toml
```

It exposes the environment to the rest of the extension code:

- `PixiEnv { name, prefix, project_root, manifest_path }`
- `command_env()`: the environment block for a process that should run inside
  the environment. `PATH` starts with `<prefix>/bin` (or `<prefix>/Scripts` on
  Windows), followed by the inherited `PATH`; `VIRTUAL_ENV`, `CONDA_PREFIX`,
  `PIXI_PROJECT_ROOT`, `PIXI_PROJECT_MANIFEST` and `PIXI_ENVIRONMENT_NAME` are
  set as well.
- `resolve_tool()`: the absolute path of a tool inside the environment. It is
  strict: the tool must be in the environment, there is no host-`PATH` fallback.

It also declares two debug adapters, `pixi-gdb` and `pixi-lldb`, and starts
them from the environment (see below).

## The extension registers no language servers

Earlier versions of this extension declared `pixi-rust`, `pixi-python`,
`pixi-toml` and other language servers of their own. Those are gone, and
`extension.toml` has no `[language_servers.*]` section. Zed's built-in language
servers (`rust-analyzer`, `basedpyright`, `taplo`, ...), tasks and formatters
are the ones that run.

**Important limitation.** Zed's extension API (including
`zed_extension_api` 0.7/0.8) has no hook that sets the `PATH` or environment of
a built-in language server, task or formatter, or of a debug adapter that
another extension declares. Only commands that an extension provides itself can
be given an environment, and `Worktree::shell_env` is read-only. "Put the pixi
environment on `PATH` for Zed's own servers" therefore cannot be done from the
extension wasm.

The supported way to run Zed's built-ins inside the pixi environment is project
configuration: the `lsp`, `dap`, `terminal` and `formatter` sections of
`.zed/settings.json` and the entries of `.zed/tasks.json`. This extension
supplies the resolved absolute paths and the environment block that such
configuration needs. Tasks can also call `pixi run`, which activates the
environment for the command.

## Debug adapters from the extension

Zed asks an extension for the command of every debug adapter it declares, so
this extension starts two adapters straight from the pixi environment:

| Adapter | Binary and arguments | Languages |
| --- | --- | --- |
| `pixi-gdb` | `gdb -i=dap` | C, C++ |
| `pixi-lldb` | `lldb-dap` | Rust, C, C++ |

Use the adapter name in the debug configuration:

```json
{
  "label": "Debug tests",
  "adapter": "pixi-lldb",
  "request": "launch",
  "program": "${ZED_WORKTREE_ROOT}/target/debug/my-crate"
}
```

Both adapters start with `<prefix>/bin` first on `PATH` and never fall back to
the host `PATH`, so no generated `dap` settings are needed for them. A binary
path set in the debug settings still wins over the environment copy. Both speak
DAP over stdio; port-based adapters (`debugpy`, `dlv dap`) stay on the generated
`dap.<adapter>` settings, because the extension API has no port allocator.
Install the binaries with `pixi add gdb lldb`.

## Using the environment in project settings

To generate these entries from the project environment, run `pixi run zed-sync` (`--check` validates); see [docs/zed-config.md](docs/zed-config.md).

Resolve the environment and get a tool path:

```rust
let env = pixi::resolve(worktree)?;
let taplo = pixi::resolve_tool(&env.prefix, "taplo", windows)?;
let env_vars = pixi::command_env(worktree, &env, windows);
```

The resulting values belong in `.zed/settings.json`. A language server setting
takes an absolute `path` and an `env` object; values are literal strings, so the
`PATH` must be written out in full:

```json
{
  "lsp": {
    "taplo": {
      "binary": {
        "path": "/path/to/.pixi/envs/default/bin/taplo",
        "env": {
          "PATH": "/path/to/.pixi/envs/default/bin:/usr/local/bin:/usr/bin:/bin",
          "VIRTUAL_ENV": "/path/to/.pixi/envs/default",
          "CONDA_PREFIX": "/path/to/.pixi/envs/default",
          "PIXI_PROJECT_ROOT": "/path/to/project",
          "PIXI_PROJECT_MANIFEST": "/path/to/project/pixi.toml",
          "PIXI_ENVIRONMENT_NAME": "default"
        }
      }
    }
  }
}
```

The same shape applies to debug adapters that the extension does not provide,
such as the port-based `debugpy` and `dlv dap` (`"dap"` with `binary`, `args`
and `env`), and to formatters and linters that Zed configures with an explicit
binary path. A task in `.zed/tasks.json` can simply run the tool through pixi:

```json
{
  "label": "test",
  "command": "pixi run test"
}
```

`pixi run` activates the environment, so tools started by the task are the pixi
ones.

### Alternative: a direnv shell hook

Zed runs the project's login shell in the worktree root and loads `.envrc` by
default (`load_direnv: "direct"`). Putting the pixi shell hook there gives the
built-in language servers, tasks and terminals the environment without a
generated settings file:

```sh
# .envrc at the project root
eval "$(pixi shell-hook)"
```

Then run `direnv allow`. Empty variables in `.envrc` are fine; Zed merges the
per-server `binary.env` settings over the shell environment, so an explicit
`lsp.<server>.binary.env` is only needed to override something.

## How the environment is found

The extension always uses `<workspace root>/pixi.toml`. There is no separate
configuration file: open the pixi project directory in Zed.

From that manifest it reads `environments_info`:

- The `default` environment is used when it exists.
- A project with exactly one environment uses that one.
- Otherwise the extension reports an error listing the available environment
  names.

Detached environments work: the prefix is wherever pixi put the environment,
not necessarily `<project>/.pixi/envs`.

If `pixi` is not on `PATH`, the extension reports that and stops; it never
guesses a `.pixi/envs` path. If `<workspace root>/pixi.toml` is missing, the
workspace is simply not a pixi project.

## Environment variables

| Variable | Value |
| --- | --- |
| `PATH` | `<prefix>/bin` (or `<prefix>/Scripts`), then the inherited `PATH` |
| `VIRTUAL_ENV` | the pixi environment prefix |
| `CONDA_PREFIX` | the pixi environment prefix |
| `PIXI_PROJECT_ROOT` | the worktree root |
| `PIXI_PROJECT_MANIFEST` | the absolute path to `pixi.toml` |
| `PIXI_ENVIRONMENT_NAME` | the pixi environment name |

The inherited `PATH` stays after the environment's `bin` directory so basic
system tools remain reachable. It is never used to resolve a project tool:
`resolve_tool` only looks inside the environment.

## Checking the environment and `PATH` without Zed

Build the native check:

```sh
pixi run env-check
# or: cargo run --features native-check --bin pixi-env-check -- [project] [tool...]
```

It prints the project, manifest, environment name, prefix, `bin` directory and
the `PATH` that project settings should carry. With tool arguments, each tool
must resolve inside the environment:

```text
taplo: /prefix/bin/taplo
python: /prefix/bin/python (shadows host /usr/bin/python)
git: NOT IN ENVIRONMENT (host copy at /usr/bin/git is ignored)
```

A tool that exists only on the host `PATH` makes the check exit nonzero. With
no tool arguments it lists the executables in the environment's `bin`
directory.

Add `--shadow` to prove the no-host-fallback rule. It builds a throwaway
environment prefix and a host `bin` directory that both contain
`pixi-shadow-tool`, then checks that the environment copy is resolved and runs
even with the host `bin` first on `PATH`, and that a tool only on the host does
not resolve:

```sh
pixi run env-check --shadow
# or: cargo run --features native-check --bin pixi-env-check -- --shadow .
```

## Installing in Zed

The extension is not published in Zed's registry, so install it as a dev
extension.

Prerequisites:

1. Rust installed with `rustup`, and the WebAssembly target added:

   ```sh
   rustup target add wasm32-wasip2
   ```

   Zed builds extensions with `wasm32-wasip2` and adds the target itself when
   Rust comes from `rustup`. Rust from conda or Homebrew is not enough unless
   that toolchain also provides the target.

2. `pixi` on the `PATH` of the process that launches Zed. On macOS, Zed started
   from the Dock may not see `~/.pixi/bin`; check by running `which pixi`, or
   launch Zed from a terminal once to test.

Steps:

1. In the command palette, run **`zed: install dev extension`**. The same button
   is in the top right of the **`zed: extensions`** page.
2. Select this repository directory (the one containing `extension.toml` and
   `Cargo.toml`). Zed compiles it with `cargo build --target wasm32-wasip2`:
   a dev install uses the debug profile (`--release` is not passed) and loads
   `target/wasm32-wasip2/debug/zed_pixi.wasm`, then copies it to
   `extension.wasm` in the same directory. If the install fails with
   "compiling Rust extension", see
   [docs/zed-install-troubleshooting.md](docs/zed-install-troubleshooting.md).
3. Open the pixi project directory itself in Zed: the workspace root must
   contain `pixi.toml`.
4. Point the language servers, debug adapters, tasks and formatters you use at
   that environment, as shown above. `pixi run env-check` prints the values.

### Process capabilities

The extension spawns exactly two kinds of process, and `extension.toml` grants
only those:

1. `pixi info --json --manifest-path <workspace>/pixi.toml`, to resolve the
   environment. `pixi` is run through the absolute path that
   `worktree.which("pixi")` returns, so the capability uses `command = "*"` with
   the arguments fixed to that one query.
2. The executable test for a candidate path in the environment:
   `/bin/sh -c 'test -x "$1"' sh <absolute path>`, and PowerShell's
   `Test-Path -LiteralPath <absolute path>` on Windows. Absolute `<prefix>/bin`
   paths are permitted only as the trailing argument of that test.

No blanket `command = "*", args = ["**"]` grant is given or needed. The
extension never starts an environment binary itself; Zed starts language
servers, formatters, linters and tasks from project settings and starts the
declared debug adapters through `get_dap_binary`, and `process:exec` gates
neither of those paths.

`process:exec` capabilities match the command string exactly. A checkout that
says `command = "pixi"` gets the `info` call rejected by Zed at runtime; keep
the `*` version.

## Building

To build a release wasm artifact by hand (a dev install compiles the debug
profile itself, as described above):

```sh
rustup target add wasm32-wasip2
pixi run build          # cargo build --release --target wasm32-wasip2
```

The Rust toolchain is not part of the pixi environment: conda-forge ships Rust
1.99 but no `rust-std-wasm32-wasip2` package, so `rustup` (or another toolchain
with the `wasm32-wasip2` target) is needed.

## Tests

```sh
pixi run test           # unit tests for environment selection and path handling
pixi run check
```

The unit tests cover the parts that do not need the Zed host: `pixi info --json`
environment selection, candidate paths, `PATH` composition, environment
merging, and strict tool resolution including the no-host-fallback case.

## Notes and limitations

- Zed extensions cannot list directories. The extension checks each candidate
  path with the platform's executable test (`/bin/sh -c 'test -x'`, or
  `Test-Path` on Windows) instead of enumerating `<prefix>/bin`. Some tools,
  such as `basedpyright-langserver`, do not survive a `--version` probe.
- The extension does not download or install tools or language servers. Install
  them with `pixi add` so the environment stays reproducible and shared by
  everyone working on the project.
- The project's own `pixi.toml` includes `rust-analyzer`, `basedpyright` and
  `taplo` to exercise environment resolution while developing the extension.
