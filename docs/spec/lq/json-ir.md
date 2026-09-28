# LQ JSON IR (grammar version 1)

| | |
|---|---|
| Title | The LQ JSON IR: the S-AST written as JSON, its mapping rules, validation and diagnostics, its JSON Schema, and its output form |
| Chapter | [LQ/json-ir], `docs/spec/lq/json-ir.md` |
| Status | draft, pass 1 pending |
| Work package | WP-19a (R-SPEC-F), part of WP-19 ([PLAN §3.2] item 1); closes the [PLAN §3.3] gap "JSON IR schema" |
| Sources | [50 §0.3] decision 1 (no JSON AST for agents); [50 §4.2] ("the JSON op batch of `moirai apply` is the JSON form of the same IR … (`--ast` prints it)"); [50 §5.1] pipeline (text, named query, JSON IR); [50 §5.2] error JSON; [50 §7.4] items 3 and 7; [50 §8.2] LQ-Bench row ("LQ, LQ with strict GQL spellings, and the JSON IR as input"); [60 §3.1] exit (the alternative surfaces are ablations); [90 §6.6] MPSP rules 3–5; [90 §7.2]; [AR §7.1] (`apply`); [AR §7.7.2]; [PLAN §2.2] (`moirai-lqbench`: "JSON IR → model AST converter"), WP-71b, WP-72 |
| Depends on | [LQ/canonical-ast] (the S-AST catalogue this IR writes as JSON, §3.2–§3.4), [LQ/grammar-v1.ebnf] (Annex P, R, G), [LQ/lexical] (literal forms and limits, §5, §8; positions, §2.3; JSON strings, §11.1), [LQ/errors] (texts), [F01] (UTF-8, §6.1) |

## 1. What the JSON IR is

The JSON IR is the **S-AST** of [LQ/canonical-ast §3] written as JSON: one JSON object per S-AST node, with the node's
tag and fields. It is not a second language. An IR document enters the pipeline where the parser's output would
([LQ/canonical-ast §2]); binding, the C-AST and every hash are then exactly those of the equivalent text, so an LQ text
and its IR have the same query hash.

It serves four purposes:

| Use | Who | Source |
|---|---|---|
| the **alternative input surface** "the JSON IR as input", an LQ-Bench ablation (not a gate) | the LQ-Bench harness at M0 (WP-71b, WP-72), on LQ-3 | [50 §8.2], [60 §3.1] exit, [50 §7.4] |
| the output of `--ast` (the tree of a query, a `TX` or a verb's expansion) | the CLI (M8); LQ-3 for fixtures | [50 §4.2] |
| the IR that the `apply` op batches lower into | the CLI's `apply` (M8), whose record format is not defined here (§9) | [50 §4.2], [AR §7.1], [90 §6.6] |
| the input of `moirai-lqbench`'s converter into the model's AST (the model has no JSON code) | R-BENCH (WP-71b) | [PLAN §2.2], [PLAN §3.2] item 9 |

Agents are not taught the IR: [50 §0.3] decision 1 keeps "no JSON AST for agents". LQ-Bench measures it as one of the
two alternative surfaces, and the ablation's result changes nothing in the grammar by itself ([60 §3.1]: "There is no
'within 5 points of the best candidate' gate").

## 2. Documents and transport

1. A JSON IR document is a UTF-8 JSON text (RFC 8259) whose value is one object: a `read`, `tx` or `define` node (the
   S-AST roots). A `read` document plays the role of `read_input`, a `tx` document of `write_input`, a `define`
   document of a standard-library definition ([LQ/grammar-v1.ebnf §P.1]).
2. The root may carry `"lq": 1`, the grammar version; an absent `lq` means 1. Any other value is E001. `lq` appears on
   the root only; inside the tree it is E001.
3. **Parameters travel beside the document**, exactly as they do for LQ text (`params`, `-p k=v`, [50 §6.1]); a
   document references them with `param` nodes and never contains their values.
4. **Transport as a string.** Wherever a tool carries an IR document, it carries it as one string property, because the
   portable MCP schema profile allows no object-typed property ([90 §6.6] rule 3). The JSON Schema of §6 validates the
   document itself, never a tool's `inputSchema`, so it may use `$ref`, `oneOf` and `const`, which MPSP forbids only in
   `tools/list` (Open point J-7).
5. At M0 the only reader is the LQ-Bench harness (WP-71b). The product's doors for IR input are M8's (§9).
6. An object must not repeat a key (E001). Key order does not matter. A key the tag does not have is E001.
7. **Nesting limit, checked while parsing** (pass 1, P1-41). The JSON decoder counts the depth of nested objects and arrays
   as it reads and refuses a document whose depth exceeds **256** with E001, at the byte where the 257th level opens,
   before any tree is built. The limit of [LQ/grammar-v1.ebnf] P13 (nesting depth 64 of LQ constructs) is checked afterwards on
   the tree (§5.2); 256 JSON levels hold every tree P13 admits (each LQ level is at most a node object and one array), so the
   first limit never refuses a document the second would accept, and a deeply nested document can no longer exhaust the
   decoder's stack.

## 3. Mapping rules

### 3.1 Nodes

A node is `{"t": "<tag>", <field>: <value>, …}` with the tag and the field names of [LQ/canonical-ast §3.2]. Any node
may also carry `"at": [start, end]`, the node's byte span in the source text ([LQ/lexical §2.3]): `--ast` prints it
and a reader ignores it.

### 3.2 Field values

| Field type in [LQ/canonical-ast §3.2] | JSON value | When omitted |
|---|---|---|
| `bool` | `true` or `false` | false |
| `int` | a JSON number written without fraction or exponent, in the range the catalogue gives | required |
| `str` | a string | required |
| `name` | a non-empty string | required |
| `enum{…}` | one of the listed names, as a string | required; for an enum with a default, the default |
| `X?` | the value, or `null` | absent (see §3.4) |
| `[X]` | an array | the empty list |
| `[X]+` | a non-empty array | required |
| a node or a union | an object, or a shorthand of §3.3 where the union allows one | required |

A name is the decoded text a back-quoted identifier would give: any string, keywords included, with no back-quotes.

### 3.3 Shorthands for literals

In a position whose type is `expr`, `argval` or `lit`, and in the elements of a list of those, a JSON scalar or array
stands for a literal node:

| JSON value | S-AST node |
|---|---|
| a string | `str` with that value |
| a number written without fraction or exponent | `int`; with a leading `-`, `neg` of the `int` of the rest |
| a number written with a fraction or an exponent | `float` whose text is the number's text; with a leading `-`, `neg` of the `float` of the rest |
| `true`, `false` | `bool` |
| `null` | the `null` literal (but see §3.4) |
| an array | `list` of the elements, each read by this table or as a node |

The explicit objects (`{"t": "int", "v": 5}`, `{"t": "str", "v": "x"}`, `{"t": "list", "elems": […]}`) are accepted
as well. A reader must keep a number's text (Open point J-4): `1.0` and `1` are different S-AST nodes (a `float` and an
`int`), and a `float` node carries its text ([LQ/canonical-ast §3.1] item 13). JSON's number syntax is a subset of
LQ's integer and float syntax once the sign is removed, so every JSON number has an LQ spelling.

