//! Locating and describing the pixi environment that belongs to a worktree.
//!
//! Zed extensions cannot list directories, so the environment is resolved by
//! asking the pixi CLI (`pixi info --json`) and individual binaries are then
//! probed by running them with `--version`.

use std::collections::HashMap;

use serde_json::Value;
use zed_extension_api as zed;

use crate::servers::Candidate;

/// Optional per-project configuration, relative to the worktree root.
pub const CONFIG_FILE: &str = ".zed/pixi.json";
pub const DEFAULT_MANIFEST: &str = "pixi.toml";
pub const DEFAULT_ENVIRONMENT: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub manifest_path: String,
    pub environment: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            manifest_path: DEFAULT_MANIFEST.to_string(),
            environment: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PixiEnv {
    pub name: String,
    pub prefix: String,
    pub project_root: String,
    pub manifest_path: String,
    /// Set when the environment was guessed without the pixi CLI.
    pub note: Option<String>,
}

pub fn read_config(worktree: &zed::Worktree) -> Result<Config, String> {
    match worktree.read_text_file(CONFIG_FILE) {
        Ok(text) => parse_config(&text),
        Err(_) => Ok(Config::default()),
    }
}

pub fn parse_config(text: &str) -> Result<Config, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|err| format!("{CONFIG_FILE} is not valid JSON: {err}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("{CONFIG_FILE} must contain a JSON object"))?;

    let manifest_path = match object.get("manifest_path") {
        None => DEFAULT_MANIFEST.to_string(),
        Some(Value::String(path)) if !path.trim().is_empty() => path.clone(),
        Some(_) => {
            return Err(format!(
                "{CONFIG_FILE}: `manifest_path` must be a non-empty string"
            ))
        }
    };

    let environment = match object.get("environment") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) if !name.trim().is_empty() => Some(name.clone()),
        Some(_) => {
            return Err(format!(
                "{CONFIG_FILE}: `environment` must be a string or null"
            ))
        }
    };

    Ok(Config {
        manifest_path,
        environment,
    })
}

/// Resolve the pixi environment for a worktree.
pub fn resolve(worktree: &zed::Worktree, config: &Config) -> Result<PixiEnv, String> {
    let project_root = worktree.root_path();
    let manifest_path = absolute_path(&project_root, &config.manifest_path);
    let requested = config
        .environment
        .as_deref()
        .unwrap_or(DEFAULT_ENVIRONMENT);

    let Some(pixi) = worktree.which("pixi") else {
        let prefix = fallback_prefix(&manifest_path, requested);
        return Ok(PixiEnv {
            name: requested.to_string(),
            prefix,
            project_root,
            manifest_path,
            note: Some("pixi was not found on PATH; assuming .pixi/envs".to_string()),
        });
    };

    let (name, prefix) = pixi_info(&pixi, &manifest_path, requested)?;
    Ok(PixiEnv {
        name,
        prefix,
        project_root,
        manifest_path,
        note: None,
    })
}

fn pixi_info(pixi: &str, manifest_path: &str, requested: &str) -> Result<(String, String), String> {
    let mut command = zed::process::Command::new(pixi)
        .args(["info", "--json", "--manifest-path", manifest_path]);
    let output = command
        .output()
        .map_err(|err| format!("could not run `{pixi} info --json`: {err}"))?;

    if output.status != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{pixi} info --json` failed (exit status {:?}): {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(&stdout)
        .map_err(|err| format!("could not parse `pixi info --json` output: {err}"))?;
    select_environment(&value, requested)
}

/// Pick the requested environment out of `pixi info --json` output.
pub fn select_environment(value: &Value, requested: &str) -> Result<(String, String), String> {
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
        .find(|env| env.get("name").and_then(Value::as_str) == Some(requested))
        .or_else(|| {
            if requested == DEFAULT_ENVIRONMENT && environments.len() == 1 {
                environments.first()
            } else {
                None
            }
        })
        .ok_or_else(|| {
            format!(
                "pixi environment `{requested}` not found; available environments: {}",
                names.join(", ")
            )
        })?;

    let name = selected
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(requested)
        .to_string();
    let prefix = selected
        .get("prefix")
        .and_then(Value::as_str)
        .filter(|prefix| !prefix.is_empty())
        .ok_or_else(|| format!("pixi environment `{name}` has no prefix; run `pixi install`"))?;

    Ok((name, prefix.to_string()))
}

/// Pick the first candidate whose binary exists inside the environment.
///
/// `checked` receives every path that was tried, for error messages.
pub fn pick_candidate(
    candidates: &'static [Candidate],
    prefix: &str,
    windows: bool,
    mut exists: impl FnMut(&str) -> bool,
    checked: &mut Vec<String>,
) -> Option<(String, &'static Candidate)> {
    for candidate in candidates {
        for path in candidate_paths(prefix, candidate.binary, windows) {
            if exists(&path) {
                return Some((path, candidate));
            }
            checked.push(path);
        }
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

fn fallback_prefix(manifest_path: &str, environment: &str) -> String {
    let manifest_dir = manifest_path
        .rsplit_once(['/', '\\'])
        .map(|(dir, _)| dir)
        .unwrap_or(".");
    format!("{manifest_dir}/.pixi/envs/{environment}")
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
pub fn command_env(worktree: &zed::Worktree, env: &PixiEnv, windows: bool) -> Vec<(String, String)> {
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
    fn parses_defaults_from_empty_object() {
        assert_eq!(parse_config("{}").unwrap(), Config::default());
    }

    #[test]
    fn parses_manifest_and_environment() {
        let config = parse_config(
            r#"{"manifest_path": "tools/py/pixi.toml", "environment": "test"}"#,
        )
        .unwrap();
        assert_eq!(config.manifest_path, "tools/py/pixi.toml");
        assert_eq!(config.environment.as_deref(), Some("test"));
    }

    #[test]
    fn rejects_bad_config_types() {
        assert!(parse_config(r#"{"environment": 3}"#).is_err());
        assert!(parse_config(r#"{"manifest_path": ""}"#).is_err());
        assert!(parse_config("[]").is_err());
    }

    #[test]
    fn selects_requested_environment() {
        let value: Value = serde_json::from_str(
            r#"{"environments_info": [
                {"name": "default", "prefix": "/p/.pixi/envs/default"},
                {"name": "test", "prefix": "/p/.pixi/envs/test"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            select_environment(&value, "test").unwrap(),
            ("test".to_string(), "/p/.pixi/envs/test".to_string())
        );
        assert_eq!(
            select_environment(&value, "default").unwrap(),
            ("default".to_string(), "/p/.pixi/envs/default".to_string())
        );
    }

    #[test]
    fn falls_back_to_the_only_environment() {
        let value: Value = serde_json::from_str(
            r#"{"environments_info": [{"name": "dev", "prefix": "/p/.pixi/envs/dev"}]}"#,
        )
        .unwrap();
        assert_eq!(
            select_environment(&value, "default").unwrap(),
            ("dev".to_string(), "/p/.pixi/envs/dev".to_string())
        );
    }

    #[test]
    fn reports_missing_environment() {
        let value: Value =
            serde_json::from_str(r#"{"environments_info": [{"name": "default", "prefix": "/x"}]}"#)
                .unwrap();
        let error = select_environment(&value, "nope").unwrap_err();
        assert!(error.contains("default"), "{error}");
    }

    #[test]
    fn makes_absolute_paths() {
        assert_eq!(absolute_path("/repo", "pixi.toml"), "/repo/pixi.toml");
        assert_eq!(absolute_path("/repo/", "sub/pixi.toml"), "/repo/sub/pixi.toml");
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
