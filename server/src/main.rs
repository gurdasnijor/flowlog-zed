//! FlowLog Datalog language server.
//!
//! Diagnostics-focused LSP backed by `flowlog-build`'s parser + typechecker.
//! Speaks LSP over stdio; also supports `--check <file>` for a one-shot CLI
//! diagnostic dump (used for smoke tests).

mod analyze;

use std::error::Error;
use std::path::Path;

use analyze::analyze;
use lsp_server::{Connection, Message, Notification};
use lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    Url,
};

type Res = Result<(), Box<dyn Error + Sync + Send>>;

fn main() -> Res {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--check") {
        let path = args
            .get(i + 1)
            .ok_or("--check requires a <file> argument")?;
        let text = std::fs::read_to_string(path)?;
        let diags = analyze(Path::new(path), &text);
        if diags.is_empty() {
            println!("OK: no diagnostics for {path}");
        }
        for d in &diags {
            println!(
                "{}:{}-{}:{} [{:?}] {}",
                d.range.start.line + 1,
                d.range.start.character + 1,
                d.range.end.line + 1,
                d.range.end.character + 1,
                d.severity.unwrap_or(lsp_types::DiagnosticSeverity::ERROR),
                d.message
            );
        }
        return Ok(());
    }
    run_lsp()
}

fn run_lsp() -> Res {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        ..Default::default()
    })?;
    let _init_params = connection.initialize(capabilities)?;
    main_loop(&connection)?;
    io_threads.join()?;
    Ok(())
}

fn main_loop(conn: &Connection) -> Res {
    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn.handle_shutdown(&req)? {
                    return Ok(());
                }
            }
            Message::Notification(not) => match not.method.as_str() {
                "textDocument/didOpen" => {
                    let p: DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
                    publish(conn, p.text_document.uri, &p.text_document.text)?;
                }
                "textDocument/didChange" => {
                    let p: DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
                    // FULL sync: the last change carries the whole document.
                    if let Some(change) = p.content_changes.into_iter().last() {
                        publish(conn, p.text_document.uri, &change.text)?;
                    }
                }
                "textDocument/didSave" => {
                    let p: DidSaveTextDocumentParams = serde_json::from_value(not.params)?;
                    if let Some(text) = p.text {
                        publish(conn, p.text_document.uri, &text)?;
                    }
                }
                _ => {}
            },
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn publish(conn: &Connection, uri: Url, text: &str) -> Res {
    let diagnostics = match uri.to_file_path() {
        Ok(path) => analyze(&path, text),
        Err(_) => Vec::new(),
    };
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version: None,
    };
    conn.sender.send(Message::Notification(Notification {
        method: "textDocument/publishDiagnostics".to_string(),
        params: serde_json::to_value(params)?,
    }))?;
    Ok(())
}
