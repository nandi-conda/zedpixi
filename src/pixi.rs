//! Locating and describing the pixi environment that belongs to a worktree.
//!
//! The extension uses the project the user opened: the `pixi.toml` at the
//! worktree root. Zed extensions cannot list directories, so the environment is
//! resolved by asking the pixi CLI (`pixi info --json --manifest-path`) and
//! individual binaries are then probed with the platform's file test.

use std::collections::HashMap;

use serde_json::Value;
use zed_extension_api as zed;

/// The manifest the extension uses: always the worktree root's `pixi.toml`.
pub const MANIFEST_FILE: &str = "pixi.toml";
pub const DEFAULT_ENVIRONMENT: &str = "default";

#[derive(Debug, Clone)]
pub struct PixiEnv {
    pub name: String,
    pub prefix: String,
    pub project_root: String,
    pub manifest_path: String,
}

/// Resolve the pixi environment described by the worktree root's `pixi.toml`.
pub fn resolve(worktree: &zed::Worktree) -> Result<PixiEnv, String> {
    let project_root = worktree.root_path();
    let manifest_path = absolute_path(&project_root, MANIFEST_FILE);

    worktree.read_text_file(MANIFEST_FILE).map_err(|_| {
        format!(
            "no {MANIFEST_FILE} at the workspace root ({manifest_path}). \
             The pixi extension uses the project the workspace opens; add \
             {MANIFEST_FILE} there, or open the project directory itself."
        )
    })?;

    let pixi = worktree.which("pixi").ok_or_else(|| {
        "pixi was not found on PATH. Install pixi and make sure the `pixi` \
         command is available to Zed."
            .to_string()
    })?;

    let (name, prefix) = pixi_info(&pixi, &manifest_path)?;
    Ok(PixiEnv {
        name,
        prefix,
        project_root,
        manifest_path,
    })
}

