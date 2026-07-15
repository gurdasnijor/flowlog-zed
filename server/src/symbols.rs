//! Symbol index built from FlowLog's own (vendored) pest grammar.
//!
//! Dialect-exact: understands `.decl` relations, `.extern fn` UDFs (declared
//! with `.extern fn` and *called bare* as `name(args)`), `.type` aliases, and
//! all extended constructs (`loop`/`fixpoint`) — none of which the
//! tree-sitter-souffle grammar can parse. Powers hover / go-to-definition /
//! find-references / rename / document-symbols.
//!
//! pest positions are 1-based (line, col); LSP positions are 0-based. Columns
//! are code points, mapped 1:1 to UTF-16 — correct for ASCII (FlowLog
//! identifiers are ASCII).
//!
//! Note: pest is all-or-nothing — on a syntax error the index is empty and
//! navigation is unavailable until the buffer parses again (diagnostics, from
//! flowlog-build, still report the error).

use std::collections::HashMap;

use lsp_types::{Position, Range};
use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "flowlog.pest"]
struct FlowLogParser;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Relation,
    Functor,
    Type,
}

pub struct Occurrence {
    pub kind: Kind,
    pub name: String,
    pub range: Range,
    pub is_def: bool,
}

pub struct Definition {
    pub range: Range,
    pub signature: String,
}

#[derive(Default)]
pub struct Index {
    pub defs: HashMap<(Kind, String), Vec<Definition>>,
    pub occurrences: Vec<Occurrence>,
}

impl Index {
    pub fn occurrence_at(&self, pos: Position) -> Option<&Occurrence> {
        self.occurrences.iter().find(|o| covers(o.range, pos))
    }

    pub fn defs_of(&self, kind: Kind, name: &str) -> Option<&Vec<Definition>> {
        self.defs.get(&(kind, name.to_string()))
    }

    fn add_def(&mut self, kind: Kind, name: String, range: Range, signature: String) {
        self.defs
            .entry((kind, name.clone()))
            .or_default()
            .push(Definition { range, signature });
        self.occurrences.push(Occurrence {
            kind,
            name,
            range,
            is_def: true,
        });
    }

    fn add_use(&mut self, kind: Kind, name: String, range: Range) {
        self.occurrences.push(Occurrence {
            kind,
            name,
            range,
            is_def: false,
        });
    }
}

fn covers(r: Range, p: Position) -> bool {
    r.start.line == p.line
        && r.end.line == p.line
        && r.start.character <= p.character
        && p.character <= r.end.character
}

type Pair<'a> = pest::iterators::Pair<'a, Rule>;

fn span_range(span: pest::Span) -> Range {
    let (sl, sc) = span.start_pos().line_col();
    let (el, ec) = span.end_pos().line_col();
    Range {
        start: Position::new(sl.saturating_sub(1) as u32, sc.saturating_sub(1) as u32),
        end: Position::new(el.saturating_sub(1) as u32, ec.saturating_sub(1) as u32),
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// First direct child matching `target`.
fn first_child<'a>(pair: &Pair<'a>, target: Rule) -> Option<Pair<'a>> {
    pair.clone().into_inner().find(|p| p.as_rule() == target)
}

fn walk(pair: Pair, index: &mut Index) {
    match pair.as_rule() {
        Rule::declaration => {
            let sig = first_line(pair.as_str());
            if let Some(name) = first_child(&pair, Rule::relation_name) {
                index.add_def(
                    Kind::Relation,
                    name.as_str().to_string(),
                    span_range(name.as_span()),
                    sig,
                );
            }
            for child in pair.into_inner() {
                if child.as_rule() != Rule::relation_name {
                    walk(child, index);
                }
            }
        }
        Rule::extern_fn => {
            let sig = first_line(pair.as_str());
            if let Some(name) = first_child(&pair, Rule::identifier) {
                index.add_def(
                    Kind::Functor,
                    name.as_str().to_string(),
                    span_range(name.as_span()),
                    sig,
                );
            }
            for child in pair.into_inner() {
                if child.as_rule() != Rule::identifier {
                    walk(child, index);
                }
            }
        }
        Rule::type_alias_decl => {
            let sig = first_line(pair.as_str());
            if let Some(name) = first_child(&pair, Rule::identifier) {
                index.add_def(
                    Kind::Type,
                    name.as_str().to_string(),
                    span_range(name.as_span()),
                    sig,
                );
            }
            for child in pair.into_inner() {
                if child.as_rule() != Rule::identifier {
                    walk(child, index);
                }
            }
        }
        Rule::fn_call_expr => {
            if let Some(name) = first_child(&pair, Rule::identifier) {
                index.add_use(
                    Kind::Functor,
                    name.as_str().to_string(),
                    span_range(name.as_span()),
                );
            }
            for child in pair.into_inner() {
                if child.as_rule() != Rule::identifier {
                    walk(child, index);
                }
            }
        }
        Rule::relation_ref => {
            index.add_use(
                Kind::Relation,
                pair.as_str().to_string(),
                span_range(pair.as_span()),
            );
        }
        Rule::alias_name => {
            index.add_use(
                Kind::Type,
                pair.as_str().to_string(),
                span_range(pair.as_span()),
            );
        }
        _ => {
            for child in pair.into_inner() {
                walk(child, index);
            }
        }
    }
}

/// Build the symbol index for `src`. Empty index if the buffer does not parse.
pub fn build(src: &str) -> Index {
    let mut index = Index::default();
    if let Ok(mut pairs) = FlowLogParser::parse(Rule::main_grammar, src) {
        if let Some(root) = pairs.next() {
            walk(root, &mut index);
        }
    }
    index
}