### 3.4 `null`

In an optional field (`X?`), `null` means absent. To place the `NULL` literal in an optional expression field — the
default of a `pdecl` (`$scope: node? = NULL`), a `case`'s `else` — write `{"t": "null"}`. Everywhere else `null` is the
`NULL` literal.

### 3.5 Revisions

A revision is written with the revision nodes (`rhead`, `rref`, `rcommit`, `rseq`, `rsuf`, `rrange`, `rlist`) or a
`param`, never as revision text: `main...lane/l10` is
`{"t": "rrange", "from": {"t": "rref", "name": "main"}, "op": "three", "to": {"t": "rref", "name": "lane/l10"}}`.
`rsuf.time` is the normalised text `YYYY-MM-DDTHH:MM:SSZ`. A revision node is allowed only where the text form reads
revision mode ([LQ/lexical §4.2]); elsewhere it is E001. A string in an argument position that is a revision position
is an `expr`, which the binder coerces to a revision, as for text.

### 3.6 Spellings

The IR has no spellings to choose between: it writes the normalised S-AST directly. The strict-GQL spelling mode of
[LQ/grammar-v1.ebnf §G] does not apply to it.

## 4. Node reference

The object for tag T has exactly the fields of row T of [LQ/canonical-ast §3.2], under the same names, with the types
mapped by §3.2 and the invariants of [LQ/canonical-ast §3.4]. The union of each field is [LQ/canonical-ast §3.3]. The
JSON Schema of §6 is generated from the same catalogue and is the machine-checkable form of this section; where prose and
schema differ, [LQ/canonical-ast §3.2] wins and the difference is a finding.

Fields that carry a default and may be omitted: every `bool`; every `[X]` list; `read.mode` (`run`), `sort.dir`
(`asc`), `scall.yield` (`none`); every optional field.

## 5. Validation and diagnostics

### 5.1 Order of checks

| Step | Check | Code on failure |
|---|---|---|
| 1 | the document is well-formed UTF-8 JSON with no repeated key | E001 (E003 for ill-formed UTF-8) |
| 2 | the root and every node match the schema of §6 | E001 |
| 3 | value rules the schema cannot state: an `int` field written with a fraction or exponent; the ranges of [LQ/lexical §8] (`nid`, `int`, a `float` that rounds to an infinity, a `dur` above its range) | E001 for the syntax, E003 for a range |
| 4 | the invariants of [LQ/canonical-ast §3.4], `lq` only on the root (§2), revision nodes only at revision positions (§3.5) | E001 |
| 5 | the checks the text parser makes that a tree can still fail (§5.2) | the code the parser would raise |
| 6 | binding, exactly as for text | the binder's codes |

### 5.2 Parser checks re-applied to a tree

| Tree | Code | Text equivalent ([LQ/grammar-v1.ebnf §R]) |
|---|---|---|
| in a `read` document, a `call` or `scall` whose `proc` begins with the segment `tx` (ASCII-case-insensitive) | E006 | `CALL tx.*` in a read |
| an `smatch` without `expect` | E007 | `MATCH` without `EXPECT` inside `TX` |
| a `tx` whose `stmts` is empty | E009 | `TX { }` |
| a `cmp` with op `=` or `<>` and a `null` operand; a `kv` with a `null` value in the `props` of an `npat` or `epat` | E118 | `= NULL`, `{p: null}` |
| a `fn` named `timestamp`, `single`, `shortestPath`, `allShortestPaths`, `nodes`, `relationships` or `CAST` (ASCII-case-insensitive) | E004 | the same names |
| a `call` or `scall` whose `proc` begins with `apoc`, `gds`, `db` or `dbms` | E004 | the same names |
| a `quant` with `min` above `max`, or a bound above 4294967295 | E114 | the same quantifiers |

Everything else that the text grammar refuses (multi-labels `:a:b`, `SKIP`, comprehensions, `USE` inside a subquery)
cannot be written as a tree of the catalogue.

### 5.3 Where an error points

A diagnostic about an IR document carries, besides the fields of [50 §5.2], the JSON Pointer (RFC 6901) of the
offending value as the additive key `ptr`; its `span` gives the byte offsets of that value in the IR text, and `line`
and `col` are computed over the IR text by [LQ/lexical §2.3]. The text form prints the pointer after the location. The
texts are [LQ/errors]'s (Open point J-3).

## 6. JSON Schema

The schema below is normative for the structure of an IR document (JSON Schema 2020-12). It is generated from the
catalogue of [LQ/canonical-ast §3.2]; `expr` and `lit` admit the shorthands of §3.3; every node admits `at`; `read`,
`tx` and `define` admit `lq`. Two catalogue requirements are left to step 5 of §5.1 so that they raise the text
parser's codes: `smatch.expect` is optional here (its absence is E007) and `tx.stmts` may be empty (E009).

