#!/usr/bin/env bash
# Check and repair the Rust toolchain Zed needs to build this dev extension.
#
# Zed compiles a dev extension itself and needs `rustc`, `cargo` and `rustup`
# on the PATH of the Zed process, plus a toolchain with the wasm32-wasip2
# target. See docs/zed-rust-toolchain.md.
#
# Usage: bash scripts/zed_rust_setup.sh
# Run it from a terminal, then fully quit and relaunch Zed.
set -euo pipefail

target=wasm32-wasip2
cargo_bin="$HOME/.cargo/bin"
original_path=$PATH
plasma_env_dir="$HOME/.config/plasma-workspace/env"
plasma_env_file="$plasma_env_dir/rust.sh"

say() { printf '%s\n' "$*"; }
report() { printf '  %-7s %s\n' "$1" "$2"; }

say "Rust toolchain for the Zed dev extension"
say
say "PATH: $PATH"
for tool in rustc cargo rustup; do
    if path=$(command -v "$tool" 2>/dev/null); then
        report "$tool" "$path"
    elif [ -x "$cargo_bin/$tool" ]; then
        report "$tool" "$cargo_bin/$tool (not on PATH)"
    else
        report "$tool" "MISSING"
    fi
done

# 1. Install rustup if it is not available anywhere.
if ! command -v rustup >/dev/null 2>&1 && [ ! -x "$cargo_bin/rustup" ]; then
    say
    say "Installing rustup with a minimal stable toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
        sh -s -- -y --profile minimal --default-toolchain stable
fi

# Make ~/.cargo/bin usable in this script even if the rc files were not read.
case ":$PATH:" in
    *":$cargo_bin:"*) ;;
    *) PATH="$cargo_bin:$PATH" ;;
esac
export PATH
if ! command -v rustc >/dev/null 2>&1; then
    say
    say "rustc is still not runnable after the install; stop here." >&2
    exit 1
fi

# 2. Add the target Zed uses.
if rustup target list --installed | grep -qx "$target"; then
    say
    say "$target is installed."
else
    say
    say "Adding the $target target..."
    rustup target add "$target"
fi

# 3. Check Zed's exact probe.
libdir=$(rustc --print target-libdir --target "$target")
say
say "rustc --print target-libdir --target $target"
report "" "$libdir"
if [ ! -d "$libdir" ]; then
    say "error: Zed's probe returned a directory that does not exist" >&2
    exit 1
fi
report "rustc" "$(rustc --version)"

# 4. Make the toolchain visible to GUI-launched Zed on KDE Plasma.
# Zed copies the environment of a login and interactive shell at startup. Test
# that shell with the desktop session's PATH, not this terminal's, because a
# terminal often has a richer PATH than the process that launches Zed.
shell_path=$(getent passwd "$(id -un)" | cut -d: -f7)
shell_path=${shell_path:-/usr/bin/bash}
session_path=$(systemctl --user show-environment 2>/dev/null | sed -n 's/^PATH=//p' || true)
if [ -n "$session_path" ]; then
    probe_path=$session_path
else
    probe_path=$original_path
fi
if ! env PATH="$probe_path" timeout 10 "$shell_path" -l -i -c 'command -v rustc' >/dev/null 2>&1; then
    if command -v plasmashell >/dev/null 2>&1; then
        if [ -f "$plasma_env_file" ]; then
            say
            say "A login shell cannot see rustc; $plasma_env_file already exists."
        else
            mkdir -p "$plasma_env_dir"
            printf 'export PATH="$HOME/.cargo/bin:$PATH"\n' > "$plasma_env_file"
            say
            say "A login shell cannot see rustc; wrote $plasma_env_file."
            say "Log out and back in so the Plasma session picks it up."
        fi
        say "Zed must be restarted after the session environment changes."
    else
        say
        say "A login shell cannot see rustc. Add this line to ~/.profile or"
        say "~/.bashrc, then restart Zed:"
        say '  . "$HOME/.cargo/env"'
    fi
else
    say
    say "A login shell sees rustc; no Plasma environment file is needed."
fi

say
say "Done. In Zed: open the repository that contains extension.toml and"
say "Cargo.toml, run 'zed: install dev extension', and select that directory."
say "Zed reads the environment at startup, so quit it completely first."
