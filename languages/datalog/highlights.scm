; Comments
[
  (line_comment)
  (block_comment)
] @comment

; Directive / declaration keywords
[
  ".decl"
  ".input"
  ".output"
  ".printsize"
  ".limitsize"
  ".type"
  ".number_type"
  ".symbol_type"
  ".pragma"
  ".functor"
  ".comp"
  ".init"
  ".plan"
  ".override"
  ".extern"
  "fn"
  ".include"
  ".iterative"
] @keyword

; Relation qualifiers / storage & aggregator keywords
[
  "brie"
  "btree"
  "btree_delete"
  "eqrel"
  "inline"
  "magic"
  "no_inline"
  "no_magic"
  "override"
  "overridable"
  "choice-domain"
  "stateful"
  "as"
  "count"
  "sum"
  "min"
  "max"
  "mean"
  "range"
  "match"
  "contains"
  "fixpoint"
  "loop"
  "while"
  "until"
] @keyword

; Types
(primitive_type) @type.builtin
(attribute type: (qualified_name (ident) @type))
(as type: (qualified_name (ident) @type))

; Column / attribute names in declarations
(attribute var: (ident) @variable.parameter)

; Relation names
(relation_decl head: (ident) @function)
(atom relation: (qualified_name (ident) @function))
(directive relation: (qualified_name (ident) @function))

; Directive option keys (e.g. IO, delimiter)
(directive key: (ident) @property)

; Intrinsic functors
(intrinsic_functor) @function
(user_defined_functor (ident) @function)
(extern_fn name: (ident) @function)
(call_expr name: (ident) @function)

; Constants
(string) @string
(number) @number
(ipv4) @number
(bool) @boolean
(nil) @constant.builtin

; Variables & wildcard
(variable (ident) @variable)
(anonymous) @variable.special
(iteration_var) @variable.special

; Operators
":-" @operator
"<=" @operator
"=" @operator
"|" @operator
"<:" @operator
(negation) @operator
(comparison operator: _ @operator)
(binary_op operator: _ @operator)
(unary_op operator: _ @operator)
"->" @operator
(loop_connective) @keyword
(loop_iter_op) @operator

; Punctuation
[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
  "."
  ":"
] @punctuation.delimiter