```json
{
 "$schema": "https://json-schema.org/draft/2020-12/schema",
 "$id": "urn:moirai:lq:json-ir:1",
 "title": "LQ JSON IR, grammar version 1",
 "oneOf": [{"$ref": "#/$defs/read"}, {"$ref": "#/$defs/tx"}, {"$ref": "#/$defs/define"}],
 "$defs": {
  "span": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 2, "maxItems": 2},
  "read": {"type": "object", "properties": {"t": {"const": "read"}, "at": {"$ref": "#/$defs/span"}, "mode": {"enum": ["run", "explain", "profile"]}, "query": {"$ref": "#/$defs/query"}, "lq": {"const": 1}}, "required": ["t", "query"], "additionalProperties": false},
  "tx": {"type": "object", "properties": {"t": {"const": "tx"}, "at": {"$ref": "#/$defs/span"}, "on": {"oneOf": [{"$ref": "#/$defs/rev"}, {"type": "null"}]}, "if_tip": {"oneOf": [{"$ref": "#/$defs/rev"}, {"type": "null"}]}, "if_targets": {"oneOf": [{"type": "string"}, {"type": "null"}]}, "key": {"oneOf": [{"type": "string"}, {"type": "null"}]}, "lease": {"oneOf": [{"type": "string"}, {"type": "null"}]}, "message": {"oneOf": [{"type": "string"}, {"type": "null"}]}, "stmts": {"type": "array", "items": {"$ref": "#/$defs/stmt"}}, "dry": {"type": "boolean"}, "lq": {"const": 1}}, "required": ["t"], "additionalProperties": false},
  "define": {"type": "object", "properties": {"t": {"const": "define"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "minLength": 1}, "params": {"type": "array", "items": {"$ref": "#/$defs/pdecl"}}, "shape": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}, "budget": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}, "body": {"$ref": "#/$defs/query"}, "lq": {"const": 1}}, "required": ["t", "name", "body"], "additionalProperties": false},
  "query": {"type": "object", "properties": {"t": {"const": "query"}, "at": {"$ref": "#/$defs/span"}, "parts": {"type": "array", "items": {"$ref": "#/$defs/part"}, "minItems": 1}, "ops": {"type": "array", "items": {"enum": ["union", "union_all", "except", "intersect"]}}}, "required": ["t", "parts"], "additionalProperties": false},
  "part": {"type": "object", "properties": {"t": {"const": "part"}, "at": {"$ref": "#/$defs/span"}, "use": {"oneOf": [{"$ref": "#/$defs/rev"}, {"type": "null"}]}, "clauses": {"type": "array", "items": {"$ref": "#/$defs/clause"}}, "return": {"oneOf": [{"$ref": "#/$defs/return"}, {"type": "null"}]}, "call": {"oneOf": [{"$ref": "#/$defs/scall"}, {"type": "null"}]}}, "required": ["t"], "additionalProperties": false},
  "scall": {"type": "object", "properties": {"t": {"const": "scall"}, "at": {"$ref": "#/$defs/span"}, "proc": {"type": "string", "minLength": 1}, "args": {"type": "array", "items": {"$ref": "#/$defs/arg"}}, "yield": {"enum": ["none", "star", "items"]}, "items": {"type": "array", "items": {"$ref": "#/$defs/yitem"}}, "where": {"$ref": "#/$defs/expr"}, "order": {"type": "array", "items": {"$ref": "#/$defs/sort"}}, "limit": {"$ref": "#/$defs/expr"}}, "required": ["t", "proc"], "additionalProperties": false},
  "match": {"type": "object", "properties": {"t": {"const": "match"}, "at": {"$ref": "#/$defs/span"}, "optional": {"type": "boolean"}, "mode": {"oneOf": [{"enum": ["walk", "trail", "acyclic", "simple", "different"]}, {"type": "null"}]}, "patterns": {"type": "array", "items": {"$ref": "#/$defs/path"}, "minItems": 1}, "where": {"$ref": "#/$defs/expr"}}, "required": ["t", "patterns"], "additionalProperties": false},
  "call": {"type": "object", "properties": {"t": {"const": "call"}, "at": {"$ref": "#/$defs/span"}, "proc": {"type": "string", "minLength": 1}, "args": {"type": "array", "items": {"$ref": "#/$defs/arg"}}, "yield": {"type": "array", "items": {"$ref": "#/$defs/yitem"}, "minItems": 1}, "where": {"$ref": "#/$defs/expr"}}, "required": ["t", "proc", "yield"], "additionalProperties": false},
  "unwind": {"type": "object", "properties": {"t": {"const": "unwind"}, "at": {"$ref": "#/$defs/span"}, "expr": {"$ref": "#/$defs/expr"}, "as": {"type": "string", "minLength": 1}}, "required": ["t", "expr", "as"], "additionalProperties": false},
  "with": {"type": "object", "properties": {"t": {"const": "with"}, "at": {"$ref": "#/$defs/span"}, "distinct": {"type": "boolean"}, "star": {"type": "boolean"}, "items": {"type": "array", "items": {"$ref": "#/$defs/item"}}, "where": {"$ref": "#/$defs/expr"}, "order": {"type": "array", "items": {"$ref": "#/$defs/sort"}}, "limit": {"$ref": "#/$defs/expr"}}, "required": ["t"], "additionalProperties": false},
  "return": {"type": "object", "properties": {"t": {"const": "return"}, "at": {"$ref": "#/$defs/span"}, "distinct": {"type": "boolean"}, "star": {"type": "boolean"}, "items": {"type": "array", "items": {"$ref": "#/$defs/item"}}, "group": {"type": "array", "items": {"$ref": "#/$defs/expr"}}, "order": {"type": "array", "items": {"$ref": "#/$defs/sort"}}, "limit": {"$ref": "#/$defs/expr"}}, "required": ["t"], "additionalProperties": false},
  "item": {"type": "object", "properties": {"t": {"const": "item"}, "at": {"$ref": "#/$defs/span"}, "expr": {"$ref": "#/$defs/expr"}, "as": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}}, "required": ["t", "expr"], "additionalProperties": false},
  "yitem": {"type": "object", "properties": {"t": {"const": "yitem"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "minLength": 1}, "as": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}}, "required": ["t", "name"], "additionalProperties": false},
  "sort": {"type": "object", "properties": {"t": {"const": "sort"}, "at": {"$ref": "#/$defs/span"}, "expr": {"$ref": "#/$defs/expr"}, "dir": {"enum": ["asc", "desc"]}}, "required": ["t", "expr"], "additionalProperties": false},
  "arg": {"type": "object", "properties": {"t": {"const": "arg"}, "at": {"$ref": "#/$defs/span"}, "name": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}, "value": {"$ref": "#/$defs/argval"}}, "required": ["t", "value"], "additionalProperties": false},
  "kv": {"type": "object", "properties": {"t": {"const": "kv"}, "at": {"$ref": "#/$defs/span"}, "key": {"type": "string", "minLength": 1}, "value": {"$ref": "#/$defs/expr"}}, "required": ["t", "key", "value"], "additionalProperties": false},
  "when": {"type": "object", "properties": {"t": {"const": "when"}, "at": {"$ref": "#/$defs/span"}, "cond": {"$ref": "#/$defs/expr"}, "then": {"$ref": "#/$defs/expr"}}, "required": ["t", "cond", "then"], "additionalProperties": false},
  "path": {"type": "object", "properties": {"t": {"const": "path"}, "at": {"$ref": "#/$defs/span"}, "start": {"$ref": "#/$defs/npat"}, "steps": {"type": "array", "items": {"$ref": "#/$defs/step"}}}, "required": ["t", "start"], "additionalProperties": false},
  "npat": {"type": "object", "properties": {"t": {"const": "npat"}, "at": {"$ref": "#/$defs/span"}, "var": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}, "labels": {"type": "array", "items": {"type": "string", "minLength": 1}}, "props": {"type": "array", "items": {"$ref": "#/$defs/kv"}}, "where": {"$ref": "#/$defs/expr"}}, "required": ["t"], "additionalProperties": false},
  "estep": {"type": "object", "properties": {"t": {"const": "estep"}, "at": {"$ref": "#/$defs/span"}, "edge": {"$ref": "#/$defs/epat"}, "node": {"$ref": "#/$defs/npat"}}, "required": ["t", "edge", "node"], "additionalProperties": false},
  "gstep": {"type": "object", "properties": {"t": {"const": "gstep"}, "at": {"$ref": "#/$defs/span"}, "group": {"$ref": "#/$defs/group"}, "node": {"$ref": "#/$defs/npat"}}, "required": ["t", "group", "node"], "additionalProperties": false},
  "epat": {"type": "object", "properties": {"t": {"const": "epat"}, "at": {"$ref": "#/$defs/span"}, "var": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}, "dir": {"enum": ["right", "left", "both"]}, "types": {"type": "array", "items": {"type": "string", "minLength": 1}}, "quant": {"oneOf": [{"$ref": "#/$defs/quant"}, {"type": "null"}]}, "props": {"type": "array", "items": {"$ref": "#/$defs/kv"}}, "where": {"$ref": "#/$defs/expr"}}, "required": ["t", "dir"], "additionalProperties": false},
  "group": {"type": "object", "properties": {"t": {"const": "group"}, "at": {"$ref": "#/$defs/span"}, "path": {"$ref": "#/$defs/path"}, "where": {"$ref": "#/$defs/expr"}, "quant": {"$ref": "#/$defs/quant"}}, "required": ["t", "path", "quant"], "additionalProperties": false},
  "quant": {"type": "object", "properties": {"t": {"const": "quant"}, "at": {"$ref": "#/$defs/span"}, "min": {"type": "integer", "minimum": 0}, "max": {"oneOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]}}, "required": ["t", "min"], "additionalProperties": false},
  "or": {"type": "object", "properties": {"t": {"const": "or"}, "at": {"$ref": "#/$defs/span"}, "l": {"$ref": "#/$defs/expr"}, "r": {"$ref": "#/$defs/expr"}}, "required": ["t", "l", "r"], "additionalProperties": false},
  "and": {"type": "object", "properties": {"t": {"const": "and"}, "at": {"$ref": "#/$defs/span"}, "l": {"$ref": "#/$defs/expr"}, "r": {"$ref": "#/$defs/expr"}}, "required": ["t", "l", "r"], "additionalProperties": false},
  "not": {"type": "object", "properties": {"t": {"const": "not"}, "at": {"$ref": "#/$defs/span"}, "e": {"$ref": "#/$defs/expr"}}, "required": ["t", "e"], "additionalProperties": false},
  "cmp": {"type": "object", "properties": {"t": {"const": "cmp"}, "at": {"$ref": "#/$defs/span"}, "op": {"enum": ["=", "<>", "<", "<=", ">", ">="]}, "l": {"$ref": "#/$defs/expr"}, "r": {"$ref": "#/$defs/expr"}}, "required": ["t", "op", "l", "r"], "additionalProperties": false},
  "isnull": {"type": "object", "properties": {"t": {"const": "isnull"}, "at": {"$ref": "#/$defs/span"}, "neg": {"type": "boolean"}, "e": {"$ref": "#/$defs/expr"}}, "required": ["t", "e"], "additionalProperties": false},
  "in": {"type": "object", "properties": {"t": {"const": "in"}, "at": {"$ref": "#/$defs/span"}, "l": {"$ref": "#/$defs/expr"}, "r": {"$ref": "#/$defs/expr"}}, "required": ["t", "l", "r"], "additionalProperties": false},
  "strpred": {"type": "object", "properties": {"t": {"const": "strpred"}, "at": {"$ref": "#/$defs/span"}, "op": {"enum": ["starts", "ends", "contains"]}, "l": {"$ref": "#/$defs/expr"}, "r": {"$ref": "#/$defs/expr"}}, "required": ["t", "op", "l", "r"], "additionalProperties": false},
  "labeltest": {"type": "object", "properties": {"t": {"const": "labeltest"}, "at": {"$ref": "#/$defs/span"}, "e": {"$ref": "#/$defs/expr"}, "labels": {"type": "array", "items": {"type": "string", "minLength": 1}, "minItems": 1}}, "required": ["t", "e", "labels"], "additionalProperties": false},
  "arith": {"type": "object", "properties": {"t": {"const": "arith"}, "at": {"$ref": "#/$defs/span"}, "op": {"enum": ["+", "-", "*", "/"]}, "l": {"$ref": "#/$defs/expr"}, "r": {"$ref": "#/$defs/expr"}}, "required": ["t", "op", "l", "r"], "additionalProperties": false},
  "neg": {"type": "object", "properties": {"t": {"const": "neg"}, "at": {"$ref": "#/$defs/span"}, "e": {"$ref": "#/$defs/expr"}}, "required": ["t", "e"], "additionalProperties": false},
  "prop": {"type": "object", "properties": {"t": {"const": "prop"}, "at": {"$ref": "#/$defs/span"}, "e": {"$ref": "#/$defs/expr"}, "name": {"type": "string", "minLength": 1}}, "required": ["t", "e", "name"], "additionalProperties": false},
  "ident": {"type": "object", "properties": {"t": {"const": "ident"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "minLength": 1}}, "required": ["t", "name"], "additionalProperties": false},
  "param": {"type": "object", "properties": {"t": {"const": "param"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "pattern": "^[A-Za-z_][A-Za-z0-9_]*$"}}, "required": ["t", "name"], "additionalProperties": false},
  "nid": {"type": "object", "properties": {"t": {"const": "nid"}, "at": {"$ref": "#/$defs/span"}, "n": {"type": "integer", "minimum": 1, "maximum": 4294967295}}, "required": ["t", "n"], "additionalProperties": false},
  "uid": {"type": "object", "properties": {"t": {"const": "uid"}, "at": {"$ref": "#/$defs/span"}, "hex": {"type": "string", "pattern": "^[0-9a-f]{32}$"}}, "required": ["t", "hex"], "additionalProperties": false},
  "int": {"type": "object", "properties": {"t": {"const": "int"}, "at": {"$ref": "#/$defs/span"}, "v": {"type": "integer", "minimum": 0, "maximum": 9223372036854775807}}, "required": ["t", "v"], "additionalProperties": false},
  "float": {"type": "object", "properties": {"t": {"const": "float"}, "at": {"$ref": "#/$defs/span"}, "v": {"type": "string", "pattern": "^[0-9]+([.][0-9]+([eE][+-]?[0-9]+)?|[eE][+-]?[0-9]+)$"}}, "required": ["t", "v"], "additionalProperties": false},
  "str": {"type": "object", "properties": {"t": {"const": "str"}, "at": {"$ref": "#/$defs/span"}, "v": {"type": "string"}}, "required": ["t", "v"], "additionalProperties": false},
  "dur": {"type": "object", "properties": {"t": {"const": "dur"}, "at": {"$ref": "#/$defs/span"}, "v": {"type": "string", "pattern": "^[0-9]+[smhdw]$"}}, "required": ["t", "v"], "additionalProperties": false},
  "bool": {"type": "object", "properties": {"t": {"const": "bool"}, "at": {"$ref": "#/$defs/span"}, "v": {"type": "boolean"}}, "required": ["t"], "additionalProperties": false},
  "null": {"type": "object", "properties": {"t": {"const": "null"}, "at": {"$ref": "#/$defs/span"}}, "required": ["t"], "additionalProperties": false},
  "exists": {"type": "object", "properties": {"t": {"const": "exists"}, "at": {"$ref": "#/$defs/span"}, "sub": {"$ref": "#/$defs/sub"}}, "required": ["t", "sub"], "additionalProperties": false},
  "countsub": {"type": "object", "properties": {"t": {"const": "countsub"}, "at": {"$ref": "#/$defs/span"}, "sub": {"$ref": "#/$defs/sub"}}, "required": ["t", "sub"], "additionalProperties": false},
  "subq": {"type": "object", "properties": {"t": {"const": "subq"}, "at": {"$ref": "#/$defs/span"}, "clauses": {"type": "array", "items": {"$ref": "#/$defs/clause"}}, "return": {"oneOf": [{"$ref": "#/$defs/return"}, {"type": "null"}]}}, "required": ["t"], "additionalProperties": false},
  "subp": {"type": "object", "properties": {"t": {"const": "subp"}, "at": {"$ref": "#/$defs/span"}, "patterns": {"type": "array", "items": {"$ref": "#/$defs/path"}, "minItems": 1}, "where": {"$ref": "#/$defs/expr"}}, "required": ["t", "patterns"], "additionalProperties": false},
  "fn": {"type": "object", "properties": {"t": {"const": "fn"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "minLength": 1}, "distinct": {"type": "boolean"}, "args": {"type": "array", "items": {"$ref": "#/$defs/arg"}}}, "required": ["t", "name"], "additionalProperties": false},
  "countstar": {"type": "object", "properties": {"t": {"const": "countstar"}, "at": {"$ref": "#/$defs/span"}}, "required": ["t"], "additionalProperties": false},
  "listpred": {"type": "object", "properties": {"t": {"const": "listpred"}, "at": {"$ref": "#/$defs/span"}, "kind": {"enum": ["all", "any", "none"]}, "var": {"type": "string", "minLength": 1}, "list": {"$ref": "#/$defs/expr"}, "pred": {"$ref": "#/$defs/expr"}}, "required": ["t", "kind", "var", "list", "pred"], "additionalProperties": false},
  "list": {"type": "object", "properties": {"t": {"const": "list"}, "at": {"$ref": "#/$defs/span"}, "elems": {"type": "array", "items": {"$ref": "#/$defs/expr"}}}, "required": ["t"], "additionalProperties": false},
  "map": {"type": "object", "properties": {"t": {"const": "map"}, "at": {"$ref": "#/$defs/span"}, "entries": {"type": "array", "items": {"$ref": "#/$defs/kv"}}}, "required": ["t"], "additionalProperties": false},
  "case": {"type": "object", "properties": {"t": {"const": "case"}, "at": {"$ref": "#/$defs/span"}, "subject": {"$ref": "#/$defs/expr"}, "whens": {"type": "array", "items": {"$ref": "#/$defs/when"}, "minItems": 1}, "else": {"$ref": "#/$defs/expr"}}, "required": ["t", "whens"], "additionalProperties": false},
  "rhead": {"type": "object", "properties": {"t": {"const": "rhead"}, "at": {"$ref": "#/$defs/span"}}, "required": ["t"], "additionalProperties": false},
  "rref": {"type": "object", "properties": {"t": {"const": "rref"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "pattern": "^[a-z0-9_][a-z0-9_-]*([.][a-z0-9_][a-z0-9_-]*)*(/[a-z0-9_][a-z0-9_-]*([.][a-z0-9_][a-z0-9_-]*)*)*$"}}, "required": ["t", "name"], "additionalProperties": false},
  "rcommit": {"type": "object", "properties": {"t": {"const": "rcommit"}, "at": {"$ref": "#/$defs/span"}, "hex": {"type": "string", "pattern": "^[0-9a-f]{7,64}$"}}, "required": ["t", "hex"], "additionalProperties": false},
  "rseq": {"type": "object", "properties": {"t": {"const": "rseq"}, "at": {"$ref": "#/$defs/span"}, "n": {"type": "integer", "minimum": 0}}, "required": ["t", "n"], "additionalProperties": false},
  "rsuf": {"type": "object", "properties": {"t": {"const": "rsuf"}, "at": {"$ref": "#/$defs/span"}, "base": {"$ref": "#/$defs/rev"}, "kind": {"enum": ["tilde", "caret", "at", "attime"]}, "n": {"oneOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]}, "time": {"oneOf": [{"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"}, {"type": "null"}]}}, "required": ["t", "base", "kind"], "additionalProperties": false},
  "rrange": {"type": "object", "properties": {"t": {"const": "rrange"}, "at": {"$ref": "#/$defs/span"}, "from": {"$ref": "#/$defs/rev"}, "op": {"enum": ["two", "three"]}, "to": {"$ref": "#/$defs/rev"}}, "required": ["t", "from", "op", "to"], "additionalProperties": false},
  "rlist": {"type": "object", "properties": {"t": {"const": "rlist"}, "at": {"$ref": "#/$defs/span"}, "elems": {"type": "array", "items": {"$ref": "#/$defs/rev"}, "minItems": 1}}, "required": ["t", "elems"], "additionalProperties": false},
  "smatch": {"type": "object", "properties": {"t": {"const": "smatch"}, "at": {"$ref": "#/$defs/span"}, "patterns": {"type": "array", "items": {"$ref": "#/$defs/path"}, "minItems": 1}, "where": {"$ref": "#/$defs/expr"}, "expect": {"$ref": "#/$defs/expect"}, "muts": {"type": "array", "items": {"$ref": "#/$defs/mut"}, "minItems": 1}}, "required": ["t", "patterns", "muts"], "additionalProperties": false},
  "smuts": {"type": "object", "properties": {"t": {"const": "smuts"}, "at": {"$ref": "#/$defs/span"}, "muts": {"type": "array", "items": {"$ref": "#/$defs/mut"}, "minItems": 1}}, "required": ["t", "muts"], "additionalProperties": false},
  "screate": {"type": "object", "properties": {"t": {"const": "screate"}, "at": {"$ref": "#/$defs/span"}, "var": {"type": "string", "minLength": 1}, "label": {"type": "string", "minLength": 1}, "props": {"type": "array", "items": {"$ref": "#/$defs/kv"}}, "edges": {"type": "array", "items": {"$ref": "#/$defs/cedge"}}, "under": {"oneOf": [{"$ref": "#/$defs/target"}, {"type": "null"}]}, "unless": {"oneOf": [{"$ref": "#/$defs/sub"}, {"type": "null"}]}}, "required": ["t", "var", "label"], "additionalProperties": false},
  "cedge": {"type": "object", "properties": {"t": {"const": "cedge"}, "at": {"$ref": "#/$defs/span"}, "dir": {"enum": ["right", "left"]}, "type": {"type": "string", "minLength": 1}, "props": {"type": "array", "items": {"$ref": "#/$defs/kv"}}, "target": {"$ref": "#/$defs/target"}}, "required": ["t", "dir", "type", "target"], "additionalProperties": false},
  "stxcall": {"type": "object", "properties": {"t": {"const": "stxcall"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "minLength": 1}, "args": {"type": "array", "items": {"$ref": "#/$defs/arg"}}, "yield": {"type": "array", "items": {"$ref": "#/$defs/yitem"}}}, "required": ["t", "name"], "additionalProperties": false},
  "sassert": {"type": "object", "properties": {"t": {"const": "sassert"}, "at": {"$ref": "#/$defs/span"}, "expr": {"$ref": "#/$defs/expr"}, "else": {"oneOf": [{"type": "string"}, {"type": "null"}]}}, "required": ["t", "expr"], "additionalProperties": false},
  "sresolve": {"type": "object", "properties": {"t": {"const": "sresolve"}, "at": {"$ref": "#/$defs/span"}, "key": {"oneOf": [{"type": "string"}, {"type": "null"}]}, "query": {"oneOf": [{"$ref": "#/$defs/query"}, {"type": "null"}]}, "expect": {"oneOf": [{"$ref": "#/$defs/expect"}, {"type": "null"}]}, "take": {"enum": ["ours", "theirs", "base", "value", "repoint"]}, "value": {"$ref": "#/$defs/expr"}, "target": {"oneOf": [{"$ref": "#/$defs/target"}, {"type": "null"}]}}, "required": ["t", "take"], "additionalProperties": false},
  "sdrop": {"type": "object", "properties": {"t": {"const": "sdrop"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "minLength": 1}}, "required": ["t", "name"], "additionalProperties": false},
  "mset": {"type": "object", "properties": {"t": {"const": "mset"}, "at": {"$ref": "#/$defs/span"}, "assigns": {"type": "array", "items": {"$ref": "#/$defs/assign"}, "minItems": 1}}, "required": ["t", "assigns"], "additionalProperties": false},
  "assign": {"type": "object", "properties": {"t": {"const": "assign"}, "at": {"$ref": "#/$defs/span"}, "target": {"$ref": "#/$defs/target"}, "prop": {"type": "string", "minLength": 1}, "value": {"$ref": "#/$defs/expr"}}, "required": ["t", "target", "prop", "value"], "additionalProperties": false},
  "mremove": {"type": "object", "properties": {"t": {"const": "mremove"}, "at": {"$ref": "#/$defs/span"}, "items": {"type": "array", "items": {"$ref": "#/$defs/tprop"}, "minItems": 1}}, "required": ["t", "items"], "additionalProperties": false},
  "tprop": {"type": "object", "properties": {"t": {"const": "tprop"}, "at": {"$ref": "#/$defs/span"}, "target": {"$ref": "#/$defs/target"}, "prop": {"type": "string", "minLength": 1}}, "required": ["t", "target", "prop"], "additionalProperties": false},
  "mdelete": {"type": "object", "properties": {"t": {"const": "mdelete"}, "at": {"$ref": "#/$defs/span"}, "targets": {"type": "array", "items": {"$ref": "#/$defs/target"}, "minItems": 1}, "opts": {"type": "array", "items": {"$ref": "#/$defs/dopt"}}}, "required": ["t", "targets"], "additionalProperties": false},
  "dpolicy": {"type": "object", "properties": {"t": {"const": "dpolicy"}, "at": {"$ref": "#/$defs/span"}, "v": {"enum": ["restrict", "cascade", "reparent"]}}, "required": ["t", "v"], "additionalProperties": false},
  "dreplaced": {"type": "object", "properties": {"t": {"const": "dreplaced"}, "at": {"$ref": "#/$defs/span"}, "target": {"$ref": "#/$defs/target"}}, "required": ["t", "target"], "additionalProperties": false},
  "drelease": {"type": "object", "properties": {"t": {"const": "drelease"}, "at": {"$ref": "#/$defs/span"}}, "required": ["t"], "additionalProperties": false},
  "dreason": {"type": "object", "properties": {"t": {"const": "dreason"}, "at": {"$ref": "#/$defs/span"}, "expr": {"$ref": "#/$defs/expr"}}, "required": ["t", "expr"], "additionalProperties": false},
  "mmove": {"type": "object", "properties": {"t": {"const": "mmove"}, "at": {"$ref": "#/$defs/span"}, "target": {"$ref": "#/$defs/target"}, "under": {"$ref": "#/$defs/target"}, "pos": {"oneOf": [{"enum": ["before", "after", "first", "last"]}, {"type": "null"}]}, "rel": {"oneOf": [{"$ref": "#/$defs/target"}, {"type": "null"}]}}, "required": ["t", "target", "under"], "additionalProperties": false},
  "medge": {"type": "object", "properties": {"t": {"const": "medge"}, "at": {"$ref": "#/$defs/span"}, "src": {"$ref": "#/$defs/target"}, "dir": {"enum": ["right", "left"]}, "type": {"type": "string", "minLength": 1}, "props": {"type": "array", "items": {"$ref": "#/$defs/kv"}}, "dst": {"$ref": "#/$defs/target"}}, "required": ["t", "src", "dir", "type", "dst"], "additionalProperties": false},
  "mreopen": {"type": "object", "properties": {"t": {"const": "mreopen"}, "at": {"$ref": "#/$defs/span"}, "target": {"$ref": "#/$defs/target"}, "reason": {"$ref": "#/$defs/expr"}}, "required": ["t", "target", "reason"], "additionalProperties": false},
  "mpatch": {"type": "object", "properties": {"t": {"const": "mpatch"}, "at": {"$ref": "#/$defs/span"}, "target": {"$ref": "#/$defs/target"}, "field": {"type": "string", "minLength": 1}, "remove": {"$ref": "#/$defs/expr"}, "add": {"$ref": "#/$defs/expr"}}, "required": ["t", "target", "field", "remove", "add"], "additionalProperties": false},
  "expect": {"type": "object", "properties": {"t": {"const": "expect"}, "at": {"$ref": "#/$defs/span"}, "kind": {"enum": ["exact", "range", "le", "ge", "param"]}, "a": {"oneOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]}, "b": {"oneOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]}, "param": {"oneOf": [{"type": "string", "pattern": "^[A-Za-z_][A-Za-z0-9_]*$"}, {"type": "null"}]}}, "required": ["t", "kind"], "additionalProperties": false},
  "pdecl": {"type": "object", "properties": {"t": {"const": "pdecl"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "pattern": "^[A-Za-z_][A-Za-z0-9_]*$"}, "type": {"$ref": "#/$defs/ptype"}, "optional": {"type": "boolean"}, "default": {"$ref": "#/$defs/lit"}}, "required": ["t", "name", "type"], "additionalProperties": false},
  "ptype": {"type": "object", "properties": {"t": {"const": "ptype"}, "at": {"$ref": "#/$defs/span"}, "name": {"type": "string", "minLength": 1}, "arg": {"oneOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}}, "required": ["t", "name"], "additionalProperties": false},
  "expr": {"oneOf": [{"type": "string"}, {"type": "number"}, {"type": "boolean"}, {"type": "null"}, {"type": "array", "items": {"$ref": "#/$defs/expr"}}, {"$ref": "#/$defs/or"}, {"$ref": "#/$defs/and"}, {"$ref": "#/$defs/not"}, {"$ref": "#/$defs/cmp"}, {"$ref": "#/$defs/isnull"}, {"$ref": "#/$defs/in"}, {"$ref": "#/$defs/strpred"}, {"$ref": "#/$defs/labeltest"}, {"$ref": "#/$defs/arith"}, {"$ref": "#/$defs/neg"}, {"$ref": "#/$defs/prop"}, {"$ref": "#/$defs/ident"}, {"$ref": "#/$defs/param"}, {"$ref": "#/$defs/nid"}, {"$ref": "#/$defs/uid"}, {"$ref": "#/$defs/int"}, {"$ref": "#/$defs/float"}, {"$ref": "#/$defs/str"}, {"$ref": "#/$defs/dur"}, {"$ref": "#/$defs/bool"}, {"$ref": "#/$defs/null"}, {"$ref": "#/$defs/exists"}, {"$ref": "#/$defs/countsub"}, {"$ref": "#/$defs/fn"}, {"$ref": "#/$defs/countstar"}, {"$ref": "#/$defs/listpred"}, {"$ref": "#/$defs/list"}, {"$ref": "#/$defs/map"}, {"$ref": "#/$defs/case"}]},
  "rev": {"oneOf": [{"$ref": "#/$defs/rhead"}, {"$ref": "#/$defs/rref"}, {"$ref": "#/$defs/rcommit"}, {"$ref": "#/$defs/rseq"}, {"$ref": "#/$defs/rsuf"}, {"$ref": "#/$defs/param"}]},
  "argval": {"anyOf": [{"$ref": "#/$defs/expr"}, {"$ref": "#/$defs/rhead"}, {"$ref": "#/$defs/rref"}, {"$ref": "#/$defs/rcommit"}, {"$ref": "#/$defs/rseq"}, {"$ref": "#/$defs/rsuf"}, {"$ref": "#/$defs/rrange"}, {"$ref": "#/$defs/rlist"}]},
  "clause": {"oneOf": [{"$ref": "#/$defs/match"}, {"$ref": "#/$defs/call"}, {"$ref": "#/$defs/unwind"}, {"$ref": "#/$defs/with"}]},
  "step": {"oneOf": [{"$ref": "#/$defs/estep"}, {"$ref": "#/$defs/gstep"}]},
  "sub": {"oneOf": [{"$ref": "#/$defs/subq"}, {"$ref": "#/$defs/subp"}]},
  "stmt": {"oneOf": [{"$ref": "#/$defs/smatch"}, {"$ref": "#/$defs/smuts"}, {"$ref": "#/$defs/screate"}, {"$ref": "#/$defs/stxcall"}, {"$ref": "#/$defs/sassert"}, {"$ref": "#/$defs/sresolve"}, {"$ref": "#/$defs/define"}, {"$ref": "#/$defs/sdrop"}]},
  "mut": {"oneOf": [{"$ref": "#/$defs/mset"}, {"$ref": "#/$defs/mremove"}, {"$ref": "#/$defs/mdelete"}, {"$ref": "#/$defs/mmove"}, {"$ref": "#/$defs/medge"}, {"$ref": "#/$defs/mreopen"}, {"$ref": "#/$defs/mpatch"}]},
  "dopt": {"oneOf": [{"$ref": "#/$defs/dpolicy"}, {"$ref": "#/$defs/dreplaced"}, {"$ref": "#/$defs/drelease"}, {"$ref": "#/$defs/dreason"}]},
  "target": {"oneOf": [{"$ref": "#/$defs/ident"}, {"$ref": "#/$defs/nid"}, {"$ref": "#/$defs/uid"}, {"$ref": "#/$defs/param"}]},
  "lit": {"oneOf": [{"type": "string"}, {"type": "number"}, {"type": "boolean"}, {"type": "null"}, {"$ref": "#/$defs/int"}, {"$ref": "#/$defs/float"}, {"$ref": "#/$defs/str"}, {"$ref": "#/$defs/dur"}, {"$ref": "#/$defs/bool"}, {"$ref": "#/$defs/null"}, {"$ref": "#/$defs/nid"}, {"$ref": "#/$defs/uid"}]}
 }
}
```

