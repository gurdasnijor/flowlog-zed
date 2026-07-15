//! Relation-level symbol index built from the tree-sitter-souffle grammar.
//!
//! Powers hover / go-to-definition / find-references for relation names. This
//! is purely syntactic (relations declared via `.decl` and used in atoms /
//! directives) and independent of `flowlog-build` — the same grammar that
//! drives highlighting.
//!
//! NOTE: tree-sitter columns are byte offsets; LSP columns are UTF-16. This
//! maps them 1:1, which is correct for ASCII (all FlowLog identifiers/keywords
//! are ASCII).

use lsp_types::{Position, Range};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor};

const QUERY: &str = r#"
(relation_decl head: (ident) @def)
(atom relation: (qualified_name (ident) @use))
(directive relation: (qualified_name (ident) @use))
"#;

/// One occurrence of a relation name in the document.
pub struct Occurrence {
    pub name: String,
    pub range: Range,
    pub is_def: bool,
}

/// A relation declaration site + its `.decl` signature text.
pub struct Definition {
    pub range: Range,
    pub signature: String,
}

#[derive(Default)]
pub struct Index {
    pub defs: std::collections::HashMap<String, Vec<Definition>>,
    pub occurrences: Vec<Occurrence>,
}

impl Index {
    /// The relation-name occurrence whose range covers `pos`, if any.
    pub fn occurrence_at(&self, pos: Position) -> Option<&Occurrence> {
        self.occurrences.iter().find(|o| covers(o.range, pos))
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

/// The enclosing `.decl` text for a declaration-name node, for hover.
fn signature_of(mut node: Node, src: &str) -> String {
    while node.kind() != "relation_decl" {
        match node.parent() {
            Some(p) => node = p,
            None => break,
        }
    }
    node.utf8_text(src.as_bytes())
        .unwrap_or("")
        .split('\n')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Build the relation index for `src`. Returns an empty index if parsing fails.
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
        let idx = item.1;
        let cap = mat.captures[idx];
        let node = cap.node;
        let text = node.utf8_text(src.as_bytes()).unwrap_or("").to_string();
        if text.is_empty() {
            continue;
        }
        let range = node_range(node);
        match names[cap.index as usize] {
            "def" => {
                index
                    .defs
                    .entry(text.clone())
                    .or_default()
                    .push(Definition {
                        range,
                        signature: signature_of(node, src),
                    });
                index.occurrences.push(Occurrence {
                    name: text,
                    range,
                    is_def: true,
                });
            }
            "use" => {
                index.occurrences.push(Occurrence {
                    name: text,
                    range,
                    is_def: false,
                });
            }
            _ => {}
        }
    }
    index
}
