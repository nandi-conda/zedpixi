//! The pixi environment of a worktree.
//!
//! A workspace is pixi-aware when its root contains `pixi.toml`. The
//! environment is resolved by asking the pixi CLI
//! (`pixi info --json --manifest-path`) and every executable is then looked up
//! strictly inside the environment's `bin` directory (or `Scripts` on
//! Windows). There is no host-`PATH` fallback: a tool that is not in the pixi
//! environment is not used by the extension.
//!
//! The response must name the workspace's own manifest in
//! `project_info.manifest_path` (see [`verify_project`]). A response for a
//! different project, a parent directory or an ambient environment is
//! rejected, so no foreign environment or toolchain can be used.
//!
//! Zed's extension API cannot change the environment of the language servers,
//! debug adapters, tasks or formatters that Zed itself starts. The helpers
//! here are therefore also exposed for project settings: `command_env` builds
//! the environment block (the pixi `bin` directory first on `PATH`), and
//! `resolve_tool` returns the absolute path of a tool inside the environment.

use std::collections::HashMap;

use serde_json::Value;
use zed_extension_api as zed;

/// The manifest that makes a workspace pixi-aware.
pub const MANIFEST_FILE: &str = "pixi.toml";
pub const DEFAULT_ENVIRONMENT: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
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
    let mut command = zed::process::Command::new(pixi)
        .args(["info", "--json", "--manifest-path", manifest_path]);
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

    parse_info(&output.stdout, manifest_path)
}

/// Read a `pixi info --json` response for `manifest_path`.
///
/// The response is only accepted when pixi reports the same manifest in
/// `project_info.manifest_path`. That check ties the environment to the
/// workspace's own `pixi.toml`: the output for another project, a parent
/// directory or an ambient environment cannot be used even when it parses.
pub fn parse_info(stdout: &[u8], manifest_path: &str) -> Result<(String, String), String> {
    let value: Value = serde_json::from_slice(stdout)
        .map_err(|err| format!("could not parse `pixi info --json` output: {err}"))?;
    verify_project(&value, manifest_path)?;
    select_environment(&value)
}

/// Reject a `pixi info --json` response that belongs to another manifest.
///
/// `manifest_path` is the workspace root's `pixi.toml`; the response must name
/// the same file in `project_info.manifest_path`. A missing `project_info` is
/// an error too: without it the environment cannot be tied to this workspace.
pub fn verify_project(value: &Value, manifest_path: &str) -> Result<(), String> {
    let reported = value
        .get("project_info")
        .and_then(|project| project.get("manifest_path"))
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            format!(
                "`pixi info --json` did not report `project_info.manifest_path`, \
                 so the environment cannot be tied to the workspace manifest \
                 {manifest_path}. Update pixi."
            )
        })?;

    if normalize_path(reported) != normalize_path(manifest_path) {
        return Err(format!(
            "`pixi info --json` resolved a different project: it reported {reported}, \
             but the workspace manifest is {manifest_path}. Refusing to use an \
             environment from outside this workspace."
        ));
    }
    Ok(())
}

/// Compare paths the way pixi reports them: forward slashes, no trailing slash.
fn normalize_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    normalized.trim_end_matches('/').to_string()
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

/// The environment's executable directory.
pub fn env_bin_dir(prefix: &str, windows: bool) -> String {
    let prefix = prefix.trim_end_matches(['/', '\\']);
    if windows {
        format!("{prefix}\\Scripts")
    } else {
        format!("{prefix}/bin")
    }
}

/// Candidate paths for `binary` inside the environment prefix, in order.
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

/// `bin_dir` first on `PATH`, followed by the inherited value.
pub fn prepend_path(bin_dir: &str, inherited: &str, windows: bool) -> String {
    if inherited.is_empty() {
        bin_dir.to_string()
    } else {
        let separator = if windows { ';' } else { ':' };
        format!("{bin_dir}{separator}{inherited}")
    }
}

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

/// Resolve `binary` strictly inside the pixi environment.
///
/// Unlike a shell lookup, this never consults the host `PATH`: either the
/// binary exists in `<prefix>/bin` (or `<prefix>/Scripts`) or this is an error.
pub fn resolve_tool(prefix: &str, binary: &str, windows: bool) -> Result<String, String> {
    resolve_tool_with(prefix, binary, windows, |path| is_executable(path, windows))
}

/// `resolve_tool`, with the existence test supplied by the caller.
///
/// Native tools pass a `std` based test; the extension passes `is_executable`.
pub fn resolve_tool_with(
    prefix: &str,
    binary: &str,
    windows: bool,
    mut exists: impl FnMut(&str) -> bool,
) -> Result<String, String> {
    let mut checked = Vec::new();
    for path in candidate_paths(prefix, binary, windows) {
        if exists(&path) {
            return Ok(path);
        }
        checked.push(path);
    }
    Err(format!(
        "`{binary}` is not in the pixi environment (checked {})",
        checked.join(", ")
    ))
}