## 7. Examples

**Q18** ([50 §2.9]), `TX ON lane/l5np KEY 'wf:r7/dev1/complete-89' LEASE 'L-18' { MATCH (t:task {id: #89}) WHERE
t.status = 'in_progress' AND t.rev = 4466 EXPECT 1 SET t.done = true }`:

```json
{"lq": 1, "t": "tx",
 "on": {"t": "rref", "name": "lane/l5np"},
 "key": "wf:r7/dev1/complete-89",
 "lease": "L-18",
 "stmts": [
  {"t": "smatch",
   "patterns": [{"t": "path",
                 "start": {"t": "npat", "var": "t", "labels": ["task"],
                           "props": [{"t": "kv", "key": "id", "value": {"t": "nid", "n": 89}}]}}],
   "where": {"t": "and",
             "l": {"t": "cmp", "op": "=", "l": {"t": "prop", "e": {"t": "ident", "name": "t"}, "name": "status"},
                   "r": "in_progress"},
             "r": {"t": "cmp", "op": "=", "l": {"t": "prop", "e": {"t": "ident", "name": "t"}, "name": "rev"},
                   "r": 4466}},
   "expect": {"t": "expect", "kind": "exact", "a": 1},
   "muts": [{"t": "mset", "assigns": [{"t": "assign", "target": {"t": "ident", "name": "t"}, "prop": "done",
                                       "value": true}]}]}]}
```

