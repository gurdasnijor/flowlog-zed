//! Symbol index built from the tree-sitter-souffle grammar.
//!
//! Indexes three kinds of named symbols and their occurrences, powering
//! hover / go-to-definition / find-references / rename / document-symbols:
//!
//! - **Relations** — declared by `.decl`, used in atoms and directives.
//! - **Functors (UDFs)** — declared by `.functor`, used as `@name(...)`.
//! - **Types** — declared by `.type`, used in attribute type positions.
//!
//! Purely syntactic and independent of `flowlog-build` — the same grammar that
//! drives highlighting.
//!
//! NOTE: tree-sitter columns are byte offsets; LSP columns are UTF-16. Mapped
//! 1:1, correct for ASCII (all FlowLog identifiers/keywords are ASCII).

use std::collections::HashMap;

use lsp_types::{Position, Range};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Relation,
    Functor,
    Type,
}

const QUERY: &str = r#"
(relation_decl head: (ident) @rel.def)
(atom relation: (qualified_name (ident) @rel.use))
(directive relation: (qualified_name (ident) @rel.use))

(functor_decl name: (ident) @fn.def)
(user_defined_functor name: (ident) @fn.use)

(subtype left: (ident) @type.def)
(type_synonym left: (ident) @type.def)
(type_union left: (ident) @type.def)
(type_record left: (ident) @type.def)
(adt left: (ident) @type.def)
(legacy_bare_type_decl (ident) @type.def)
(attribute type: (qualified_name (ident) @type.use))
(as type: (qualified_name (ident) @type.use))
"#;

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
    /// The occurrence whose range covers `pos`, if any.
    pub fn occurrence_at(&self, pos: Position) -> Option<&Occurrence> {
        self.occurrences.iter().find(|o| covers(o.range, pos))
    }

    /// Definitions for a resolved symbol.
    pub fn defs_of(&self, kind: Kind, name: &str) -> Option<&Vec<Definition>> {
        self.defs.get(&(kind, name.to_string()))
    }
}

fn covers(r: Range, p: Position) -> bool {
    r.start.line == p.line
        && r.end.line == p.line
        && r.start.character <= p.character
        && p.character <= r.end.character
}

fn node_range(n: Node) -> Range {
    let s = n.start_position();
    let e = n.end_position();
    Range {
        start: Position::new(s.row as u32, s.column as u32),
        end: Position::new(e.row as u32, e.column as u32),
    }
}

/// First line of the enclosing declaration, for hover signatures.
fn signature_of(mut node: Node, src: &str) -> String {
    loop {
        match node.kind() {
            "relation_decl" | "functor_decl" | "type_decl" | "legacy_type_decl" => break,
            _ => match node.parent() {
                Some(p) => node = p,
                None => break,
            },
        }
    }
    node.utf8_text(src.as_bytes())
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn classify(capture_name: &str) -> Option<(Kind, bool)> {
    let (prefix, role) = capture_name.split_once('.')?;
    let kind = match prefix {
        "rel" => Kind::Relation,
        "fn" => Kind::Functor,
        "type" => Kind::Type,
        _ => return None,
    };
    Some((kind, role == "def"))
}

/// Build the symbol index for `src`. Empty index if parsing fails.
pub fn build(src: &str) -> Index {
    let mut index = Index::default();
    let language = tree_sitter_souffle::LANGUAGE.into();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return index;
    }
    let Some(tree) = parser.parse(src, None) else {
        return index;
    };
    let Ok(query) = Query::new(&language, QUERY) else {
        return index;
    };
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut caps = cursor.captures(&query, tree.root_node(), src.as_bytes());
    while let Some(item) = caps.next() {
        let mat = &item.0;
        let cap = mat.captures[item.1];
        let node = cap.node;
        let Some((kind, is_def)) = classify(names[cap.index as usize]) else {
            continue;
        };
        let text = node.utf8_text(src.as_bytes()).unwrap_or("").to_string();
        if text.is_empty() {
            continue;
        }
        let range = node_range(node);
        if is_def {
            index
                .defs
                .entry((kind, text.clone()))
                .or_default()
                .push(Definition {
                    range,
                    signature: signature_of(node, src),
                });
        }
        index.occurrences.push(Occurrence {
            kind,
            name: text,
            range,
            is_def,
        });
    }
    index
}
