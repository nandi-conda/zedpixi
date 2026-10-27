# Pixi language servers for Zed

A [Zed](https://zed.dev) extension that runs language servers from the
[pixi](https://pixi.sh) environment of the project you have open.

Instead of installing a separate copy of every language server, add it to your
project's `pixi.toml`:

```sh
pixi add rust-analyzer taplo basedpyright
```

The extension uses the project the workspace opens: the `pixi.toml` at the
workspace root. It asks `pixi info --json --manifest-path` for that manifest,
picks the first matching server binary that exists, and starts it with the
environment's `bin` directory first on `PATH`.

## Supported servers

| Zed language server id | Languages | Binaries, in preference order | Install with |
| --- | --- | --- | --- |
| `pixi-rust` | Rust | `rust-analyzer` | `pixi add rust-analyzer` |
| `pixi-python` | Python | `basedpyright-langserver`, `pyright-langserver`, `pylsp`, `ruff server` | `pixi add basedpyright` |
| `pixi-toml` | TOML | `taplo lsp stdio` | `pixi add taplo` |
| `pixi-yaml` | YAML | `yaml-language-server --stdio` | `pixi add yaml-language-server` |
| `pixi-typescript` | TypeScript, JavaScript | `typescript-language-server --stdio` | `pixi add typescript-language-server` |
| `pixi-go` | Go | `gopls` | `pixi add gopls` |
| `pixi-cpp` | C, C++ | `clangd` | `pixi add clangd` |

Every server is declared with `opt_in_languages`, so it never starts next to a
built-in Zed server or a server from another extension. Enable only the ones you
want, in your Zed settings:

```json
{
  "languages": {
    "Rust": { "language_servers": ["pixi-rust"] },
    "Python": { "language_servers": ["pixi-python"] },
    "TOML": { "language_servers": ["pixi-toml"] }
  }
}
```

Listing only the pixi server replaces the default server for that language. Add
`"..."` to the list if you want to keep the default server as well.

## How the environment is found

The extension always uses `<workspace root>/pixi.toml`. There is no separate
configuration file: open the pixi project directory in Zed.

From that manifest it runs:

```sh
pixi info --json --manifest-path <workspace root>/pixi.toml
```

and reads the environment prefix from `environments_info`. Detached
environments therefore work: the prefix is wherever pixi put the environment,
not necessarily `<project>/.pixi/envs`.

The `default` environment is used when it exists. A project with exactly one
environment uses that one. Otherwise the extension reports an error listing the
available environment names.

If `pixi` is not on `PATH`, the extension reports that and stops; it never
guesses a `.pixi/envs` path. If `<workspace root>/pixi.toml` is missing, it says
so and asks you to open the project directory itself.

## Overriding a server binary

The standard Zed LSP settings work per server id:

```json
{
  "lsp": {
    "pixi-python": {
      "binary": {
        "path": "/opt/custom/basedpyright-langserver",
        "arguments": ["--stdio"],
        "env": { "PYRIGHT_PYTHON_FORCE_VERSION": "latest" }
      },
      "initialization_options": {},
      "settings": {}
    }
  }
}
```

- `binary.path` skips discovery completely.
- `binary.arguments` replaces the default arguments when set.
- `binary.env` is merged over the environment variables the extension sets.
- `initialization_options` and `settings` are passed through to the server.

If no pixi server binary is found, the extension falls back to the same binary on
`$PATH`. Otherwise Zed reports which paths were checked and which `pixi add`
command installs the server.

## Environment variables passed to servers

| Variable | Value |
| --- | --- |
| `PATH` | `<prefix>/bin` (or `<prefix>/Scripts`), then the shell `PATH` |
| `VIRTUAL_ENV` | the pixi environment prefix |
| `CONDA_PREFIX` | the pixi environment prefix |
| `PIXI_PROJECT_ROOT` | the worktree root |
| `PIXI_PROJECT_MANIFEST` | the absolute path to `pixi.toml` |
| `PIXI_ENVIRONMENT_NAME` | the pixi environment name |

This makes Python and Node based servers use the interpreters from the pixi
environment.

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

2. `pixi` on the `PATH` of the process that launches Zed. The extension reports
   a missing `pixi` and stops. On macOS, Zed started from the Dock may not see
   `~/.pixi/bin`; check by running `which pixi`, or launch Zed from a terminal
   once to test.

Steps:

1. In the command palette, run **`zed: install dev extension`**. The same button
   is in the top right of the **`zed: extensions`** page.
2. Select this repository directory (the one containing `extension.toml` and
   `Cargo.toml`). Zed compiles it with `cargo build --release --target
   wasm32-wasip2` and loads `target/wasm32-wasip2/release/zed_pixi.wasm`. You do
   not run `cargo` yourself.
3. The Extensions page lists Pixi as `DEV`.
4. Every server is opt-in. Enable only the ones you want in Zed settings:

   ```json
   {
     "languages": {
       "Rust": { "language_servers": ["pixi-rust"] },
       "Python": { "language_servers": ["pixi-python"] },
       "TOML": { "language_servers": ["pixi-toml"] }
     }
   }
   ```

   Listing only the pixi server replaces the default server for that language.
   Add `"..."` to the list to keep the default server as well.
5. Open the pixi project directory itself in Zed: the workspace root must
   contain `pixi.toml`. Then install the servers into that environment, for
   example `pixi add rust-analyzer basedpyright taplo`.

To see errors, run `zed: open log`; for verbose output, start Zed from a
terminal with `zed --foreground`. After changing the extension source, run
`zed: install dev extension` again so Zed rebuilds it.

One known trap: `process:exec` capabilities match the command string exactly.
The extension runs `pixi` through the absolute path returned by
`worktree.which("pixi")`, so `extension.toml` grants `command = "*"` for
`info --json --manifest-path *`. A checkout that still says `command = "pixi"`
gets that call rejected by Zed at runtime; keep the `*` version.

## Building

To build the same wasm artifact by hand:

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
environment selection, binary path construction, and environment merging.

With a pixi project installed, `pixi run native-check` runs the same discovery
as the extension natively and prints the environment prefix and each server it
finds or misses.

## Notes and limitations

- Zed extensions cannot list directories. The extension therefore checks each
  candidate path with the platform's executable test (`/bin/sh -c 'test -x'`, or
  `Test-Path` on Windows) instead of enumerating `<prefix>/bin`. Some servers,
  such as `basedpyright-langserver`, do not survive a `--version` probe.
- Only the servers listed in `src/servers.rs` are provided. Add a row there and
  a `[language_servers.*]` entry in `extension.toml` to support another server.
- The extension does not download or install language servers. Install them with
  pixi so the environment stays reproducible and shared by everyone working on
  the project.
- The project's own `pixi.toml` includes `rust-analyzer`, `basedpyright`, and
  `taplo` to exercise the extension while developing it.
