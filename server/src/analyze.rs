//! FlowLog source analysis: run FlowLog's real parser + typechecker over a
//! buffer and lower the resulting diagnostics into LSP `Diagnostic`s.
//!
//! `flowlog_build`'s parser reads from disk (`SourceMap::load`), so an
//! in-memory buffer is written to a temp sibling file first — keeping the
//! document's directory context (and thus `.include` resolution) intact.

use std::path::Path;

use codespan_reporting::diagnostic::{Diagnostic as CsDiag, LabelStyle, Severity};
use flowlog_build::common::{Config, Diagnostic as FlowDiag, ExecutionMode, FileId, SourceMap};
use flowlog_build::parser::Program;
use flowlog_build::typechecker::check_program;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

fn config_for(path: &str) -> Config {
    Config {
        program: path.to_string(),
        fact_dir: None,
        executable_path: None,
        output_dir: None,
        mode: ExecutionMode::default(),
        profile: false,
        sip: false,
        str_intern: false,
        udf_file: None,
        save_temps: false,
        include_dirs: Vec::new(),
    }
}

/// `SourceMap::line_col` is 1-based; LSP positions are 0-based.
/// NOTE: `character` is a byte offset within the line — correct for ASCII
/// (all FlowLog keywords/identifiers are ASCII); non-ASCII columns would need
/// UTF-16 remapping.
fn pos(sm: &SourceMap, file: FileId, byte: usize) -> Position {
    let (line, col) = sm.line_col(file, byte as u32);
    Position {
        line: line.saturating_sub(1),
        character: col.saturating_sub(1),
    }
}

fn severity(s: Severity) -> DiagnosticSeverity {
    match s {
        Severity::Bug | Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Note => DiagnosticSeverity::INFORMATION,
        Severity::Help => DiagnosticSeverity::HINT,
    }
}

fn cs_to_lsp(cs: &CsDiag<FileId>, sm: &SourceMap) -> Diagnostic {
    let label = cs
        .labels
        .iter()
        .find(|l| l.style == LabelStyle::Primary)
        .or_else(|| cs.labels.first());
    let range = match label {
        Some(l) => {
            let fid = l.file_id;
            Range {
                start: pos(sm, fid, l.range.start),
                end: pos(sm, fid, l.range.end),
            }
        }
        None => Range {
            start: Position::new(0, 0),
            end: Position::new(0, 0),
        },
    };
    let mut message = cs.message.clone();
    if let Some(l) = label {
        if !l.message.is_empty() {
            message = format!("{message}: {}", l.message);
        }
    }
    Diagnostic {
        range,
        severity: Some(severity(cs.severity)),
        message,
        source: Some("flowlog".to_string()),
        ..Default::default()
    }
}

/// Parse + typecheck `text` as the document at `orig_path`; return LSP
/// diagnostics. FlowLog's pipeline is fail-fast, so at most one error is
/// reported per pass (the next surfaces once the current is fixed).
pub fn analyze(orig_path: &Path, text: &str) -> Vec<Diagnostic> {
    let dir = orig_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = orig_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("buffer");
    let tmp = dir.join(format!(".{stem}.flowloglsp.{}.dl", std::process::id()));
    if std::fs::write(&tmp, text).is_err() {
        return Vec::new();
    }
    let tmp_str = tmp.to_string_lossy().to_string();
    let cfg = config_for(&tmp_str);
    let mut sm = SourceMap::new();
    let mut out = Vec::new();
    match Program::parse(&tmp_str, cfg.is_extended(), &mut sm) {
        Err(e) => out.push(cs_to_lsp(&e.to_diagnostic(), &sm)),
        Ok(mut program) => {
            if let Err(e) = check_program(&mut program, &cfg) {
                out.push(cs_to_lsp(&e.to_diagnostic(), &sm));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    out
}
