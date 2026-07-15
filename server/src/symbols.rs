//! Symbol index built from FlowLog's own (vendored) pest grammar.
//!
//! Dialect-exact: understands `.decl` relations, `.extern fn` UDFs (declared
//! with `.extern fn` and *called bare* as `name(args)`), `.type` aliases, and
//! all extended constructs (`loop`/`fixpoint`) — none of which the
//! tree-sitter-flowlog grammar can parse. Powers hover / go-to-definition /
//! find-references / rename / document-symbols.
//!
//! pest positions are 1-based (line, col); LSP positions are 0-based. Columns
//! are code points, mapped 1:1 to UTF-16 — correct for ASCII (FlowLog
//! identifiers are ASCII).
//!
//! Note: pest is all-or-nothing — on a syntax error the index is empty and
//! navigation is unavailable until the buffer parses again (diagnostics, from
//! flowlog-parser, still report the error).

use std::collections::{HashMap, HashSet};

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
        Rule::call_expr => {
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
    // Bare body UDF calls (`is_ok(x, y)` as a filter) parse as relation refs;
    // FlowLog resolves calls by name, so reclassify any relation-use whose name
    // has an `.extern fn` declaration to a Functor use — enabling UDF
    // go-to-definition / hover / references on bare body calls.
    let functor_names: HashSet<String> = index
        .defs
        .keys()
        .filter(|(k, _)| *k == Kind::Functor)
        .map(|(_, n)| n.clone())
        .collect();
    if !functor_names.is_empty() {
        for occ in &mut index.occurrences {
            if occ.kind == Kind::Relation && functor_names.contains(&occ.name) {
                occ.kind = Kind::Functor;
            }
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_relation_defs_and_uses() {
        let src = ".decl Edge(a: number, b: number)\n\
                   .decl Path(a: number, b: number)\n\
                   .output Path\n\
                   Path(a, b) :- Edge(a, b).\n";
        let idx = build(src);
        assert!(idx.defs_of(Kind::Relation, "Edge").is_some(), "Edge def");
        assert!(idx.defs_of(Kind::Relation, "Path").is_some(), "Path def");
        assert!(
            idx.occurrences
                .iter()
                .any(|o| o.kind == Kind::Relation && o.name == "Edge" && !o.is_def),
            "Edge body use"
        );
    }

    #[test]
    fn indexes_extern_fn_def_and_call() {
        // `ok(x) = 1` -> `ok` is a `call_expr` (the renamed `fn_call_expr`);
        // its use must be classified as a Functor, resolving to the decl.
        let src = ".decl A(x: number)\n\
                   .decl B(x: number)\n\
                   .extern fn ok(x: number) -> number\n\
                   .output A\n\
                   A(x) :- B(x), ok(x) = 1.\n";
        let idx = build(src);
        assert!(idx.defs_of(Kind::Functor, "ok").is_some(), "ok fn def");
        assert!(
            idx.occurrences
                .iter()
                .any(|o| o.kind == Kind::Functor && o.name == "ok" && !o.is_def),
            "ok fn use"
        );
    }

    #[test]
    fn parses_main_next_constructs_for_indexing() {
        // Raw strings are a main-next construct; pest is all-or-nothing, so a
        // non-empty index proves the vendored grammar parses them.
        let src = ".decl Msg(s: symbol)\n\
                   .output Msg\n\
                   Msg(r\"hi\").\n";
        let idx = build(src);
        assert!(
            idx.defs_of(Kind::Relation, "Msg").is_some(),
            "Msg def under raw-string program"
        );
    }
}
