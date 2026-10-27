# Troubleshooting a failed `zed: install dev extension`

The toast from **`zed: install dev extension`** shows only the outermost
error. For an extension with Rust code that is usually:

```
Failed to install dev extension: compiling Rust extension
```

Everything below that line (the `cargo` output, a missing target, a missing
`rustup`) is in Zed's log, not in the toast.

## Where the real error is

### The Zed log

| OS | Log file |
| --- | --- |
| Linux | `$XDG_DATA_HOME/zed/logs/Zed.log` (default `~/.local/share/zed/logs/Zed.log`) |
| macOS | `~/Library/Logs/Zed/Zed.log` |
| Windows | `%LOCALAPPDATA%\Zed\logs\Zed.log` |

The previous session is `Zed.log.old` next to it.

In Zed, the command palette has **`zed: open log`** and **`zed: reveal log in
file manager`**. From a terminal:

```sh
tail -f ~/.local/share/zed/logs/Zed.log
```

Then install the extension again and look for the error. Useful filters:

```sh
grep -n -E 'dev extension|compiling Rust|failed to build extension|wasm32-wasip2|running `cargo`|rustup' \
  ~/.local/share/zed/logs/Zed.log
```

A failed build is logged as `ERROR [extensions_ui] Failed to install dev
extension: ...` with the full error chain and the complete `cargo` stderr. For
example, a missing target appears as:

```
failed to build extension error[E0463]: can't find crate for `std`
```

or, when `rustc` rejects the target name:

```
failed to retrieve the `wasm32-wasip2` target libdir: ...
```

### Run Zed from a terminal

This shows the log live and, more importantly, makes Zed inherit the same
`PATH` as your shell:

```sh
cd /path/to/this/repo
zed --foreground
```

While Zed's output goes to a terminal, it writes the log there instead of to
`Zed.log`. Use `ZED_LOG=debug` for debug-level lines:

```sh
ZED_LOG=debug zed --foreground
```

Launching Zed this way is the quickest check for the `cargo is not on Zed's
PATH` case: if the build works from this Zed but not from the desktop
launcher, the launcher's environment is the problem.

### Extension directories

Zed's data directory holds installed and dev extensions:

| OS | Directory |
| --- | --- |
| Linux | `$XDG_DATA_HOME/zed/extensions` (default `~/.local/share/zed/extensions`) |
| macOS | `~/Library/Application Support/Zed/extensions` |
| Windows | `%LOCALAPPDATA%\Zed\extensions` |

On Linux:

```sh
ls -l ~/.local/share/zed/extensions/
```

Relevant entries:

- `<id>` (here `pixi`) is a symlink to the extension's source directory, created
  only after a successful build. A missing or dangling `pixi` symlink means the
  install did not complete.
- `build/` is the extension builder's cache, mainly the downloaded `wasi-sdk`
  used for Tree-sitter grammars. It is **not** the cargo build directory.
- `work/<id>/` is where the compiled `extension.wasm` is unpacked and run.
- `installed/<id>/` holds registry-installed extensions.

A dev extension is compiled **in place, in the directory you selected**. Zed
runs, with that directory as the working directory:

```sh
cargo build --target wasm32-wasip2 [--release] --target-dir <repo>/target
```

Current Zed releases build dev extensions without `--release` (debug), so the
artifact is `target/wasm32-wasip2/debug/zed_pixi.wasm`. After the build Zed
copies the wasm to `<repo>/extension.wasm` and then creates the symlink. There
is no separate copy of the source under `~/.local/share/zed`.

## Reproduce the build by hand

Run these in the repository root, in the same shell you would start Zed from:

```sh
rustc -V
cargo -V
rustup show active-toolchain
rustup target list --installed | grep wasm32-wasip2
rustup target add wasm32-wasip2

cargo build --target wasm32-wasip2            # what current Zed does for a dev extension
cargo build --release --target wasm32-wasip2  # release artifact
```

Expected artifacts:

```sh
ls -l target/wasm32-wasip2/debug/zed_pixi.wasm
ls -l target/wasm32-wasip2/release/zed_pixi.wasm
```

Do not expect `pixi run build` to supply the Rust toolchain. `pixi.toml` in
this repository does not install Rust; the task runs `cargo` from `PATH`. If
`cargo` is missing it fails with:

```
cargo: command not found
```

The same applies to Zed: the extension is built with the toolchain on Zed's
`PATH`, not with anything from a pixi environment.

## Common failures

| What the log says | Cause | Fix |
| --- | --- | --- |
| `running \`cargo\`` with `No such file or directory` | `cargo` is not on the `PATH` of the process that started Zed (common for desktop launches, and for Rust installed after login) | Start Zed from a terminal, or make `~/.cargo/bin` part of the desktop session's `PATH`; check `which cargo` and `zed --foreground` |
| `failed to retrieve the \`wasm32-wasip2\` target libdir: ...` | `rustc` is too old to know the target, or is not a rustup toolchain | `rustup update stable`, then `rustup target add wasm32-wasip2` |
| `the \`wasm32-wasip2\` target is not installed, and \`rustup\` is not available to install it` | Rust is present but `rustup` is not on Zed's `PATH`, or Rust came from Homebrew/conda/Nix | Install Rust with rustup, put `~/.cargo/bin` on the `PATH` that launches Zed, and add the target |
| `failed to install the \`wasm32-wasip2\` target: ...` | `rustup target add` failed: network/proxy problem, or the active toolchain cannot receive it | Run `rustup target add wasm32-wasip2` manually in the same shell and read its error |
| `error[E0463]: can't find crate for \`std\`` | The target is missing from the **active** toolchain (a directory override or `RUSTUP_TOOLCHAIN` may select a different toolchain than the one you added it to) | `rustup show active-toolchain`; `rustup target add wasm32-wasip2 --toolchain <toolchain>` |
| Ordinary Rust compile errors in `src/` | Code or `zed_extension_api` version problem | The full `cargo` diagnostics are in the log; reproduce with the commands above |
| `compiled wasm did not contain a valid zed extension api version` or `extension API version ... is not supported` | The build succeeded, but this Zed is older than the `zed_extension_api` version in `Cargo.toml` | Update Zed |
| Install fails only under a pixi environment | A conda/pixi Rust was used. conda-forge Rust does not ship `wasm32-wasip2` | Use rustup stable on the `PATH`, not the pixi environment's Rust |

## What to include in a bug report

```sh
zed --version
uname -a
rustc -V
cargo -V
rustup show active-toolchain
rustup target list --installed
```

and the matching lines from the Zed log, starting at `compiling Rust crate for
extension`.
