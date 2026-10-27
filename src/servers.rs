//! The language servers this extension can launch, and the binaries that
//! provide them.
//!
//! Each entry lists candidates in preference order. The first candidate whose
//! binary is present in the pixi environment is used; the host `$PATH` is not
//! consulted. A candidate carries the arguments needed to speak LSP over stdio.

#[derive(Debug)]
pub struct Candidate {
    pub binary: &'static str,
    pub lsp_args: &'static [&'static str],
}

pub struct ServerSpec {
    pub id: &'static str,
    pub candidates: &'static [Candidate],
}

const fn candidate(binary: &'static str, lsp_args: &'static [&'static str]) -> Candidate {
    Candidate { binary, lsp_args }
}

const RUST: ServerSpec = ServerSpec {
    id: "pixi-rust",
    candidates: &[candidate("rust-analyzer", &[])],
};

const PYTHON: ServerSpec = ServerSpec {
    id: "pixi-python",
    candidates: &[
        candidate("basedpyright-langserver", &["--stdio"]),
        candidate("pyright-langserver", &["--stdio"]),
        candidate("pylsp", &[]),
        candidate("ruff", &["server"]),
    ],
};

const TOML: ServerSpec = ServerSpec {
    id: "pixi-toml",
    candidates: &[candidate("taplo", &["lsp", "stdio"])],
};

const YAML: ServerSpec = ServerSpec {
    id: "pixi-yaml",
    candidates: &[candidate("yaml-language-server", &["--stdio"])],
};

const TYPESCRIPT: ServerSpec = ServerSpec {
    id: "pixi-typescript",
    candidates: &[candidate("typescript-language-server", &["--stdio"])],
};

const GO: ServerSpec = ServerSpec {
    id: "pixi-go",
    candidates: &[candidate("gopls", &[])],
};

const CPP: ServerSpec = ServerSpec {
    id: "pixi-cpp",
    candidates: &[candidate("clangd", &[])],
};

pub const SERVERS: &[ServerSpec] = &[RUST, PYTHON, TOML, YAML, TYPESCRIPT, GO, CPP];

pub fn spec(id: &str) -> Option<&'static ServerSpec> {
    SERVERS.iter().find(|server| server.id == id)
}

/// Hints for `pixi add`, keyed by server id.
pub fn install_hint(id: &str) -> &'static str {
    match id {
        "pixi-rust" => "pixi add rust-analyzer",
        "pixi-python" => "pixi add basedpyright",
        "pixi-toml" => "pixi add taplo",
        "pixi-yaml" => "pixi add yaml-language-server",
        "pixi-typescript" => "pixi add typescript-language-server",
        "pixi-go" => "pixi add gopls",
        "pixi-cpp" => "pixi add clangd",
        _ => "pixi add <language-server>",
    }
}
