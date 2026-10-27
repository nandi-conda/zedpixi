//! A Zed extension that makes Zed use a project's pixi environment.
//!
//! A workspace is pixi-aware when its root contains `pixi.toml`. The
//! extension resolves that project's environment with
//! `pixi info --json --manifest-path <root>/pixi.toml` and exposes the
//! environment's executable directory, plus an environment block whose `PATH`
//! starts with that directory (see [`pixi`]).
//!
//! The extension deliberately provides no language servers, context servers or
//! slash commands. Zed's language servers, debug adapters, tasks and
//! formatters are built in, and the Zed extension API has no hook that changes
//! their environment. Project settings are the supported way to point them at
//! the pixi environment; `pixi::resolve_tool` gives the absolute path of a tool
//! inside the environment and `pixi::command_env` gives the matching
//! environment block.

use zed_extension_api as zed;

pub mod pixi;

struct PixiExtension;

impl zed::Extension for PixiExtension {
    fn new() -> Self {
        Self
    }
}

zed::register_extension!(PixiExtension);