fn pixi_info(pixi: &str, manifest_path: &str) -> Result<(String, String), String> {
    let mut command =
        zed::process::Command::new(pixi).args(["info", "--json", "--manifest-path", manifest_path]);
    let output = command
        .output()
        .map_err(|err| format!("could not run `{pixi} info --json`: {err}"))?;

    if output.status != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{pixi} info --json --manifest-path {manifest_path}` failed \
             (exit status {:?}): {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(&stdout)
        .map_err(|err| format!("could not parse `pixi info --json` output: {err}"))?;
    select_environment(&value)
}

/// Pick the environment out of `pixi info --json` output.
///
/// The `default` environment is used when the project has one. A project with
/// exactly one environment uses that one; anything else is an error listing the
/// available names.
pub fn select_environment(value: &Value) -> Result<(String, String), String> {
    let environments = value
        .get("environments_info")
        .and_then(Value::as_array)
        .ok_or_else(|| "`pixi info --json` did not contain `environments_info`".to_string())?;

    let names: Vec<&str> = environments
        .iter()
        .filter_map(|env| env.get("name").and_then(Value::as_str))
        .collect();

    let selected = environments
        .iter()
        .find(|env| env.get("name").and_then(Value::as_str) == Some(DEFAULT_ENVIRONMENT))
        .or_else(|| {
            if environments.len() == 1 {
                environments.first()
            } else {
                None
            }
        })
        .ok_or_else(|| {
            format!(
                "this project has no `{DEFAULT_ENVIRONMENT}` pixi environment; \
                 available environments: {}",
                names.join(", ")
            )
        })?;

    let name = selected
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_ENVIRONMENT)
        .to_string();
    let prefix = selected
        .get("prefix")
        .and_then(Value::as_str)
        .filter(|prefix| !prefix.is_empty())
        .ok_or_else(|| format!("pixi environment `{name}` has no prefix; run `pixi install`"))?;

    Ok((name, prefix.to_string()))
}

/// Resolve one executable inside the pixi environment.
///
/// Only tools that are part of the environment are resolved: there is no host
/// `PATH` lookup, so Zed never silently runs a binary outside the project.
/// `checked` receives every path that was tried, for error messages.
pub fn resolve_tool(
    prefix: &str,
    binary: &str,
    windows: bool,
    mut exists: impl FnMut(&str) -> bool,
    checked: &mut Vec<String>,
) -> Option<String> {
    for path in candidate_paths(prefix, binary, windows) {
        if exists(&path) {
            return Some(path);
        }
        checked.push(path);
    }
    None
}

pub fn absolute_path(root: &str, path: &str) -> String {
    if path.starts_with('/') || path.starts_with('\\') || path.get(1..2) == Some(":") {
        path.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches(['/', '\\']), path)
    }
}

pub fn is_windows(os: zed::Os) -> bool {
    matches!(os, zed::Os::Windows)
}

/// Candidate paths for a binary inside the environment prefix.
pub fn candidate_paths(prefix: &str, binary: &str, windows: bool) -> Vec<String> {
    let prefix = prefix.trim_end_matches(['/', '\\']);
    if windows {
        let mut paths = Vec::new();
        for dir in ["Scripts", "bin"] {
            for extension in ["exe", "cmd", "bat"] {
                paths.push(format!("{prefix}\\{dir}\\{binary}.{extension}"));
            }
            paths.push(format!("{prefix}\\{dir}\\{binary}"));
        }
        paths
    } else {
        vec![format!("{prefix}/bin/{binary}")]
    }
}

pub fn env_bin_dir(prefix: &str, windows: bool) -> String {
    let prefix = prefix.trim_end_matches(['/', '\\']);
    if windows {
        format!("{prefix}\\Scripts")
    } else {
        format!("{prefix}/bin")
    }
}

/// Whether `path` exists and is executable.
///
/// Some language servers (notably `basedpyright-langserver`) start speaking LSP
/// as soon as they are run and accept neither `--version` nor `--help`, so the
/// candidate paths are checked with the platform's file test instead of by
/// running them.
pub fn is_executable(path: &str, windows: bool) -> bool {
    let mut command = if windows {
        let escaped = path.replace('\'', "''");
        let script =
            format!("if (Test-Path -LiteralPath '{escaped}') {{ exit 0 }} else {{ exit 1 }}");
        zed::process::Command::new("powershell").args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script.as_str(),
        ])
    } else {
        zed::process::Command::new("/bin/sh").args(["-c", "test -x \"$1\"", "sh", path])
    };
    matches!(command.output(), Ok(output) if output.status == Some(0))
}

/// Environment for a language server started from the pixi environment.
///
/// The environment's `bin` directory comes first on `PATH`, so tools that the
/// server spawns (`python`, `node`, ...) are the ones from the pixi
/// environment rather than whatever happens to be on the user's `PATH`.
pub fn command_env(
    worktree: &zed::Worktree,
    env: &PixiEnv,
    windows: bool,
) -> Vec<(String, String)> {
    let bin_dir = env_bin_dir(&env.prefix, windows);
    let separator = if windows { ';' } else { ':' };
    let inherited_path = shell_env(worktree, "PATH");
    let path = if inherited_path.is_empty() {
        bin_dir
    } else {
        format!("{bin_dir}{separator}{inherited_path}")
    };

    vec![
        ("PATH".to_string(), path),
        ("VIRTUAL_ENV".to_string(), env.prefix.clone()),
        ("CONDA_PREFIX".to_string(), env.prefix.clone()),
        ("PIXI_PROJECT_ROOT".to_string(), env.project_root.clone()),
        (
            "PIXI_PROJECT_MANIFEST".to_string(),
            env.manifest_path.clone(),
        ),
        ("PIXI_ENVIRONMENT_NAME".to_string(), env.name.clone()),
    ]
}

fn shell_env(worktree: &zed::Worktree, key: &str) -> String {
    worktree
        .shell_env()
        .into_iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .unwrap_or_default()
}

/// Merge user-configured environment variables over the defaults.
pub fn merge_env(
    defaults: Vec<(String, String)>,
    overrides: Option<HashMap<String, String>>,
) -> Vec<(String, String)> {
    let Some(overrides) = overrides else {
        return defaults;
    };
    let mut merged = defaults;
    for (key, value) in overrides {
        match merged.iter_mut().find(|(name, _)| *name == key) {
            Some(entry) => entry.1 = value,
            None => merged.push((key, value)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_default_environment() {
        let value: Value = serde_json::from_str(
            r#"{"environments_info": [
                {"name": "default", "prefix": "/p/.pixi/envs/default"},
                {"name": "test", "prefix": "/p/.pixi/envs/test"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            select_environment(&value).unwrap(),
            ("default".to_string(), "/p/.pixi/envs/default".to_string())
        );
    }

    #[test]
    fn uses_the_only_environment() {
        let value: Value = serde_json::from_str(
            r#"{"environments_info": [{"name": "dev", "prefix": "/p/.pixi/envs/dev"}]}"#,
        )
        .unwrap();
        assert_eq!(
            select_environment(&value).unwrap(),
            ("dev".to_string(), "/p/.pixi/envs/dev".to_string())
        );
    }

    #[test]
    fn reports_a_missing_default_environment() {
        let value: Value = serde_json::from_str(
            r#"{"environments_info": [
                {"name": "test", "prefix": "/p/envs/test"},
                {"name": "docs", "prefix": "/p/envs/docs"}
            ]}"#,
        )
        .unwrap();
        let error = select_environment(&value).unwrap_err();
        assert!(error.contains("test, docs"), "{error}");
    }

    #[test]
    fn reports_an_environment_without_prefix() {
        let value: Value =
            serde_json::from_str(r#"{"environments_info": [{"name": "default", "prefix": ""}]}"#)
                .unwrap();
        let error = select_environment(&value).unwrap_err();
        assert!(error.contains("pixi install"), "{error}");
    }

    #[test]
    fn makes_absolute_paths() {
        assert_eq!(absolute_path("/repo", "pixi.toml"), "/repo/pixi.toml");
        assert_eq!(absolute_path("/repo/", "pixi.toml"), "/repo/pixi.toml");
        assert_eq!(absolute_path("/repo", "/abs/pixi.toml"), "/abs/pixi.toml");
    }

    #[test]
    fn builds_unix_and_windows_paths() {
        assert_eq!(
            candidate_paths("/env", "taplo", false),
            vec!["/env/bin/taplo"]
        );
        let windows = candidate_paths(r"C:\env", "taplo", true);
        assert!(windows.contains(&r"C:\env\Scripts\taplo.exe".to_string()));
        assert!(windows.contains(&r"C:\env\bin\taplo.cmd".to_string()));
        assert_eq!(env_bin_dir("/env", false), "/env/bin");
        assert_eq!(env_bin_dir(r"C:\env", true), r"C:\env\Scripts");
    }

    #[test]
    fn finds_the_binary_inside_the_environment() {
        let mut checked = Vec::new();
        let found = resolve_tool(
            "/env",
            "taplo",
            false,
            |path| path == "/env/bin/taplo",
            &mut checked,
        );
        assert_eq!(found.as_deref(), Some("/env/bin/taplo"));
        assert!(checked.is_empty());
    }

    #[test]
    fn reports_a_binary_that_is_not_in_the_environment() {
        let mut checked = Vec::new();
        let found = resolve_tool("/env", "taplo", false, |_| false, &mut checked);
        assert!(found.is_none(), "no host PATH lookup is attempted");
        assert_eq!(checked, vec!["/env/bin/taplo".to_string()]);
    }

    #[test]
    fn finds_windows_binaries_in_scripts_and_bin() {
        let mut checked = Vec::new();
        let found = resolve_tool(
            r"C:\env",
            "taplo",
            true,
            |path| path == r"C:\env\Scripts\taplo.exe",
            &mut checked,
        );
        assert_eq!(found.as_deref(), Some(r"C:\env\Scripts\taplo.exe"));
        assert!(checked.is_empty());

        let mut checked = Vec::new();
        let found = resolve_tool(
            r"C:\env",
            "taplo",
            true,
            |path| path == r"C:\env\bin\taplo",
            &mut checked,
        );
        assert_eq!(found.as_deref(), Some(r"C:\env\bin\taplo"));
        assert_eq!(checked.len(), 7);
    }

    #[test]
    fn user_environment_overrides_defaults() {
        let defaults = vec![("PATH".to_string(), "/bin".to_string())];
        let overrides = HashMap::from([
            ("PATH".to_string(), "/custom".to_string()),
            ("EXTRA".to_string(), "1".to_string()),
        ]);
        let merged = merge_env(defaults, Some(overrides));
        assert!(merged.contains(&("PATH".to_string(), "/custom".to_string())));
        assert!(merged.contains(&("EXTRA".to_string(), "1".to_string())));
        assert!(!merged.contains(&("PATH".to_string(), "/bin".to_string())));
    }
}
