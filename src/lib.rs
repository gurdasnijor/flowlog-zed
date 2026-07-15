//! Zed extension entry point for FlowLog Datalog.
//!
//! Registers the `souffle` tree-sitter grammar (highlighting/outline) and
//! launches the native `flowlog-lsp` server, which is expected on `$PATH`
//! (install it with `cargo install --path server` or copy the release binary
//! into a `$PATH` dir such as `~/.local/bin`).

use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

const SERVER_BIN: &str = "flowlog-lsp";

struct FlowLogExtension;

impl zed::Extension for FlowLogExtension {
    fn new() -> Self {
        FlowLogExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let command = worktree.which(SERVER_BIN).ok_or_else(|| {
            format!(
                "`{SERVER_BIN}` not found on PATH. Build & install it: \
                 `cargo install --path server` or copy \
                 `server/target/release/{SERVER_BIN}` into a PATH directory."
            )
        })?;
        Ok(Command {
            command,
            args: Vec::new(),
            env: Vec::new(),
        })
    }
}

zed::register_extension!(FlowLogExtension);
