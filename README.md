# flowlog-zed

Zed editor support for **FlowLog** / Soufflé-flavoured Datalog (`.dl`) files:

- **Syntax highlighting** — via the [`tree-sitter-souffle`][tss] grammar.
- **Outline** — relations and type declarations in the outline panel / symbol search.
- **Diagnostics** — real parse + type errors from FlowLog's own compiler front end
  (`flowlog-build`), so the exact dialect is understood: `int32`, `IO="command"`,
  extended syntax, etc.

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

It is diagnostics-focused. Hover / go-to-definition / rename / completion are **not**
implemented (they would require a symbol index over the AST); highlighting and outline
come from tree-sitter.

> Note: `flowlog-build`'s parser/typechecker modules are `pub` but `#[doc(hidden)]`
> ("do not rely on these from external crates"), so this server may need updating when
> `flowlog-build` changes its internals.

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