/// Environment for a process that should run inside the pixi environment.
///
/// The environment's executable directory comes first on `PATH`, so tools that
/// a server or task spawns (`python`, `node`, `cargo`, ...) are the ones from
/// the pixi environment. The rest of the inherited `PATH` follows so that
/// basic system tools remain available; the extension itself never uses it to
/// find a project tool.
pub fn command_env(worktree: &zed::Worktree, env: &PixiEnv, windows: bool) -> Vec<(String, String)> {
    let bin_dir = env_bin_dir(&env.prefix, windows);
    let inherited_path = shell_env(worktree, "PATH");

    vec![
        (
            "PATH".to_string(),
            prepend_path(&bin_dir, &inherited_path, windows),
        ),
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
        let value: Value = serde_json::from_str(
            r#"{"environments_info": [{"name": "default", "prefix": ""}]}"#,
        )
        .unwrap();
        let error = select_environment(&value).unwrap_err();
        assert!(error.contains("pixi install"), "{error}");
    }

    const WORKSPACE_MANIFEST: &str = "/home/me/project/pixi.toml";

    #[test]
    fn accepts_a_response_for_the_workspace_manifest() {
        let info = r#"{
            "project_info": {"name": "project", "manifest_path": "/home/me/project/pixi.toml"},
            "environments_info": [{"name": "default", "prefix": "/home/me/project/.pixi/envs/default"}]
        }"#;
        assert_eq!(
            parse_info(info.as_bytes(), WORKSPACE_MANIFEST).unwrap(),
            (
                "default".to_string(),
                "/home/me/project/.pixi/envs/default".to_string()
            )
        );
    }

    #[test]
    fn rejects_a_foreign_project_manifest() {
        // A perfectly valid response, but for another project.
        let info = r#"{
            "project_info": {"name": "leancheck", "manifest_path": "/tmp/leancheck/pixi.toml"},
            "environments_info": [{"name": "default", "prefix": "/tmp/leancheck/.pixi/envs/default"}]
        }"#;
        let error = parse_info(info.as_bytes(), WORKSPACE_MANIFEST).unwrap_err();
        assert!(error.contains("/tmp/leancheck/pixi.toml"), "{error}");
        assert!(error.contains(WORKSPACE_MANIFEST), "{error}");
        assert!(error.contains("outside this workspace"), "{error}");
    }

    #[test]
    fn rejects_a_response_without_a_project_manifest() {
        let info = r#"{"environments_info": [{"name": "default", "prefix": "/env"}]}"#;
        let error = parse_info(info.as_bytes(), WORKSPACE_MANIFEST).unwrap_err();
        assert!(error.contains("project_info.manifest_path"), "{error}");
        assert!(error.contains(WORKSPACE_MANIFEST), "{error}");
    }

    #[test]
    fn a_foreign_lean_pin_cannot_leak_in() {
        // Another Lean project pins `lean4 = "==4.33.0"` and has a `lake` in
        // its prefix. Nothing about it may reach this workspace.
        let foreign = r#"{
            "project_info": {"name": "leancheck", "manifest_path": "/tmp/leancheck/pixi.toml"},
            "environments_info": [{
                "name": "default",
                "prefix": "/tmp/leancheck/.pixi/envs/default",
                "dependencies": ["lean4"]
            }]
        }"#;
        assert!(parse_info(foreign.as_bytes(), WORKSPACE_MANIFEST).is_err());

        // This workspace has no `lean4`, so it has no `lake` either; the one
        // in the foreign prefix is never considered.
        let local = r#"{
            "project_info": {"name": "project", "manifest_path": "/home/me/project/pixi.toml"},
            "environments_info": [{
                "name": "default",
                "prefix": "/home/me/project/.pixi/envs/default",
                "dependencies": ["rust-analyzer", "basedpyright", "taplo", "python"]
            }]
        }"#;
        let (_, prefix) = parse_info(local.as_bytes(), WORKSPACE_MANIFEST).unwrap();
        let error = resolve_tool_with(&prefix, "lake", false, |path| {
            path == "/tmp/leancheck/.pixi/envs/default/bin/lake"
        })
        .unwrap_err();
        assert!(error.contains("not in the pixi environment"), "{error}");
        assert!(!error.contains("leancheck"), "{error}");
    }

    #[test]
    fn compares_paths_the_way_pixi_reports_them() {
        assert_eq!(normalize_path("/a//b/"), "/a/b");
        assert_eq!(normalize_path(r"C:\a\b\pixi.toml"), "C:/a/b/pixi.toml");
        assert_ne!(
            normalize_path("/a/b/pixi.toml"),
            normalize_path("/a/c/pixi.toml")
        );
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
    fn puts_the_environment_first_on_path() {
        assert_eq!(
            prepend_path("/env/bin", "/usr/bin:/bin", false),
            "/env/bin:/usr/bin:/bin"
        );
        assert_eq!(prepend_path("/env/bin", "", false), "/env/bin");
        assert_eq!(
            prepend_path(r"C:\env\Scripts", r"C:\Windows", true),
            r"C:\env\Scripts;C:\Windows"
        );
    }

    #[test]
    fn resolves_a_tool_inside_the_environment() {
        let found = resolve_tool_with("/env", "taplo", false, |path| path == "/env/bin/taplo")
            .unwrap();
        assert_eq!(found, "/env/bin/taplo");
    }

    #[test]
    fn does_not_fall_back_to_the_host_path() {
        // The host has the tool, the environment does not: resolution fails.
        let error = resolve_tool_with("/env", "taplo", false, |path| path == "/usr/bin/taplo")
            .unwrap_err();
        assert!(error.contains("/env/bin/taplo"), "{error}");
        assert!(error.contains("not in the pixi environment"), "{error}");
    }

    #[test]
    fn resolves_windows_scripts_and_bin() {
        let found = resolve_tool_with(r"C:\env", "taplo", true, |path| {
            path == r"C:\env\Scripts\taplo.exe"
        })
        .unwrap();
        assert_eq!(found, r"C:\env\Scripts\taplo.exe");
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
