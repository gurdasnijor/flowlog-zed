//! FlowLog source analysis: run FlowLog's real parser + typechecker over a
//! buffer and lower the resulting diagnostics into LSP `Diagnostic`s.
//!
//! flowlog-parser's `parse` reads from disk (`SourceMap::load`), so an
//! in-memory buffer is written to a temp sibling file first — keeping the
//! document's directory context (and thus `.include` resolution) intact.
//!
//! Mode: we analyze in `DatalogBatch` first; if that reports the program needs
//! extended mode (explicit `loop`/`fixpoint` blocks), we re-analyze in
//! `ExtendBatch`.

use std::path::Path;

use codespan_reporting::diagnostic::{Diagnostic as CsDiag, LabelStyle, Severity};
use flowlog_common::{Config, Diagnostic as FlowDiag, ExecutionMode, FileId, SourceMap};
use flowlog_parser::parse;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

fn config_for(path: &str, mode: ExecutionMode) -> Config {
    Config {
        program: path.to_string(),
        mode,
        ..Default::default()
    }
}

/// `SourceMap::line_col` is 1-based; LSP positions are 0-based.
/// NOTE: `character` is a byte offset within the line — correct for ASCII.
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

fn run(orig_path: &Path, text: &str, mode: ExecutionMode) -> Vec<Diagnostic> {
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
    let mut cfg = config_for(&tmp_str, mode);
    let mut sm = SourceMap::new();
    let mut out = Vec::new();
    // `parse` runs the full pipeline (parse + typecheck + fold + prune); any
    // stage's failure — a type error included — surfaces as one `ParseError`.
    if let Err(e) = parse(&tmp_str, &[], &mut sm, &mut cfg) {
        out.push(cs_to_lsp(&e.to_diagnostic(), &sm));
    }
    let _ = std::fs::remove_file(&tmp);
    out
}

/// Parse + typecheck `text` as the document at `orig_path`; return LSP
/// diagnostics. FlowLog's pipeline is fail-fast, so at most one error is
/// reported per pass. Retries in extended mode if the program needs `loop`
/// blocks.
pub fn analyze(orig_path: &Path, text: &str) -> Vec<Diagnostic> {
    let diags = run(orig_path, text, ExecutionMode::DatalogBatch);
    if diags
        .iter()
        .any(|d| d.message.contains("extend-batch") || d.message.contains("extend-inc"))
    {
        return run(orig_path, text, ExecutionMode::ExtendBatch);
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doc(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn valid_program_is_clean() {
        let src = ".decl Edge(a: number, b: number)\n\
                   .decl Path(a: number, b: number)\n\
                   .output Path\n\
                   Path(a, b) :- Edge(a, b).\n";
        let diags = analyze(&doc("flowlog_ok.dl"), src);
        assert!(diags.is_empty(), "expected clean, got {diags:?}");
    }

    #[test]
    fn raw_string_fact_is_accepted() {
        // Raw strings are a main-next grammar construct absent from published
        // flowlog-build 0.3.4; the diagnostics path must accept them now.
        let src = ".decl Msg(s: symbol)\n\
                   .output Msg\n\
                   Msg(r\"hi\").\n";
        let diags = analyze(&doc("flowlog_raw.dl"), src);
        assert!(
            diags.is_empty(),
            "raw string should parse+typecheck, got {diags:?}"
        );
    }

    #[test]
    fn undeclared_relation_is_reported() {
        // A real error must still surface — proves the typechecker runs.
        let src = ".decl A(x: number)\n\
                   .output A\n\
                   A(x) :- B(x).\n";
        let diags = analyze(&doc("flowlog_err.dl"), src);
        assert!(!diags.is_empty(), "undeclared relation B should error");
    }
}
