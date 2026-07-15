; Relation declarations form the document outline.
(relation_decl
  (ident) @name) @item

; Extern (UDF) function declarations.
(extern_fn
  name: (ident) @name) @item

; Type declarations (.type Name = ...).
(type_decl
  (type_synonym left: (ident) @name)) @item
(type_decl
  (subtype left: (ident) @name)) @item
(type_decl
  (type_union left: (ident) @name)) @item
(type_decl
  (legacy_bare_type_decl (ident) @name)) @item
