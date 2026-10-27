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

## Building

The extension is a normal Zed Rust extension targeting `wasm32-wasip2`:

```sh
rustup target add wasm32-wasip2
pixi run build          # cargo build --release --target wasm32-wasip2
```

In Zed, run `zed: extensions`, choose **Install Dev Extension**, and select this
directory. Zed builds the extension itself.

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
