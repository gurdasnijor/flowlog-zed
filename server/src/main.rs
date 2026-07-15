//! FlowLog Datalog language server.
//!
//! - Diagnostics: `flowlog-build`'s real parser + typechecker (see `analyze`),
//!   with automatic extended-mode retry for `loop`/`fixpoint` programs.
//! - Hover / go-to-definition / find-references / rename / document-symbols /
//!   completion for relations (and `.type` types): a tree-sitter symbol index
//!   (see `symbols`).
//!
//! Speaks LSP over stdio; also supports `--check <file>` for a one-shot CLI
//! diagnostic dump (used for smoke tests).

mod analyze;
mod symbols;

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

use analyze::analyze;
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, Location, MarkupContent, MarkupKind, OneOf, Position,
    PublishDiagnosticsParams, RenameParams, ServerCapabilities, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url, WorkspaceEdit,
};

use symbols::Kind;

type Res = Result<(), Box<dyn Error + Sync + Send>>;

const DIRECTIVES: &[&str] = &[
    ".decl",
    ".input",
    ".output",
    ".printsize",
    ".include",
    ".extern",
    ".type",
    ".pragma",
];
const TYPES: &[&str] = &[
    "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "f32", "f64",
    "string", "bool",
];
const KEYWORDS: &[&str] = &["fixpoint", "loop", "while", "until", "True", "False"];

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
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    })?;
    let _init_params = connection.initialize(capabilities)?;
    main_loop(&connection)?;
    io_threads.join()?;
    Ok(())
}

fn main_loop(conn: &Connection) -> Res {
    let mut docs: HashMap<Url, String> = HashMap::new();
    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn.handle_shutdown(&req)? {
                    return Ok(());
                }
                let resp = handle_request(req, &docs);
                conn.sender.send(Message::Response(resp))?;
            }
            Message::Notification(not) => match not.method.as_str() {
                "textDocument/didOpen" => {
                    let p: DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
                    docs.insert(p.text_document.uri.clone(), p.text_document.text.clone());
                    publish(conn, p.text_document.uri, &p.text_document.text)?;
                }
                "textDocument/didChange" => {
                    let p: DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
                    if let Some(change) = p.content_changes.into_iter().last() {
                        docs.insert(p.text_document.uri.clone(), change.text.clone());
                        publish(conn, p.text_document.uri, &change.text)?;
                    }
                }
                "textDocument/didSave" => {
                    let p: DidSaveTextDocumentParams = serde_json::from_value(not.params)?;
                    if let Some(text) = p.text {
                        docs.insert(p.text_document.uri.clone(), text.clone());
                        publish(conn, p.text_document.uri, &text)?;
                    }
                }
                "textDocument/didClose" => {
                    let p: DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
                    docs.remove(&p.text_document.uri);
                }
                _ => {}
            },
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn ok_response<T: serde::Serialize>(id: lsp_server::RequestId, value: T) -> Response {
    Response {
        id,
        result: Some(serde_json::to_value(value).unwrap_or(serde_json::Value::Null)),
        error: None,
    }
}

fn null_response(id: lsp_server::RequestId) -> Response {
    Response {
        id,
        result: Some(serde_json::Value::Null),
        error: None,
    }
}

fn handle_request(req: Request, docs: &HashMap<Url, String>) -> Response {
    let id = req.id.clone();
    match req.method.as_str() {
        "textDocument/hover" => match serde_json::from_value::<HoverParams>(req.params) {
            Ok(p) => {
                let loc = p.text_document_position_params;
                hover(docs, &loc.text_document.uri, loc.position)
                    .map(|h| ok_response(id.clone(), h))
                    .unwrap_or_else(|| null_response(id))
            }
            Err(_) => null_response(id),
        },
        "textDocument/definition" => {
            match serde_json::from_value::<GotoDefinitionParams>(req.params) {
                Ok(p) => {
                    let loc = p.text_document_position_params;
                    definition(docs, &loc.text_document.uri, loc.position)
                        .map(|d| ok_response(id.clone(), d))
                        .unwrap_or_else(|| null_response(id))
                }
                Err(_) => null_response(id),
            }
        }
        "textDocument/references" => {
            match serde_json::from_value::<lsp_types::ReferenceParams>(req.params) {
                Ok(p) => {
                    let include_decl = p.context.include_declaration;
                    let loc = p.text_document_position;
                    ok_response(
                        id,
                        references(docs, &loc.text_document.uri, loc.position, include_decl),
                    )
                }
                Err(_) => null_response(id),
            }
        }
        "textDocument/rename" => match serde_json::from_value::<RenameParams>(req.params) {
            Ok(p) => {
                let loc = p.text_document_position;
                rename(docs, &loc.text_document.uri, loc.position, &p.new_name)
                    .map(|w| ok_response(id.clone(), w))
                    .unwrap_or_else(|| null_response(id))
            }
            Err(_) => null_response(id),
        },
        "textDocument/documentSymbol" => {
            match serde_json::from_value::<DocumentSymbolParams>(req.params) {
                Ok(p) => ok_response(id, document_symbols(docs, &p.text_document.uri)),
                Err(_) => null_response(id),
            }
        }
        "textDocument/completion" => match serde_json::from_value::<CompletionParams>(req.params) {
            Ok(p) => {
                let uri = p.text_document_position.text_document.uri;
                ok_response(id, completion(docs, &uri))
            }
            Err(_) => null_response(id),
        },
        _ => null_response(id),
    }
}

