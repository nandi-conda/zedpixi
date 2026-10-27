# Pixi language servers for Zed

A [Zed](https://zed.dev) extension that runs language servers from the
[pixi](https://pixi.sh) environment of the project you have open.

Instead of installing a separate copy of every language server, add it to your
project's `pixi.toml`:

```sh
pixi add rust-analyzer taplo basedpyright
```

The extension finds the environment with `pixi info --json` (so detached pixi
environments work too), picks the first matching server binary that runs, and
starts it with the environment's `bin` directory first on `PATH`.

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

## Choosing the environment

By default the extension uses the `default` pixi environment of the worktree
root (`pixi.toml`). Put a `.zed/pixi.json` file in the project to change that:

```json
{
  "manifest_path": "python/pixi.toml",
  "environment": "test"
}
```

- `manifest_path` is relative to the worktree root (an absolute path also works).
- `environment` is a pixi environment name; omit it for `default`.

The extension reads the environment prefix from `pixi info --json`. If the
`pixi` CLI is not on `PATH`, it falls back to `.pixi/envs/<environment>`, which
only exists for projects that keep environments inside the project.

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
pixi run test           # unit tests for config and environment parsing
pixi run check
```

The unit tests cover the parts that do not need the Zed host: `.zed/pixi.json`
parsing, `pixi info --json` environment selection, binary path construction, and
environment merging.

## Notes and limitations

- Zed extensions cannot list directories. The extension therefore probes each
  candidate path by running it with `--version` instead of enumerating
  `<prefix>/bin`.
- Only the servers listed in `src/servers.rs` are provided. Add a row there and
  a `[language_servers.*]` entry in `extension.toml` to support another server.
- The extension does not download or install language servers. Install them with
  pixi so the environment stays reproducible and shared by everyone working on
  the project.
- The project's own `pixi.toml` includes `rust-analyzer`, `basedpyright`, and
  `taplo` to exercise the extension while developing it.