**Q5**, `MATCH (x:task)((a:task)-[:BLOCKS]->(b) WHERE a.unfinished){1,5}(#93) RETURN x`:

```json
{"t": "read",
 "query": {"t": "query", "parts": [
  {"t": "part",
   "clauses": [
    {"t": "match", "patterns": [
     {"t": "path", "start": {"t": "npat", "var": "x", "labels": ["task"]},
      "steps": [
       {"t": "gstep",
        "group": {"t": "group",
                  "path": {"t": "path", "start": {"t": "npat", "var": "a", "labels": ["task"]},
                           "steps": [{"t": "estep", "edge": {"t": "epat", "dir": "right", "types": ["BLOCKS"]},
                                      "node": {"t": "npat", "var": "b"}}]},
                  "where": {"t": "prop", "e": {"t": "ident", "name": "a"}, "name": "unfinished"},
                  "quant": {"t": "quant", "min": 1, "max": 5}},
        "node": {"t": "npat", "props": [{"t": "kv", "key": "id", "value": {"t": "nid", "n": 93}}]}}]}]}],
   "return": {"t": "return", "items": [{"t": "item", "expr": {"t": "ident", "name": "x"}}]}}]}}
```

**Q13**, `CALL diff(main...lane/l10) YIELD change, node, side WHERE side = 'both'` (a standalone call with a revision
argument):

