//! A Zed extension that starts language servers from a project's pixi
//! environment.
//!
//! For every server this extension provides, it looks for the server binary in
//! the environment's `bin` directory (or `Scripts` on Windows), then falls back
//! to `$PATH`. The server binary and its arguments can be overridden with the
//! usual Zed LSP settings.

use zed_extension_api as zed;

pub mod pixi;
pub mod servers;

use zed::settings::LspSettings;
use zed::{LanguageServerId, Worktree};

struct PixiExtension;

impl zed::Extension for PixiExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> zed::Result<zed::Command> {
        let id: &str = language_server_id.as_ref();
        let spec = servers::spec(id)
            .ok_or_else(|| format!("pixi extension does not provide `{id}`"))?;
        let config = pixi::read_config(worktree)?;
        let settings = LspSettings::for_worktree(id, worktree).unwrap_or_default();
        let binary = settings.binary;
        let (os, _arch) = zed::current_platform();
        let windows = pixi::is_windows(os);

        // An explicit path always wins. This also covers users who keep a
        // language server outside of pixi.
        if let Some(path) = binary.as_ref().and_then(|binary| binary.path.clone()) {
            let args = binary
                .as_ref()
                .and_then(|binary| binary.arguments.clone())
                .unwrap_or_else(|| default_args(&spec.candidates[0]));
            let env = match pixi::resolve(worktree, &config) {
                Ok(env) => pixi::command_env(worktree, &env, windows),
                Err(_) => Vec::new(),
            };
            let env = pixi::merge_env(env, binary.and_then(|binary| binary.env));
            return Ok(command(path, args, env));
        }

        let env = pixi::resolve(worktree, &config)?;
        let mut checked = Vec::new();

        let found = pixi::pick_candidate(
            spec.candidates,
            &env.prefix,
            windows,
            |path| pixi::is_executable(path, windows),
            &mut checked,
        );
        if let Some((path, candidate)) = found {
            let args = binary
                .as_ref()
                .and_then(|binary| binary.arguments.clone())
                .unwrap_or_else(|| default_args(candidate));
            let defaults = pixi::command_env(worktree, &env, windows);
            let env = pixi::merge_env(defaults, binary.as_ref().and_then(|b| b.env.clone()));
            return Ok(command(path, args, env));
        }

        // The binary may also be installed outside the environment.
        for candidate in spec.candidates {
            if let Some(path) = worktree.which(candidate.binary) {
                let args = binary
                    .as_ref()
                    .and_then(|binary| binary.arguments.clone())
                    .unwrap_or_else(|| default_args(candidate));
                let env: Vec<(String, String)> = binary
                    .as_ref()
                    .and_then(|b| b.env.clone())
                    .map(|env| env.into_iter().collect())
                    .unwrap_or_default();
                return Ok(command(path, args, env));
            }
        }

        let note = env
            .note
            .as_deref()
            .map(|note| format!("\n{note}"))
            .unwrap_or_default();
        Err(format!(
            "no language server for `{id}` found in the pixi environment `{}` ({}).\n\
             Checked: {}\n\
             Install one with `{}`, or set `lsp.{id}.binary.path` in your Zed settings.{note}",
            env.name,
            env.prefix,
            checked.join(", "),
            servers::install_hint(id),
        ))
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        let settings =
            LspSettings::for_worktree(language_server_id.as_ref(), worktree).unwrap_or_default();
        Ok(settings.initialization_options)
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        let settings =
            LspSettings::for_worktree(language_server_id.as_ref(), worktree).unwrap_or_default();
        Ok(settings.settings)
    }
}

fn default_args(candidate: &servers::Candidate) -> Vec<String> {
    candidate
        .lsp_args
        .iter()
        .map(|arg| arg.to_string())
        .collect()
}

fn command(command: String, args: Vec<String>, env: Vec<(String, String)>) -> zed::Command {
    zed::Command {
        command,
        args,
        env,
    }
}

zed::register_extension!(PixiExtension);
