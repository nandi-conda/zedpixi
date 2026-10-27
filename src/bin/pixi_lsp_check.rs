//! Check pixi language server discovery natively, without Zed.
//!
//! Build and run with:
//!
//! ```sh
//! cargo run --features native-check --bin pixi-lsp-check -- [project-root]
//! ```
//!
//! This uses the same environment selection and candidate ordering as the
//! extension, but checks paths with `std::path` instead of the Zed host.

use std::path::Path;
use std::process::Command;

use zed_pixi::pixi;
use zed_pixi::servers;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let root = std::path::absolute(&root).map_err(|error| error.to_string())?;
    let root = root.to_string_lossy().to_string();

    let manifest = Path::new(&root).join(pixi::MANIFEST_FILE);
    if !manifest.is_file() {
        return Err(format!(
            "no {} at the workspace root ({})",
            pixi::MANIFEST_FILE,
            manifest.display()
        ));
    }
    let manifest = manifest.to_string_lossy().to_string();

    let output = Command::new("pixi")
        .args(["info", "--json", "--manifest-path", &manifest])
        .output()
        .map_err(|error| format!("could not run pixi: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "pixi info failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("could not parse pixi info output: {error}"))?;
    let (name, prefix) = pixi::select_environment(&value)?;

    println!("project:     {root}");
    println!("manifest:    {manifest}");
    println!("environment: {name}");
    println!("prefix:      {prefix}");
    println!();

    let windows = cfg!(windows);
    for spec in servers::SERVERS {
        // The same rule as the extension: the binary must be part of the pixi
        // environment.
        let mut checked = Vec::new();
        let mut found = None;
        for candidate in spec.candidates {
            let path = pixi::resolve_tool(
                &prefix,
                candidate.binary,
                windows,
                |path| Path::new(path).is_file(),
                &mut checked,
            );
            if let Some(path) = path {
                found = Some((path, candidate));
                break;
            }
        }
        match found {
            Some((path, candidate)) => {
                println!("{}: {} {:?}", spec.id, path, candidate.lsp_args);
            }
            None => {
                println!("{}: not found (tried {})", spec.id, checked.join(", "));
            }
        }
    }

    Ok(())
}