```json
{"t": "read",
 "query": {"t": "query", "parts": [
  {"t": "part",
   "call": {"t": "scall", "proc": "diff",
            "args": [{"t": "arg", "value": {"t": "rrange", "from": {"t": "rref", "name": "main"}, "op": "three",
                                            "to": {"t": "rref", "name": "lane/l10"}}}],
            "yield": "items",
            "items": [{"t": "yitem", "name": "change"}, {"t": "yitem", "name": "node"}, {"t": "yitem", "name": "side"}],
            "where": {"t": "cmp", "op": "=", "l": {"t": "ident", "name": "side"}, "r": "both"}}}]}}
```

**The example of [LQ/canonical-ast §4.4]**, whose C-AST encoding is [LQ/canonical-ast §6.5]:

```json
{"t": "read",
 "query": {"t": "query", "parts": [
  {"t": "part",
   "clauses": [
    {"t": "match",
     "patterns": [{"t": "path", "start": {"t": "npat", "var": "t", "labels": ["task"]}}],
     "where": {"t": "and",
               "l": {"t": "cmp", "op": "=", "l": {"t": "prop", "e": {"t": "ident", "name": "t"}, "name": "status"},
                     "r": "open"},
               "r": {"t": "cmp", "op": "<=", "l": {"t": "prop", "e": {"t": "ident", "name": "t"}, "name": "priority"},
                     "r": 1}}}],
   "return": {"t": "return", "items": [{"t": "item", "expr": {"t": "ident", "name": "t"}}],
              "order": [{"t": "sort", "expr": {"t": "prop", "e": {"t": "ident", "name": "t"}, "name": "priority"}}],
              "limit": 5}}]}}
```

