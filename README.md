# flowlog-zed

Zed editor support for **FlowLog** / Soufflé-flavoured Datalog (`.dl`) files:

- **Syntax highlighting** — via the [`tree-sitter-souffle`][tss] grammar (basic
  subset; see *Limitations*).
- **Diagnostics** — real parse + type errors from FlowLog's own compiler front end
  (`flowlog-build`), with automatic extended-mode retry for `loop`/`fixpoint`
  programs. Exact dialect: `int32`/`uint*`/`f64`/`bool`, `.extern fn`, loop blocks.
- **Hover, go-to-definition, find-references, rename** — for relations,
  `.extern fn` UDFs (jump from a bare `f(x)` call to its `.extern fn`), and
  `.type` aliases.
- **Completion** — relations, UDFs, types, directives, primitive types, keywords.
- **Document symbols** — relations / UDFs / types in outline + symbol search.

## Components

| Path | What it is |
|------|-----------|
| `extension.toml`, `src/lib.rs` | The Zed extension: registers the grammar + launches the language server. Compiled to a WASM component. |
| `languages/datalog/` | `config.toml`, `highlights.scm`, `outline.scm` — language config + tree-sitter queries. |
| `server/` | `flowlog-lsp` — a native stdio language server that runs `flowlog_build`'s parser + typechecker and publishes diagnostics. |

## Why a native server (no Java)

The only pre-existing LSP for this syntax is the Java [souffle-lsp-plugin][slp], which
speaks Soufflé — not FlowLog — and requires a JVM per session. This server instead
links `flowlog-build` directly and calls its real `Program::parse` +
`typechecker::check_program`, so diagnostics are exact for the FlowLog dialect with
no runtime dependency beyond the (native) binary.

Diagnostics use `flowlog-build` (with automatic extended-mode retry for
`loop`/`fixpoint` programs). Hover / go-to-definition / find-references / rename /
document-symbols / completion are powered by a symbol index built from FlowLog's
**own vendored pest grammar** (`server/src/flowlog.pest`) — dialect-exact, so
`.extern fn` UDFs, bare UDF calls, `.type` aliases, and `loop`/`fixpoint` blocks
all resolve correctly. (`flowlog-build`'s AST is sealed — `span()` is `pub(crate)` —
so it can't drive editor positions; the pest grammar can.)

### Limitations

- **Highlighting** uses the tree-sitter-souffle grammar, which parses the basic
  Datalog subset but **not** FlowLog extensions (`.extern fn`, `loop`/`fixpoint`,
  `@it`). Files using those get degraded highlighting on those lines. Full-dialect
  highlighting needs a FlowLog-specific tree-sitter grammar (future work). All the
  LSP features above are unaffected — they use the pest grammar, not tree-sitter.
- Symbol navigation is single-file and requires the buffer to parse (pest is
  all-or-nothing); diagnostics still report errors while the buffer is invalid.

> Note: `flowlog-build`'s parser/typechecker modules are `pub` but `#[doc(hidden)]`
> ("do not rely on these from external crates"), so the diagnostics path may need
> updating when `flowlog-build` changes its internals.

## Install

### 1. Build & install the language server

```sh
cd server
cargo build --release
cp target/release/flowlog-lsp ~/.local/bin/    # any directory on your $PATH
```

The extension finds the server via `$PATH` (`worktree.which("flowlog-lsp")`).

### 2. Install the Zed extension

From Zed: `cmd-shift-p` -> **zed: install dev extension** -> select this repo.
Zed compiles the WASM component and the grammar for you.

Open any `.dl` file. If the language isn't detected automatically, use
`cmd-shift-p` -> **Select Language** -> **Datalog**.

## Development

```sh
# language server
cd server && cargo build --release && ./target/release/flowlog-lsp --check path/to/prog.dl

# extension wasm (Zed normally does this on install)
cargo build --release --target wasm32-wasip1
```

## Credits

- Grammar: [langston-barrett/tree-sitter-souffle][tss]
- Compiler front end: [flowlog-rs/flowlog][fl] (`flowlog-build`)

[tss]: https://github.com/langston-barrett/tree-sitter-souffle
[slp]: https://github.com/jdaridis/souffle-lsp-plugin
[fl]: https://github.com/flowlog-rs/flowlog
