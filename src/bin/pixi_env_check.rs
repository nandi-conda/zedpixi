//! Check pixi environment resolution and `PATH` propagation natively.
//!
//! Build and run with:
//!
//! ```sh
//! cargo run --features native-check --bin pixi-env-check -- [project-root] [tool...]
//! ```
//!
//! The check uses the same environment selection and path construction as the
//! extension, but runs `pixi` and tests files with `std` instead of the Zed
//! host. It prints the resolved environment and the `PATH` that Zed processes
//! should receive.
//!
//! Every tool argument must resolve inside the environment. That is what makes
//! the check useful: a tool that only exists on the host `PATH` fails here, so
//! the strict "pixi first, no host fallback" rule is visible without Zed.
//! Without tool arguments the check lists the executables in the environment's
//! `bin` directory instead.

use std::fs;
use std::path::Path;
use std::process::Command;

use zed_pixi::pixi;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| ".".to_string());
    let tools: Vec<String> = args.collect();

    let root = std::path::absolute(&root)
        .map_err(|error| format!("invalid project root: {error}"))?
        .to_string_lossy()
        .to_string();
    let manifest = Path::new(&root).join(pixi::MANIFEST_FILE);
    if !manifest.is_file() {
        return Err(format!(
            "no {} at the workspace root ({})",
            pixi::MANIFEST_FILE,
            manifest.display()
        ));
    }
    let manifest = manifest.to_string_lossy().to_string();

    let pixi = which_pixi().ok_or_else(|| {
        "pixi was not found on PATH. The extension never guesses an environment \
         path without the pixi CLI."
            .to_string()
    })?;
    let output = Command::new(&pixi)
        .args(["info", "--json", "--manifest-path", &manifest])
        .output()
        .map_err(|error| format!("could not run `{pixi} info --json`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`pixi info --json` failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("could not parse pixi info output: {error}"))?;
    let (name, prefix) = pixi::select_environment(&value)?;

    let windows = cfg!(windows);
    let bin_dir = pixi::env_bin_dir(&prefix, windows);
    let inherited = std::env::var("PATH").unwrap_or_default();
    let path = pixi::prepend_path(&bin_dir, &inherited, windows);

    println!("project:     {root}");
    println!("manifest:    {manifest}");
    println!("environment: {name}");
    println!("prefix:      {prefix}");
    println!("bin:         {bin_dir}");
    println!("PATH:        {path}");
    println!();

    let tools = if tools.is_empty() {
        list_executables(&bin_dir)
            .map_err(|error| format!("could not list {bin_dir}: {error}"))?
    } else {
        tools
    };

    let mut missing = Vec::new();
    for tool in &tools {
        match pixi::resolve_tool_with(&prefix, tool, windows, is_executable) {
            Ok(path) => {
                let first = first_on_inherited_path(tool, &inherited, windows);
                match first {
                    Some(host) if host != path => {
                        println!("{tool}: {path} (shadows host {host})")
                    }
                    Some(_) => println!("{tool}: {path} (also first on host PATH)"),
                    None => println!("{tool}: {path}"),
                }
            }
            Err(_) => {
                let host = first_on_inherited_path(tool, &inherited, windows);
                let detail = host
                    .map(|path| format!("host copy at {path} is ignored"))
                    .unwrap_or_else(|| "not on the host PATH either".to_string());
                println!("{tool}: NOT IN ENVIRONMENT ({detail})");
                missing.push(tool.clone());
            }
        }
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} tool(s) are not in the pixi environment: {}",
            missing.len(),
            missing.join(", ")
        ))
    }
}

fn which_pixi() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["pixi", "pixi.exe"] {
            let candidate = dir.join(name);
            if is_executable(&candidate.to_string_lossy()) {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn first_on_inherited_path(binary: &str, inherited: &str, windows: bool) -> Option<String> {
    let separator = if windows { ';' } else { ':' };
    for dir in inherited.split(separator).filter(|dir| !dir.is_empty()) {
        for path in executable_names(dir, binary, windows) {
            if is_executable(&path) {
                return Some(path);
            }
        }
    }
    None
}

/// Names a shell would try for `binary` in the directory `dir`.
fn executable_names(dir: &str, binary: &str, windows: bool) -> Vec<String> {
    let dir = dir.trim_end_matches(['/', '\\']);
    if windows {
        let mut paths = Vec::new();
        for extension in ["exe", "cmd", "bat"] {
            paths.push(format!("{dir}\\{binary}.{extension}"));
        }
        paths.push(format!("{dir}\\{binary}"));
        paths
    } else {
        vec![format!("{dir}/{binary}")]
    }
}

fn list_executables(dir: &str) -> Result<Vec<String>, std::io::Error> {
    let mut names: Vec<String> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_file()).unwrap_or(false))
        .filter(|entry| is_executable(&entry.path().to_string_lossy()))
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    Ok(names)
}

fn is_executable(path: &str) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
