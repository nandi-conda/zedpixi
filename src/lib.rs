//! A Zed extension that makes Zed use a project's pixi environment.
//!
//! A workspace is pixi-aware when its root contains `pixi.toml`. The
//! extension resolves that project's environment with
//! `pixi info --json --manifest-path <root>/pixi.toml` and exposes the
//! environment's executable directory, plus an environment block whose `PATH`
//! starts with that directory (see [`pixi`]).
//!
//! The extension deliberately provides no language servers, context servers or
//! slash commands. Zed's language servers, formatters, linters and tasks are
//! built in, and the Zed extension API has no hook that changes their
//! environment; project settings are the supported way to point them at the
//! pixi environment (`docs/zed-config.md`, `scripts/zed_pixi_sync.py`).
//!
//! Debug adapters are the exception: Zed does ask the extension for their
//! command. [`zed::Extension::get_dap_binary`] starts `pixi-gdb` and
//! `pixi-lldb` from the pixi environment, with [`pixi::command_env`] so the
//! adapter and debuggee see the environment's `PATH`. That routing needs no
//! generated settings and never looks at the host `PATH`.

use zed_extension_api as zed;

pub mod dap;
pub mod pixi;

struct PixiExtension;

impl zed::Extension for PixiExtension {
    fn new() -> Self {
        Self
    }

    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: zed::DebugTaskDefinition,
        user_provided_debug_adapter_path: Option<String>,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::DebugAdapterBinary> {
        let adapter = dap::spec(&adapter_name)
            .ok_or_else(|| format!("`{adapter_name}` is not a debug adapter from this extension"))?;
        let env = pixi::resolve(worktree)?;
        let (os, _arch) = zed::current_platform();
        let windows = pixi::is_windows(os);

        // A path chosen in the debug settings always wins; otherwise the
        // adapter must come from the pixi environment. There is no host
        // fallback.
        let command = match user_provided_debug_adapter_path {
            Some(path) => path,
            None => pixi::resolve_tool(&env.prefix, adapter.binary, windows)?,
        };
        let config_json: serde_json::Value = serde_json::from_str(&config.config)
            .map_err(|err| format!("could not parse debug configuration: {err}"))?;
        let request = dap::request_kind(&config_json)?;

        Ok(zed::DebugAdapterBinary {
            command: Some(command),
            arguments: adapter.args.iter().map(|arg| arg.to_string()).collect(),
            envs: pixi::command_env(worktree, &env, windows),
            cwd: None,
            connection: None,
            request_args: zed::StartDebuggingRequestArguments {
                configuration: config.config.clone(),
                request,
            },
        })
    }

    fn dap_request_kind(
        &mut self,
        adapter_name: String,
        config: serde_json::Value,
    ) -> zed::Result<zed::StartDebuggingRequestArgumentsRequest> {
        dap::spec(&adapter_name)
            .ok_or_else(|| format!("`{adapter_name}` is not a debug adapter from this extension"))?;
        dap::request_kind(&config)
    }

    fn dap_config_to_scenario(
        &mut self,
        config: zed::DebugConfig,
    ) -> zed::Result<zed::DebugScenario> {
        let request = match &config.request {
            zed::DebugRequest::Launch(launch) => serde_json::json!({
                "request": "launch",
                "program": launch.program,
                "cwd": launch.cwd,
                "args": launch.args,
                "envs": launch.envs,
            }),
            zed::DebugRequest::Attach(attach) => serde_json::json!({
                "request": "attach",
                "process_id": attach.process_id,
            }),
        };
        let scenario =
            dap::scenario_config(&config.adapter, &request, &config.label, config.stop_on_entry)?;

        Ok(zed::DebugScenario {
            label: config.label,
            adapter: config.adapter,
            build: None,
            config: scenario["config"].to_string(),
            tcp_connection: None,
        })
    }
}

zed::register_extension!(PixiExtension);