**Q21**'s definition in portable form ([50 §2.9], [50 §4.4]):

```json
{"t": "define", "name": "stale_blockers",
 "params": [{"t": "pdecl", "name": "scope", "type": {"t": "ptype", "name": "node"},
             "default": {"t": "uid", "hex": "018f3c2e7a117b3c9d5e4c2f1a0b9e09"}},
            {"t": "pdecl", "name": "days", "type": {"t": "ptype", "name": "int"}, "default": 3}],
 "shape": "node",
 "body": {"t": "query", "parts": [{"t": "part",
   "clauses": [{"t": "match",
     "patterns": [{"t": "path", "start": {"t": "npat", "var": "b", "labels": ["task"]},
                   "steps": [{"t": "estep", "edge": {"t": "epat", "dir": "right", "types": ["BLOCKS"]},
                              "node": {"t": "npat", "var": "t", "labels": ["task"]}}]}],
     "where": {"t": "and",
       "l": {"t": "and",
         "l": {"t": "in", "l": {"t": "ident", "name": "t"},
               "r": {"t": "fn", "name": "subtree", "args": [{"t": "arg", "value": {"t": "param", "name": "scope"}}]}},
         "r": {"t": "cmp", "op": "=", "l": {"t": "prop", "e": {"t": "ident", "name": "b"}, "name": "status"},
               "r": "in_progress"}},
       "r": {"t": "cmp", "op": "<", "l": {"t": "prop", "e": {"t": "ident", "name": "b"}, "name": "updated_at"},
             "r": {"t": "arith", "op": "-", "l": {"t": "fn", "name": "now"},
                   "r": {"t": "arith", "op": "*", "l": {"t": "param", "name": "days"}, "r": {"t": "dur", "v": "1d"}}}}}}],
   "return": {"t": "return", "distinct": true, "items": [{"t": "item", "expr": {"t": "ident", "name": "b"}}],
              "order": [{"t": "sort", "expr": {"t": "prop", "e": {"t": "ident", "name": "b"}, "name": "updated_at"}}]}}]}}
```