fn hover(docs: &HashMap<Url, String>, uri: &Url, pos: Position) -> Option<Hover> {
    let text = docs.get(uri)?;
    let index = symbols::build(text);
    let occ = index.occurrence_at(pos)?;
    let defs = index.defs_of(occ.kind, &occ.name)?;
    let sigs: Vec<String> = defs.iter().map(|d| d.signature.clone()).collect();
    if sigs.is_empty() {
        return None;
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```flowlog\n{}\n```", sigs.join("\n")),
        }),
        range: Some(occ.range),
    })
}

fn definition(
    docs: &HashMap<Url, String>,
    uri: &Url,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    let text = docs.get(uri)?;
    let index = symbols::build(text);
    let occ = index.occurrence_at(pos)?;
    let defs = index.defs_of(occ.kind, &occ.name)?;
    let locations: Vec<Location> = defs
        .iter()
        .map(|d| Location {
            uri: uri.clone(),
            range: d.range,
        })
        .collect();
    if locations.is_empty() {
        return None;
    }
    Some(GotoDefinitionResponse::Array(locations))
}

fn references(
    docs: &HashMap<Url, String>,
    uri: &Url,
    pos: Position,
    include_decl: bool,
) -> Vec<Location> {
    let Some(text) = docs.get(uri) else {
        return Vec::new();
    };
    let index = symbols::build(text);
    let Some(occ) = index.occurrence_at(pos) else {
        return Vec::new();
    };
    let (kind, name) = (occ.kind, occ.name.clone());
    index
        .occurrences
        .iter()
        .filter(|o| o.kind == kind && o.name == name && (include_decl || !o.is_def))
        .map(|o| Location {
            uri: uri.clone(),
            range: o.range,
        })
        .collect()
}

fn rename(
    docs: &HashMap<Url, String>,
    uri: &Url,
    pos: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    let text = docs.get(uri)?;
    let index = symbols::build(text);
    let occ = index.occurrence_at(pos)?;
    let (kind, name) = (occ.kind, occ.name.clone());
    let edits: Vec<TextEdit> = index
        .occurrences
        .iter()
        .filter(|o| o.kind == kind && o.name == name)
        .map(|o| TextEdit {
            range: o.range,
            new_text: new_name.to_string(),
        })
        .collect();
    if edits.is_empty() {
        return None;
    }
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn document_symbols(docs: &HashMap<Url, String>, uri: &Url) -> DocumentSymbolResponse {
    let Some(text) = docs.get(uri) else {
        return DocumentSymbolResponse::Nested(Vec::new());
    };
    let index = symbols::build(text);
    let mut syms: Vec<DocumentSymbol> = Vec::new();
    for ((kind, name), defs) in &index.defs {
        for d in defs {
            #[allow(deprecated)]
            syms.push(DocumentSymbol {
                name: name.clone(),
                detail: Some(d.signature.clone()),
                kind: match kind {
                    Kind::Relation => SymbolKind::STRUCT,
                    Kind::Functor => SymbolKind::FUNCTION,
                    Kind::Type => SymbolKind::INTERFACE,
                },
                tags: None,
                deprecated: None,
                range: d.range,
                selection_range: d.range,
                children: None,
            });
        }
    }
    syms.sort_by_key(|s| (s.range.start.line, s.range.start.character));
    DocumentSymbolResponse::Nested(syms)
}

fn completion(docs: &HashMap<Url, String>, uri: &Url) -> CompletionResponse {
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut push = |label: &str, kind: CompletionItemKind, detail: &str| {
        items.push(CompletionItem {
            label: label.to_string(),
            kind: Some(kind),
            detail: Some(detail.to_string()),
            ..Default::default()
        });
    };
    for d in DIRECTIVES {
        push(d, CompletionItemKind::KEYWORD, "directive");
    }
    for t in TYPES {
        push(t, CompletionItemKind::TYPE_PARAMETER, "type");
    }
    for k in KEYWORDS {
        push(k, CompletionItemKind::KEYWORD, "keyword");
    }
    if let Some(text) = docs.get(uri) {
        let index = symbols::build(text);
        for (kind, name) in index.defs.keys() {
            let (ck, detail) = match kind {
                Kind::Relation => (CompletionItemKind::STRUCT, "relation"),
                Kind::Functor => (CompletionItemKind::FUNCTION, "functor"),
                Kind::Type => (CompletionItemKind::INTERFACE, "type"),
            };
            push(name, ck, detail);
        }
    }
    CompletionResponse::Array(items)
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
