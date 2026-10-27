//! Debug adapters started from the pixi environment.
//!
//! Zed asks the extension for a [`DebugAdapterBinary`] whenever a debug
//! session uses one of the adapters declared in `extension.toml`. Both
//! adapters here speak DAP over stdio, so the command is simply the binary
//! inside the environment, with the environment's activation variables.
//!
//! Adapters that need a TCP port (`debugpy`, `dlv dap`) are not provided: the
//! extension API has no port allocator, so guessing one would be worse than
//! letting the user keep Zed's built-in adapter.

use serde_json::{json, Map, Value};

use zed_extension_api::{StartDebuggingRequestArgumentsRequest as RequestKind, *};

pub struct Adapter {
    /// The adapter name used in `extension.toml` and in `debug.json`.
    pub id: &'static str,
    /// The executable inside the pixi environment.
    pub binary: &'static str,
    /// Arguments that make the binary speak DAP over stdio.
    pub args: &'static [&'static str],
    pub languages: &'static [&'static str],
    /// Key the adapter expects for an attach request's process id.
    pub attach_pid_key: &'static str,
    /// Key the adapter expects for "stop on entry".
    pub stop_on_entry_key: &'static str,
}

pub const ADAPTERS: &[Adapter] = &[
    Adapter {
        id: "pixi-gdb",
        binary: "gdb",
        args: &["-i=dap"],
        languages: &["C", "C++"],
        attach_pid_key: "pid",
        stop_on_entry_key: "stopAtBeginningOfMainSubprogram",
    },
    Adapter {
        id: "pixi-lldb",
        binary: "lldb-dap",
        args: &[],
        languages: &["Rust", "C", "C++"],
        attach_pid_key: "processId",
        stop_on_entry_key: "stopOnEntry",
    },
];

pub fn spec(id: &str) -> Option<&'static Adapter> {
    ADAPTERS.iter().find(|adapter| adapter.id == id)
}

/// Read the request kind out of an adapter configuration.
pub fn request_kind(config: &Value) -> Result<RequestKind, String> {
    match config.get("request").and_then(Value::as_str) {
        Some("launch") => Ok(RequestKind::Launch),
        Some("attach") => Ok(RequestKind::Attach),
        Some(other) => Err(format!(
            "unknown debug request `{other}`; expected `launch` or `attach`"
        )),
        None => Err("debug configuration has no `request` field".to_string()),
    }
}

/// Translate Zed's adapter-agnostic debug configuration into this adapter's
/// JSON configuration.
///
/// `request` is the JSON encoding of [`DebugConfig::request`]: either
/// `{"request":"launch","program":...,"cwd":...,"args":[...],"envs":[...]}`
/// or `{"request":"attach","process_id":...}`.
pub fn scenario_config(
    adapter_id: &str,
    request: &Value,
    label: &str,
    stop_on_entry: Option<bool>,
) -> Result<Value, String> {
    let adapter = spec(adapter_id)
        .ok_or_else(|| format!("`{adapter_id}` is not a debug adapter from the pixi extension"))?;

    let mut config = Map::new();
    config.insert("request".to_string(), request["request"].clone());

    match request_kind(request)? {
        RequestKind::Launch => {
            let program = request
                .get("program")
                .and_then(Value::as_str)
                .ok_or_else(|| "a launch request needs a `program`".to_string())?;
            config.insert("program".to_string(), json!(program));

            if let Some(cwd) = request.get("cwd").and_then(Value::as_str) {
                config.insert("cwd".to_string(), json!(cwd));
            }
            if let Some(args) = request.get("args").cloned() {
                config.insert("args".to_string(), args);
            }
            if let Some(envs) = request.get("envs").and_then(Value::as_array) {
                let env = envs
                    .iter()
                    .filter_map(|pair| {
                        let pair = pair.as_array()?;
                        Some((pair.first()?.as_str()?.to_string(), pair.get(1)?.clone()))
                    })
                    .collect::<Map<String, Value>>();
                if !env.is_empty() {
                    config.insert("env".to_string(), Value::Object(env));
                }
            }
            if let Some(stop_on_entry) = stop_on_entry {
                config.insert(adapter.stop_on_entry_key.to_string(), json!(stop_on_entry));
            }
        }
        RequestKind::Attach => {
            let process_id = request
                .get("process_id")
                .cloned()
                .ok_or_else(|| "an attach request needs a `process_id`".to_string())?;
            config.insert(adapter.attach_pid_key.to_string(), process_id);
        }
    }

    Ok(json!({
        "label": label,
        "adapter": adapter.id,
        "config": Value::Object(config),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_declared_adapters() {
        assert_eq!(spec("pixi-gdb").unwrap().binary, "gdb");
        assert_eq!(spec("pixi-lldb").unwrap().binary, "lldb-dap");
        assert!(spec("Debugpy").is_none());
    }

    #[test]
    fn reads_the_request_kind() {
        assert_eq!(request_kind(&json!({"request": "launch"})).unwrap(), RequestKind::Launch);
        assert_eq!(request_kind(&json!({"request": "attach"})).unwrap(), RequestKind::Attach);
        assert!(request_kind(&json!({"request": "run"})).is_err());
        assert!(request_kind(&json!({})).is_err());
    }

    #[test]
    fn builds_a_launch_configuration() {
        let request = json!({
            "request": "launch",
            "program": "/repo/.pixi/envs/default/bin/python",
            "cwd": "/repo",
            "args": ["main.py"],
            "envs": [["APP_ENV", "test"]],
        });
        let scenario = scenario_config("pixi-lldb", &request, "LLDB: main", Some(true)).unwrap();
        assert_eq!(scenario["adapter"], "pixi-lldb");
        assert_eq!(scenario["label"], "LLDB: main");
        assert_eq!(scenario["config"]["request"], "launch");
        assert_eq!(scenario["config"]["program"], "/repo/.pixi/envs/default/bin/python");
        assert_eq!(scenario["config"]["cwd"], "/repo");
        assert_eq!(scenario["config"]["args"][0], "main.py");
        assert_eq!(scenario["config"]["env"]["APP_ENV"], "test");
        assert_eq!(scenario["config"]["stopOnEntry"], true);
    }

    #[test]
    fn builds_an_attach_configuration_with_the_adapter_key() {
        let request = json!({"request": "attach", "process_id": 4242});
        let gdb = scenario_config("pixi-gdb", &request, "GDB: attach", None).unwrap();
        assert_eq!(gdb["config"]["request"], "attach");
        assert_eq!(gdb["config"]["pid"], 4242);

        let lldb = scenario_config("pixi-lldb", &request, "LLDB: attach", None).unwrap();
        assert_eq!(lldb["config"]["processId"], 4242);
    }

    #[test]
    fn rejects_configurations_it_cannot_translate() {
        let error = scenario_config("Debugpy", &json!({"request": "launch"}), "x", None).unwrap_err();
        assert!(error.contains("not a debug adapter"), "{error}");
        let error =
            scenario_config("pixi-gdb", &json!({"request": "launch"}), "x", None).unwrap_err();
        assert!(error.contains("program"), "{error}");
    }
}
