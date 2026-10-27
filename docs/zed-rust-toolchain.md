# Rust toolchain for Zed dev-extension builds

Zed compiles a dev extension itself. If that step fails with:

```text
ERROR compiling Rust extension: running rustc: No such file or directory (os error 2)
```

then Zed could not find `rustc` on the `PATH` of the Zed process. The
extension source, `extension.toml` and the selected directory are not involved:
Zed stops before it runs `cargo`.

## What Zed does

Before compiling, `crates/extension/src/extension_builder.rs` runs this probe
(`install_rust_wasm_target_if_needed`):

```sh
rustc --print target-libdir --target wasm32-wasip2
```

If the target directory does not exist it runs `rustup target add
wasm32-wasip2`. It then runs, in the extension directory:

```sh
cargo build --target wasm32-wasip2 --target-dir target
```

and loads `target/wasm32-wasip2/debug/zed_pixi.wasm`. A dev install is a debug
build (`CompileExtensionOptions::dev()`), so there is no `--release` flag and
no `release` directory. The `pixi run build` task in this repository builds the
release artifact for packaging, which is a different command.

So the Zed process needs `rustc`, `cargo` and `rustup` on its `PATH`, and its
`rustc` needs the `wasm32-wasip2` target.

## How Zed gets its PATH

On Linux and macOS, when Zed is started from the desktop (its stdout is not a
terminal), it captures the environment of a login and interactive shell run in
`$HOME` and copies it into its own process environment
(`crates/zed/src/main.rs`, `crates/util/src/util.rs:load_login_shell_environment`).
For `bash` that means `~/.profile` and `~/.bashrc` are sourced. When Zed is
started from a terminal it skips that capture and inherits the terminal's
environment.

Zed captures this once, at startup. A toolchain installed while Zed is running
is not picked up until Zed is restarted.

On the Arch/KDE Plasma machine where this was diagnosed, the session's base
`PATH`, the environment that GUI programs start from, was:

```text
/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/var/lib/flatpak/exports/bin:/usr/bin/site_perl:/usr/bin/vendor_perl:/usr/bin/core_perl
```

`/etc/environment` set nothing. No Rust package was installed with pacman
(`pacman -Q rust rustup cargo` found none, and `/var/log/pacman.log` had no
Rust installation), and there was no `rustc` under `/usr`. A `rustup`
toolchain lives in `~/.cargo/bin`, which is on `PATH` only if a shell startup
file or the desktop session adds it.

The reported error therefore means one of these:

1. Rust was never installed with `rustup`.
2. `rustup` was installed with `--no-modify-path`, or its init line was not
   added to the shell files that a login and interactive shell reads, so
   `~/.cargo/bin` never reaches Zed's captured `PATH`.
3. The shell environment capture failed and Zed kept the desktop `PATH`.

All three are fixed by making `rustc` visible to the process that launches
Zed.

## Fix

Run the check script, then restart Zed:

From the repository root:

```sh
bash scripts/zed_rust_setup.sh
```

The script installs `rustup` with a minimal stable toolchain if it is missing,
adds `wasm32-wasip2`, checks Zed's exact `rustc` probe, and writes a Plasma
session environment file if a login shell cannot see `rustc`.

The same steps by hand:

```sh
# 1. Install rustup (skip if `command -v rustup` already prints a path).
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
  sh -s -- -y --profile minimal --default-toolchain stable

# 2. Use it in this shell.
. "$HOME/.cargo/env"

# 3. Add the extension target. Zed adds it too, but doing it here shows errors.
rustup target add wasm32-wasip2

# 4. Run Zed's exact probe. It must print an existing directory, e.g.
#    /home/nandi/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/wasm32-wasip2/lib
rustc --print target-libdir --target wasm32-wasip2
```

Then **fully quit Zed and start it again** (closing the window is not enough
if a Zed process stays alive). In the new process run
**`zed: install dev extension`** and select the directory that contains
`extension.toml` and `Cargo.toml`.

### If Zed still cannot find rustc

`rustc --version` works in a terminal but Zed cannot find it. The shell
environment Zed captured does not contain `~/.cargo/bin`. Choose one:

- Desktop session environment (survives restarts, applies to every GUI app).
  Create `~/.config/plasma-workspace/env/rust.sh`:

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH"
  ```

  Then log out and log in again, or restart the Plasma session.
- Shell startup files. Make sure `~/.profile` or `~/.bashrc` contains the line
  the rustup installer adds:

  ```sh
  . "$HOME/.cargo/env"
  ```

  Zed reads these files when it is started from the desktop.

Check what Zed actually captured: run `zed: open log`, open `Zed.log`, and
look for the line

```text
set environment variables from shell:/usr/bin/bash, path:<PATH>
```

The `path:` value must contain the directory printed by `command -v rustc`.
If the line is missing, the capture failed; launching `zed` from a terminal
lets Zed inherit that terminal's `PATH` as a workaround, and the log line shows
what the desktop session gives it.

### Verify the compile without Zed

From the repository root:

```sh
rustc --version
rustup target list --installed        # must list wasm32-wasip2
cargo build --target wasm32-wasip2 --target-dir target
```

A successful build writes `target/wasm32-wasip2/debug/zed_pixi.wasm`.
`target/` is ignored by git.