Each example validates against §6.

## 8. The output form (`--ast`, goldens)

When moirai or LQ-3 prints an IR document (`--ast`, a fixture golden), it prints one form so that goldens compare
byte for byte:

1. one line, no whitespace between tokens, UTF-8, with strings escaped by the rule of [LQ/lexical §11.1];
2. in each object, `t` first, then the fields in catalogue order, then `at` if spans are printed;
3. omitted: an absent optional field, an empty `[X]` list, a `bool` field that is false, an enum field equal to its
   default, `lq` (the root's grammar version is implied);
4. the shorthands of §3.3 for `str`, non-negative `int`, `bool` and `null` in an expression position that is not an
   optional field; every other node, `float`, `dur`, `nid`, `uid` and `list` included, as an object;
5. `--ast` prints spans; fixture goldens omit them.

## 9. Relation to the `apply` op batch

[50 §4.2] calls the JSON op batch of `moirai apply` "the JSON form of the same IR", and [90 §6.6] keeps that batch on
the CLI (`apply`, `--json`) and off the MCP surface. This chapter fixes the IR the batch lowers into: every op of a
batch (`create`, `set`, `link`, `unlink`, `move`, `doc_patch`, `transition`, with `$refs` and guards — the batch [90]'s
edit list for [AR §7.2] describes) becomes statements of one `tx` document, and the batch's idempotency is that document's ([AR §6.4]).
The op-batch record format, its `$refs` and its lowering table are M8's, specified with `apply` and `result.v1`
([90 §7.2]); they are not part of format v1 or of this chapter (Open point J-2).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [90 §6.6] MPSP rule 3 (no object-typed tool property) | that an IR document travels as one string property of a tool, and that the IR's own schema is not a tool schema | §2 items 4–5 |
| [PLAN §3.3] gap "JSON IR schema" (WP-19) | the whole chapter | §1–§8 |

No [60 §2.5] row, R-1…R-18 item, F-item, X-F item or [90 §10.1] item is specified here: the JSON IR is an input and
output form of the query surface, not part of format v1.

## Holes

None. No field, name, rule or code of this chapter waits on an M0 measurement. The IR follows the S-AST, which freezes
with the query surface after WP-72 ([LQ/grammar-v1.ebnf] O-11).

## Open points for the review

| # | Point | Resolution here |
|---|---|---|
| J-1 | **PLAN §3.3 gap "JSON IR schema" (WP-19).** [50] names the JSON IR as an input surface, the form of `apply` batches and `--ast`'s output, but gives no schema. | Resolved: the JSON IR is the S-AST in JSON (§1, §3), with the schema of §6 generated from the one catalogue of [LQ/canonical-ast §3.2]. It is the S-AST, not the C-AST, because an input surface must be bound like text (names, coercions, scopes and errors identical), and a C-AST cannot be written without a store's uids and commit ids. |
| J-2 | [50 §4.2] and [90 §6.6] tie the `apply` op batch to "the same IR", but [AR §7.1] describes `apply`'s input as `result.v1` records and op batches (`create/set/link/…` with `$refs`). | This chapter fixes the target IR only; the op-batch records and their lowering are M8's (§9). Nothing in format v1 depends on them. The product accepts IR documents at no door in M0. |
| J-3 | Error location for a tree has no line in the user's text. | An additive `ptr` key (JSON Pointer) beside `span` (§5.3). [LQ/errors] and the envelope chapter carry the text form; the key is additive to the frozen envelope ([50 §6.4]: "extended only by additive keys"). |
| J-4 | A float's text must survive the JSON reader, because the S-AST keeps it. | Required of every reader (§3.3). R-BENCH's converter (serde_json) needs the `arbitrary_precision` feature or its own number scan; the product's reader (M8) is moirai's own. |
| J-5 | Revision nodes are explicit in the IR, so a tree could put one where text has no revision mode. | E001 there (§3.5); the text-equivalent string stays allowed and is coerced. |
| J-6 | Shorthands. The IR is an ablation surface for agents, so scalars and arrays may stand for literals (§3.3); `null` means absent in optional fields and the `NULL` literal elsewhere (§3.4). | As stated; the output form uses the shorthands only where they are unambiguous (§8). |
| J-7 | MPSP ([90 §6.6] rule 4) forbids `$ref`, `oneOf` and `const` in tool schemas. | Not applicable: the IR travels as a string property of a tool, and this schema validates the string's content on the server side (§2 item 4). |
| J-8 | Grammar version inside the document. | Optional `lq` on the root, default 1 (§2 item 2), so a stored IR golden stays unambiguous when a later grammar version exists. |
| J-9 | Which LQ-Bench tool carries the IR, and the card of the JSON-IR arm. | WP-70/WP-71b's; this chapter fixes only the document. |
