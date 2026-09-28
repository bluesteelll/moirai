# LQ syntax tree and canonical AST (grammar version 1)

| | |
|---|---|
| Title | The LQ syntax tree (S-AST) and its text form for fixtures; the canonical AST (C-AST), its binary encoding and the query hash; the values derived from the hash (idempotency payload, F10 `stmt_hash`, cursor hash, EXPLAIN id, F3 named-query hash); the portable form of stored definitions |
| Chapter | [LQ/canonical-ast], `docs/spec/lq/canonical-ast.md` |
| Status | draft, pass 1 pending |
| Work package | WP-19a (R-SPEC-F), part of WP-19 ([PLAN §3.2] item 1); closes the [PLAN §3.3] gap "Canonical-AST encoding" |
| Sources | [50 §2.3] parser notes; [50 §2.5] edge table (LQ names, reverse aliases, symmetric kinds); [50 §2.6] built-ins and table functions; [50 §2.8] accepted spellings; [50 §3.2] typing and coercion; [50 §3.4] rules 3–4; [50 §3.5] cursors; [50 §3.7] rule 3; [50 §3.10] items 2, 3, 6, 8, 9, 11; [50 §4.1]–[50 §4.4] (named queries, portable form, F3 storage, merge); [50 §5.1]; [50 §5.3] canonical form; [50 §5.11] EXPLAIN id; [50 §8.1] F1, F2, F3, F10; [50 §8.3] property tests; [AR §4.3] commit header (`idem_payload`, `stmt_hash`); [AR §4.6] "Not hashed"; [AR §6.4] idempotency; [AR §7.7.1]–[AR §7.7.2]; [90 §8.1] L1; [72 m3]; the A1 review's S-02 and S-10 (`docs/spec/reviews/a1-S.md`) |
| Depends on | [F01] (fixed-width types §5.1, `bool8` §5.4, `f64` §5.5, `b16`/`b32` §5.6, `lp()` §6.3, hex text §6.4, the hash set §7.1, input framing §7.3); [LQ/grammar-v1.ebnf] (the productions); [LQ/lexical] (tokens and literal values); [LQ/std] (signatures, §2.9; lookup, §2.1); [LQ/gql-spelling] (the spelling table); [F06] (commit header layout); [F08] (the F1 and F2 schema rows); [F12], [RULES/merge-table] (the named-query merge rule) |

LQ text is parsed into a syntax tree, the **S-AST**. The binder turns the S-AST into the **canonical AST (C-AST)**: names
resolved, aliases and spellings mapped to one form, constants typed and portable, variables renamed. The C-AST has one
binary encoding, and BLAKE3-128 of that encoding is the **query hash**. This chapter defines the S-AST, its text form
for fixtures, the rules that produce the C-AST, the encoding, the values derived from the hash, and the portable form of
stored definitions.

## 1. Purpose, uses and stability

### 1.1 Uses of the query hash

| Use | Value | Where | Source |
|---|---|---|---|
| query hash `H` | BLAKE3-128 of the C-AST encoding (§7) | — | [50 §5.3] |
| idempotency payload | `H` of the `TX` block's C-AST | `idem_payload [16]` of the commit header and the idempotency record | [AR §4.3], [AR §6.4], [50 §3.10] item 8 |
| F10 `stmt_hash` | `H` of the `TX` block's C-AST (§7.2) | commit header, unhashed | [50 §8.1] F10, [AR §4.3] |
| default idempotency key | `H` enters the key derivation of [AR §6.4] | — | [AR §6.4] |
| pinned cursor | the 64-bit hash of the canonical query: `H[0..8]` read as u64 LE | the cursor | [50 §3.5] |
| EXPLAIN id | `q:` followed by the 8 lower-case hex digits of `H[0..4]` | EXPLAIN header (`query q:7f3a19c2`, Q23) | [50 §5.3], [50 §5.11] |
| named-query hash (F3) | `H` of the `DEFINE` C-AST, over the portable form (§8) | `QUERIES` item, unhashed, derived, recomputed on import | [50 §4.4], [50 §8.1] F3 |
| named-query merge | equal F3 hashes are not a conflict | merge rule of the `QUERIES` item | [50 §4.4], [AR §5a.7] |
| fixtures | S-AST and C-AST in the text form of §4; the encoding's bytes | `fixtures/lq/` (WP-22) | [50 §8.3] |

### 1.2 What the canonical form removes and what it keeps

The C-AST removes: formatting, comments and keyword case; Cypher-versus-GQL spellings; reverse aliases (rewritten to
the stored kind with the endpoints swapped); variable names (renamed by order of first appearance); parameter names and
order at call sites (values substituted with their types); store-local node numbers and revisions (portable forms); the
`EXPLAIN`/`PROFILE` mode; the match modes; the checked `GROUP BY` list ([50 §3.10] item 8, [50 §5.3]).

It keeps: the order of commutative operands and of every list the author wrote ([50 §5.3]: "reordering is left to the
planner so the canonical form stays obviously faithful"), parameter values, property and field names, `RETURN` aliases,
literal values, and every construct with a meaning of its own.

### 1.3 Stability

This chapter is the canonical-AST algorithm of grammar version 1. The algorithm is frozen per stored grammar version and
retained like the `.moi` encoders ([50 §4.4], [72 m3]): a later grammar version defines its own and keeps this one for
every stored definition whose `lq` is 1, so a hash recomputed on import never changes a merge decision.

The C-AST is internal. Agents never read it: `--show-query`, `--show-tx`, the reading echo and error rewrites use the
display printer and the display spelling of quantifiers, which LQ-Bench chooses ([50 §2.8], [90 §8.1] L1). The
canonical form, its encoding and every hash are independent of the display spelling.

## 2. Pipeline

```
LQ text ──lexer + parser──▶ S-AST ◀── JSON IR reader ── JSON IR ([LQ/json-ir])
                              │
                              │  binder, with the binding context of §5.1
                              ▼
                            C-AST ──encoding (§6)──▶ bytes ──BLAKE3-128──▶ H (§7)
```

The S-AST carries source spans and every written name; the C-AST carries neither spans nor variable names. A bind error
(any code the binder raises) means there is no C-AST and no hash.

## 3. The S-AST

### 3.1 Spelling normalisations applied by the parser

The parser builds the S-AST with these normalisations already applied ([LQ/grammar-v1.ebnf §P.15]). Each maps
spellings with one meaning to one node, so that the display printer can print any S-AST in either display spelling and
`parse(print(ast)) == ast` holds (§3.4).

1. `!=` gives `cmp` with op `<>`.
2. `x NOT IN y` gives `not`(`in`(x, y)). (Under two-valued logic the two are the same, [50 §3.3].)
3. `IS NOT NULL` gives `isnull` with `neg` = true.
4. **Quantifiers** give `quant(min, max)`, `max` absent meaning unbounded:

   | Written | Spelling | min | max |
   |---|---|---|---|
   | `-[:T*]->` | Cypher, inside the brackets | 1 | unbounded |
   | `-[:T*n]->` | Cypher | n | n |
   | `-[:T*m..n]->` | Cypher | m | n |
   | `-[:T*..n]->` | Cypher | 1 | n |
   | `-[:T*m..]->` | Cypher | m | unbounded |
   | `->+` | GQL, after the pattern | 1 | unbounded |
   | `->*` | GQL | 0 | unbounded |
   | `->{m,n}` | GQL | m | n |
   | `->{m,}` | GQL | m | unbounded |
   | `->{m}` | GQL | m | m |
   | `->{,n}` | GQL | 0 | n |

   The Cypher forms keep Cypher's meaning, in which a bare `*` means one or more hops ([50 §2.8]: "keeps Cypher's
   meaning wherever it accepts Cypher's spelling"); the GQL `*` means zero or more ([50 §3.7] rule 1). Two quantifiers
   on one edge, a lower bound above the upper, or a bound above 4294967295 is E114.
5. A **pattern predicate** in expression position, `exists(path)` and `size(path)` give `exists(subp([path], _))`,
   `exists(subp([path], _))` and `countsub(subp([path], _))` ([50 §2.8], fixtures `pattern-predicate`,
   `exists-fn-pattern`, `size-fn-pattern`).
6. `exists(e)` with exactly one positional argument that is not a path, and no `DISTINCT`, gives `isnull(true, e)`
   (`exists(n.p)` → `n.p IS NOT NULL`, [50 §2.8]).
7. A **node-literal pattern** `(#N)` or `(#u:…)` gives `npat` with no variable, no labels, `props` = [`kv`(`id`, the
   literal)] and no `where`: `(#51)` and `({id: #51})` are one S-AST.
8. An empty property map `{}` gives `props` = [].
9. `RETURN ALL` gives `distinct` = false; `ASC`, `ASCENDING` and no keyword give `asc`; `DESC` and `DESCENDING` give
   `desc`.
10. `INSERT` gives the same nodes as `CREATE` (`screate`, `medge`).
11. `DIFFERENT RELATIONSHIPS` and `DIFFERENT EDGES` give match mode `different`.
12. `REF@{n}` gives `rsuf(at, n)`; `~` and `^` without a count give n = 1; a datetime gives its normalised text
    `YYYY-MM-DDTHH:MM:SSZ` ([LQ/lexical §7.5]).
13. Strings, back-quoted identifiers and escapes are decoded; integers and node literals carry their values (leading
    zeros gone); floats and durations carry their source text ([LQ/lexical §5.6]).
14. `TX` options go into fixed fields whatever their written order; `USE` goes into `part.use`.

Everything else stays as written: variable, label, type, function, procedure and property names with their case; the
match mode; the `GROUP BY` list; positional and named arguments in written order; the order of every list. The
reading echo, W02, the `GROUP BY` check and every diagnostic need them.

### 3.2 Node catalogue

Types: `bool` (default false when absent in JSON), `int`, `str` (any string), `name` (a non-empty string), `enum{…}`,
a node tag, or a union of §3.3; `X?` is optional; `[X]` is a list (default empty) and `[X]+` a non-empty list. Fields
are listed in their order; the S-expression form (§4) and the JSON IR ([LQ/json-ir §4]) use this order and these
names.

| Tag | Fields, in order (type) | From | Notes |
|---|---|---|---|
| `read` | `mode` enum{run, explain, profile} = run; `query` query | read_input | The read root. `EXPLAIN`/`PROFILE` set `mode`. |
| `tx` | `on` rev?; `if_tip` rev?; `if_targets` str?; `key` str?; `lease` str?; `message` str?; `stmts` [stmt]; `dry` bool | write_input, tx, tx_option | Options in fixed fields whatever their written order; a repeated option is E001. An empty `stmts` is E009. |
| `define` | `name` name; `params` [pdecl]; `shape` name?; `budget` name?; `body` query | define_stmt | `name` is the qname joined with `.`. Also the start symbol of std library files ([LQ/grammar-v1.ebnf §P.1]). |
| `query` | `parts` [part]+; `ops` [enum{union, union_all, except, intersect}] | query, set_op | `ops` has one entry fewer than `parts`; parts combine left to right. |
| `part` | `use` rev?; `clauses` [clause]; `return` return?; `call` scall? | single_query | Either `call` is present, `clauses` is empty and `return` absent (a standalone call), or `call` is absent and `return` present. |
| `scall` | `proc` name; `args` [arg]; `yield` enum{none, star, items} = none; `items` [yitem]; `where` expr?; `order` [sort]; `limit` expr? | standalone_call, order_limit | `items` is non-empty exactly when `yield` = `items`; `where` needs a `YIELD`. |
| `match` | `optional` bool; `mode` enum{walk, trail, acyclic, simple, different}?; `patterns` [path]+; `where` expr? | match_clause, optional_clause, match_mode | `mode` is absent for `OPTIONAL MATCH`. `DIFFERENT RELATIONSHIPS` and `DIFFERENT EDGES` both give `different`. |
| `call` | `proc` name; `args` [arg]; `yield` [yitem]+; `where` expr? | call_clause |  |
| `unwind` | `expr` expr; `as` name | unwind_clause |  |
| `with` | `distinct` bool; `star` bool; `items` [item]; `where` expr?; `order` [sort]; `limit` expr? | with_clause, proj_items, order_limit | `star` is the leading `*`. |
| `return` | `distinct` bool; `star` bool; `items` [item]; `group` [expr]; `order` [sort]; `limit` expr? | return_clause, proj_items, order_limit | `RETURN ALL` gives `distinct` = false. `group` holds the `GROUP BY` list. |
| `item` | `expr` expr; `as` name? | proj_item |  |
| `yitem` | `name` name; `as` name? | yield_items |  |
| `sort` | `expr` expr; `dir` enum{asc, desc} = asc | sort_item | `ASCENDING` gives `asc`, `DESCENDING` `desc`; no keyword gives `asc`. |
| `arg` | `name` name?; `value` argval | arg | `value` is a revision node only at a revision position ([LQ/lexical §4.2]). |
| `kv` | `key` name; `value` expr | prop_map, map_lit | One entry of a property map or map literal, in written order. |
| `when` | `cond` expr; `then` expr | case_expr |  |
| `path` | `start` npat; `steps` [step] | path, path_pred |  |
| `npat` | `var` name?; `labels` [name]; `props` [kv]; `where` expr? | node_pat, label_expr | `(#N)` and `(#u:...)` give `props` = [`id`: the literal] and no variable. An empty map gives `props` = []. |
| `estep` | `edge` epat; `node` npat | path |  |
| `gstep` | `group` group; `node` npat | path |  |
| `epat` | `var` name?; `dir` enum{right, left, both}; `types` [name]; `quant` quant?; `props` [kv]; `where` expr? | edge_pat, edge_body | `->` gives `right`, `<-` `left`, no arrowhead `both`. The Cypher `*` form and the postfix quantifier both give `quant`. |
| `group` | `path` path; `where` expr?; `quant` quant | group_pat |  |
| `quant` | `min` int; `max` int? | quantifier, edge_body | `max` absent = unbounded; the mapping is §3.1 item 4. |
| `or` | `l` expr; `r` expr | expr | Left-associative. |
| `and` | `l` expr; `r` expr | and_expr | Left-associative. |
| `not` | `e` expr | not_expr, pred_expr | `x NOT IN y` gives `not`(`in`(x, y)). |
| `cmp` | `op` enum{=, <>, <, <=, >, >=}; `l` expr; `r` expr | pred_expr, cmp_op | `!=` gives `<>`. |
| `isnull` | `neg` bool; `e` expr | pred_expr | `IS NOT NULL` gives `neg` = true. |
| `in` | `l` expr; `r` expr | pred_expr |  |
| `strpred` | `op` enum{starts, ends, contains}; `l` expr; `r` expr | pred_expr |  |
| `labeltest` | `e` expr; `labels` [name]+ | pred_expr, label_expr |  |
| `arith` | `op` enum{+, -, *, /}; `l` expr; `r` expr | add_expr, mul_expr | Left-associative. |
| `neg` | `e` expr | unary_expr |  |
| `prop` | `e` expr; `name` name | postfix_expr |  |
| `ident` | `name` name | primary, target | An identifier in expression position: a variable or a bare word the binder coerces. |
| `param` | `name` name | param | The name without `$`. |
| `nid` | `n` int | node_lit | `#N`; 1 <= n <= 4294967295. |
| `uid` | `hex` str | node_lit | `#u:`; 32 lower-case hex digits. |
| `int` | `v` int | literal | 0 <= v <= 9223372036854775807. |
| `float` | `v` str | literal | The literal as written ([LQ/lexical §5.6]). |
| `str` | `v` str | literal | The decoded value. |
| `dur` | `v` str | literal | The literal as written, e.g. `3d`. |
| `bool` | `v` bool | literal |  |
| `null` | (none) | literal | The `NULL` literal. |
| `exists` | `sub` sub | primary, path_pred, func_call | Also from a pattern predicate and from `exists(path)`. |
| `countsub` | `sub` sub | primary, func_call | Also from `size(path)`. |
| `subq` | `clauses` [clause]; `return` return? | subquery | Clause form. |
| `subp` | `patterns` [path]+; `where` expr? | subquery, path_pred | Pattern form. |
| `fn` | `name` name; `distinct` bool; `args` [arg] | func_call | Name as written. |
| `countstar` | (none) | func_call | `count( * )`. |
| `listpred` | `kind` enum{all, any, none}; `var` name; `list` expr; `pred` expr | func_call |  |
| `list` | `elems` [expr] | list_lit |  |
| `map` | `entries` [kv] | map_lit |  |
| `case` | `subject` expr?; `whens` [when]+; `else` expr? | case_expr |  |
| `rhead` | (none) | rev_base | `HEAD`. |
| `rref` | `name` str | rev_base, ref_name |  |
| `rcommit` | `hex` str | rev_base, commit_lit | The hex digits without `c`; 7 to 64 of them. |
| `rseq` | `n` int | rev_base, seq_lit |  |
| `rsuf` | `base` rev; `kind` enum{tilde, caret, at, attime}; `n` int?; `time` str? | revspec, rev_suffix | Suffixes nest left to right. `n` is present for `tilde`, `caret` and `at` (an omitted count is 1); `time` for `attime`, as `YYYY-MM-DDTHH:MM:SSZ`. |
| `rrange` | `from` rev; `op` enum{two, three}; `to` rev | rev_arg |  |
| `rlist` | `elems` [rev]+ | rev_arg |  |
| `smatch` | `patterns` [path]+; `where` expr?; `expect` expect; `muts` [mut]+ | tx_stmt |  |
| `smuts` | `muts` [mut]+ | tx_stmt |  |
| `screate` | `var` name; `label` name; `props` [kv]; `edges` [cedge]; `under` target?; `unless` sub? | create_stmt | `INSERT` gives the same node. |
| `cedge` | `dir` enum{right, left}; `type` name; `props` [kv]; `target` target | create_stmt, edge_step |  |
| `stxcall` | `name` name; `args` [arg]; `yield` [yitem] | tx_stmt, tx_name | `name` without the `tx.` prefix. |
| `sassert` | `expr` expr; `else` str? | tx_stmt |  |
| `sresolve` | `key` str?; `query` query?; `expect` expect?; `take` enum{ours, theirs, base, value, repoint}; `value` expr?; `target` target? | resolve_stmt | Either `key`, or `query` with `expect`. `value` exactly for `value`, `target` exactly for `repoint`. |
| `sdrop` | `name` name | tx_stmt, qname |  |
| `mset` | `assigns` [assign]+ | mutation |  |
| `assign` | `target` target; `prop` name; `value` expr | mutation |  |
| `mremove` | `items` [tprop]+ | mutation |  |
| `tprop` | `target` target; `prop` name | mutation |  |
| `mdelete` | `targets` [target]+; `opts` [dopt] | mutation, delete_opt | Options in written order; a repeated option kind is E001. |
| `dpolicy` | `v` enum{restrict, cascade, reparent} | delete_opt |  |
| `dreplaced` | `target` target | delete_opt |  |
| `drelease` | (none) | delete_opt |  |
| `dreason` | `expr` expr | delete_opt |  |
| `mmove` | `target` target; `under` target; `pos` enum{before, after, first, last}?; `rel` target? | mutation | `rel` exactly for `before` and `after`. |
| `medge` | `src` target; `dir` enum{right, left}; `type` name; `props` [kv]; `dst` target | mutation, edge_step | `CREATE (a)-[:T]->(b)`; `INSERT` gives the same node. |
| `mreopen` | `target` target; `reason` expr | mutation |  |
| `mpatch` | `target` target; `field` name; `remove` expr; `add` expr | mutation |  |
| `expect` | `kind` enum{exact, range, le, ge, param}; `a` int?; `b` int?; `param` name? | expect | `exact`: `a`; `range`: `a` and `b`; `le`, `ge`: `a`; `param`: `param`. |
| `pdecl` | `name` name; `type` ptype; `optional` bool; `default` lit? | param_decl | `name` without `$`. `default` absent means no `=` was written; `= NULL` gives the `null` node. |
| `ptype` | `name` name; `arg` name? | type |  |

### 3.3 Unions

| Union | Members |
|---|---|
| `expr` | `or and not cmp isnull in strpred labeltest arith neg prop ident param nid uid int float str dur bool null exists countsub fn countstar listpred list map case` |
| `rev` | `rhead rref rcommit rseq rsuf param` |
| `argval` | `expr`, and `rhead rref rcommit rseq rsuf rrange rlist` (the latter only at a revision position, [LQ/lexical §4.2]) |
| `clause` | `match call unwind with` |
| `step` | `estep gstep` |
| `sub` | `subq subp` |
| `stmt` | `smatch smuts screate stxcall sassert sresolve define sdrop` |
| `mut` | `mset mremove mdelete mmove medge mreopen mpatch` |
| `dopt` | `dpolicy dreplaced drelease dreason` |
| `target` | `ident nid uid param` |
| `lit` | `int float str dur bool null nid uid` |

### 3.4 Invariants and the printer property

- The parser only builds trees that satisfy the notes of §3.2 (for example `part`'s two forms, `scall`'s `yield`/`items`
  pairing, `sresolve`'s key-or-query, `expect`'s fields per kind, `mmove`'s `rel` with `before`/`after`, `rsuf`'s
  `n`/`time`, `query`'s `ops` length). The JSON IR reader checks the same invariants and refuses a violation with E001
  ([LQ/json-ir §5]).
- **Printer property** ([50 §8.2] LQ-1, [50 §8.3], PLAN WP-93a): for every S-AST `a`, `parse(print(a)) == a`, where
  `print` is the display printer in either display spelling and `==` ignores spans. The printer back-quotes every name
  that is not a plain word or that is a reserved word ([LQ/lexical §6.1]); prints strings in single quotes, escaping
  `\`, `'`, LF, CR, HT and the controls of [LQ/lexical §5.2] (the latter as `\u{…}`); prints floats and durations as
  their text; and prints `not`(`in`(x, y)) as `NOT x IN y`.

## 4. S-expression form (AST streams in fixtures)

### 4.1 Syntax

```
sexpr  = "(" tag *( ws field ) ")"
field  = sexpr / list / string / int / atom / "_"
list   = "[" [ field *( ws field ) ] "]"
tag    = 1*( %x61-7A / "_" )            ; S-AST: lower case
       / 1*( %x41-5A / "_" )            ; C-AST: upper case
atom   = 1*( %x61-7A / "_" ) / "=" / "<>" / "<" / "<=" / ">" / ">=" / "+" / "-" / "*" / "/"
int    = [ "-" ] 1*DIGIT
string = a JSON string written by the rule of [LQ/lexical §11.1]
ws     = 1*( SP / HTAB / LF )
```

Fixtures may break lines and indent anywhere `ws` is allowed. Two S-expressions are equal when their token sequences
are equal (whitespace between tokens ignored).

### 4.2 Rendering the S-AST

A node renders as `(`, its tag, then each field of §3.2 in order, then `)`. `bool` renders `true`/`false`; `int`
decimal; `str` and `name` as a JSON string; an enum value as its atom (`right`, `union_all`, `<=`, `+`); an absent
optional field as `_`; a list as `[…]`. Spans are not rendered.

### 4.3 Rendering the C-AST

The same rules apply to the tags and fields of §6.3, with these additions: a tuple element of a list renders as
`[a b …]`; a `u32` variable or item index as an int; `uid` and commit ids as JSON strings of lower-case hex (32 and 64
digits); a float as the JSON string of its 16 lower-case hex digits of the IEEE 754 bits; an enum code as the atom
§6.4 gives it.

### 4.4 Example

`MATCH (t:task) WHERE t.status = 'open' AND t.priority <= 1 RETURN t ORDER BY t.priority LIMIT 5` parses to

```
(read run
 (query
  [(part _
    [(match false _
      [(path (npat "t" ["task"] [] _) [])]
      (and (cmp = (prop (ident "t") "status") (str "open"))
           (cmp <= (prop (ident "t") "priority") (int 1))))]
    (return false false [(item (ident "t") _)] [] [(sort (prop (ident "t") "priority") asc)] (int 5))
    _)]
  []))
```

and `match (x:Task) where x.status = open and x.priority <= 'P1' return x order by x.priority asc limit 5` parses to
the same tree with `"x"` for `"t"`, `["Task"]` for `["task"]`, `(ident "open")` for `(str "open")` and `(str "P1")` for
`(int 1)`. Bound against the core schema ([F08]: `status` an enum with the value `open`; `priority` with `coerce` =
priority, F2), both give the one C-AST of §6.5:

```
(QUERY
 (PART _
  (CLAUSES
   [(MATCH false
     [(PATH (NODEP 0 ["task"] [] _) [])]
     (AND (CMP = (PROP (VAR 0) "status") (ENUM "open"))
          (CMP <= (PROP (VAR 0) "priority") (INT 1))))]
   (RETURN false false [(RITEM (VAR 0) _)] [(SORT (PROP (VAR 0) "priority") false)] (INT 5))))
 [])
```

## 5. From the S-AST to the C-AST

### 5.1 Binding context

The binder supplies, and the encoder never computes:

1. the **schema version of the view** of each query part ([50 §3.2], [50 §3.9] item 5): kinds; fields with their types
   and F2 rows (`optional`, `coerce`); edge kinds with their F1 rows (`lq_name`, `reverse_names`, `symmetric`); the
   named-query catalog (F3); the built-in and relation registry; the signatures of [LQ/std];
2. the **store's identity maps**: `#N` → uid (the `UID` column, `ALLOC`, [AR §3.1], F17), and sequence number or
   commit prefix → full commit id;
3. the **bound parameter values** and their use-site types ([50 §3.2]);
4. the **scope resolution** of §5.7.

### 5.2 Elements removed

| S-AST element | Why |
|---|---|
| `read.mode` (`EXPLAIN`, `PROFILE`) | a mode, not a query ([50 §5.11]); the EXPLAIN id is the query's hash |
| `match.mode` | changes nothing ([50 §3.4] rule 2, [50 §3.7] rule 4) |
| `return.group` | a checked restatement of the implicit grouping ([50 §2.7]); the binder checks it first |
| `tx.on`, `tx.key`, `tx.lease`, `tx.dry` | bound outside the payload: the branch and the key by the idempotency record ([AR §6.4]), the lease by the fencing check, `DRY` never commits (Open point C-2) |
| the display name of an unaliased `RETURN` column | output rendering, derived by the display printer (Open point C-3) |
| variable, parameter and alias names other than `RETURN` aliases | renamed (§5.7) or substituted (§5.5) |
| spans, and the S-AST's written spellings of names | replaced by canonical names (§5.3, §5.4) |

### 5.3 Names

| S-AST name | C-AST name |
|---|---|
| a label (`npat.labels`, `labeltest.labels`, `screate.label`) | the kind's declared name in the schema, matched ASCII-case-insensitively ([LQ/lexical §9]); the pseudo-label is `DELETED` |
| an edge type | §5.4 |
| a scalar or aggregate function (`fn.name`) | the canonical name of Table 5.3 |
| a procedure (`call.proc`, `scall.proc`) | the resolved callee: a relation's registry name (Table 5.3, e.g. `blockers`); a standard named query `std.<name>`; a project named query its `qname` (Open point C-6) |
| a named mutation (`stxcall.name`) | `tx.<name>` |
| property names, `kv` keys, yield field names, named-argument names, `RETURN` aliases, `define.name`, `pdecl.name`, `sdrop.name` | exactly as written (decoded) |
| `define.shape`, `define.budget`, `ptype.name`, `ptype.arg` | ASCII lower case |

**Table 5.3 — canonical names of built-ins** ([50 §2.6]). Lookup is ASCII-case-insensitive; the canonical name is the
spelling in the first column, byte for byte.

| Canonical | Also accepted | Kind |
|---|---|---|
| `subtree`, `descendants`, `ancestors`, `children` | — | scalar (set of nodes) |
| `applies`, `applies_role`, `applies_phase`, `fits_role`, `glob_match`, `text_match` | — | scalar (bool) |
| `file`, `link_state`, `staleness`, `relevant_to`, `me`, `view_ref`, `date`, `duration` | — | scalar |
| `now` | `datetime` with no argument | scalar |
| `datetime` | — (with one argument) | scalar |
| `size` | `cardinality`, `length` | scalar |
| `lower` | `toLower` | scalar |
| `upper` | `toUpper` | scalar |
| `trim`, `substring`, `coalesce`, `round`, `abs`, `toString`, `toInteger`, `toFloat`, `id`, `labels`, `type` | — | scalar |
| `count`, `sum`, `min`, `max`, `avg` | — | aggregate |
| `collect` | `collect_list` | aggregate |
| `blockers`, `subtree`, `neighbors`, `search`, `history`, `blame`, `log`, `diff`, `changes`, `conflicts`, `violations`, `across`, `refs`, `leases`, `markers`, `links`, `root_moves`, `schema`, `schema_edges`, `queries` | — | relation (table function) |

Function names and relation names are separate namespaces ([50 §2.6]): `subtree` is both, with different defaults. The
scalar `subtree(n [, depth])` has no depth bound when `depth` is omitted; the relation `subtree(n, depth: 3)` defaults to 3.
The scalar signatures are [LQ/std §2.10]'s, the relation signatures [LQ/std §2.9]'s (pass 1, A1-54).

### 5.4 Edge types and directions

Each written type name is resolved against the view's edge schema (F1), ASCII-case-insensitively, in this order: the
kind's `lq_name`; its forward synonyms (the table below); its stored snake-case name (`derived_from`, [50 §2.5]); its
`reverse_names`. The C-AST names the kind by its `lq_name`. `parent` and `PARENT` are E107; an unknown name is E104
([50 §2.5]).

**Forward synonyms** (grammar version 1; pass 1, A1-56, closing Open point C-8). A forward synonym names a kind in its
forward direction (no direction flip) and is part of LQ, not of the edge schema: F1's `reverse_names` cannot hold it
([F08] open point 43), project edge kinds have none, and a later grammar version may add rows.

| Synonym | Resolves to (`lq_name`) | Source |
|---|---|---|
| `SUBTASK_OF` | `CHILD_OF` | [50 §2.5] |

**Patterns (`epat`).** Each type gets an effective direction from the written direction and the name's orientation:

| Written | Forward name (`lq_name`, synonym, stored name) | Reverse alias |
|---|---|---|
| `-[:T]->` (`right`) | `right` | `left` |
| `<-[:T]-` (`left`) | `left` | `right` |
| `-[:T]-` (`both`) | `both` | `both` |

A kind whose F1 row is `symmetric` (`CONTRADICTS`, `RELATES`) gets `both` whatever is written ([50 §2.5] item 3,
[50 §3.4] rule 3). The C-AST `EDGEP` carries the list of (`lq_name`, effective direction) in written order and a
pattern-level direction of 0; a pattern with no type (`-->`, `<--`, `--`, `-[e]->`) carries an empty list and the written
direction. So `(#51)-[:BLOCKED_BY]->(b)` and `(#51)<-[:BLOCKS]-(b)` have one C-AST, as [50 §3.2] requires ("the
canonical form is what the reading echo, EXPLAIN and the query hash see"), and `[:BLOCKS|BLOCKED_BY]` keeps both
readings.

**Writes.** A created edge is stored in one direction, so the symmetric rule does not apply to writes:
- `cedge` (the edges of `CREATE (x:kind …)-[:T]->(y)`): the effective direction relative to the created node, by the
  table above, is encoded as 1 (out: the created node is the source) or 2 (in).
- `medge` (`CREATE (a)-[:T]->(b)`): the C-AST `MEDGE` is (source, `lq_name`, destination) in the stored direction:
  `(a)-[:T]->(b)` with a forward name is (a, T, b); a left arrow or a reverse alias swaps the two; both swap back.

### 5.5 Constants, coercions and parameters

**Literals.** Without coercion: `int` → `INT`; `float` → `FLOAT` (the correctly rounded binary64 value,
[LQ/lexical §5.6]; a literal is never negative and never a NaN, so the canonical rules of [F01 §5.5] have nothing to do
there); `str` → `TEXT` (the decoded bytes, no Unicode normalisation); `dur` → `DURATION` (milliseconds);
`bool` → `BOOL`; `null` → `NULL`; `nid` and `uid` → `NODE` (the node's uid, 16 bytes; a number never allocated or a uid
this store does not know is E111). A `neg` of a literal stays `NEG`: nothing is folded.

**Coercion** ([50 §3.2]; F2 `coerce`). The binder decides the type of a literal or bare word from the other operand or
the use site; the C-AST records the resulting constant:

| Use-site type | S-AST forms accepted | C-AST |
|---|---|---|
| an enum field, an enum parameter | `str`, an unbound `ident` | `ENUM` with the value's declared name |
| `priority` (F2 `coerce` = priority) | `int` n, `str` or `ident` `P<n>` | `INT` n |
| a revision (F2 `coerce` = revision-integer; a `rev` parameter; a revision-typed yielded column) | `int` n (a sequence number), `str` or unbound `ident` of revision shape | the revision nodes of §5.6 |
| a timestamp | `str` in ISO 8601 date or date-time form | `TIMESTAMP`, milliseconds since the Unix epoch, UTC |
| a node, or the property `id` (`{id: 40}`, `n.id = 40`) | `int` N, `nid`, `uid` | `NODE` (uid) |
| anything else | the literal | the uncoerced constant above |

A bound variable always wins over coercion ([50 §3.2]); an unbound `ident` that nothing coerces is a bind error.

**Parameters.** In a read query, a `TX` and a call, every `param` is replaced by the constant of its bound value, typed
by its use site ([50 §3.2]; [50 §5.3] "parameter values substituted with their types"):

| Parameter type | C-AST |
|---|---|
| `node` | `NODE` |
| `list<node>`, `list<int>`, `list<text>`, `list<rev>` ([LQ/std §2.2]) | `LIST` of the element constants |
| `int` | `INT` |
| `float` | `FLOAT`; the value −0.0 is encoded as +0.0, and a NaN is refused (E110), as [F01] open point 8 proposes for values that enter a canonical form |
| `range<int>` | `RANGEINT` (`..1` → lo absent, hi 1; `2..` → lo 2, hi absent) |
| `bool` | `BOOL` |
| `text` | `TEXT`, or `ENUM` when the use site is an enum (the coercion table applies to values too) |
| an enum | `ENUM` |
| `rev` | the revision nodes of §5.6 |
| `timestamp`, `duration` | `TIMESTAMP`, `DURATION` |
| absent (an optional parameter not given, a JSON `null`) | `NULL` |

A query with `$p` bound to a value and the same query with an equal literal written in place therefore have the same
C-AST. Inside a `DEFINE` body — and only there — a parameter stays a parameter: `PARAM(i)`, where i is the 0-based
position of its declaration in `params`; declaration defaults are constants.

### 5.6 Revisions

| S-AST | C-AST |
|---|---|
| `rhead` | `RHEAD` |
| `rref(name)` | `RREF(name)` |
| `rcommit(hex)` | `RCOMMIT` with the full 32-byte id of the unique commit whose id starts with the hex digits (E301 if none or several) |
| `rseq(n)` | `RCOMMIT` with the id of the commit whose store sequence number is n (E301 if none) |
| `rsuf(base, tilde \| caret, n)` | `RSUF(base′, 1 \| 2, n)` |
| `rsuf(base, at, n)` | `RSUF(base′, 3, n)` — store-local, E117 inside a definition |
| `rsuf(base, attime, t)` | `RSUF(base′, 4, milliseconds of t)` — store-local, E117 inside a definition |
| `rrange(a, two \| three, b)` | `RRANGE(a′, 1 \| 2, b′)` |
| `rlist(…)` | `RLIST(…)` |
| a `rev` parameter's value, a coerced `str`, `ident` or `int` | the value read with the revision grammar of [LQ/lexical §7] (an `int` n reads as `s<n>`), then this table |

Sequence numbers and commit prefixes are resolved because they are store-local (a prefix may be ambiguous in another
store, [50 §4.4]); refs, `HEAD` and suffixes stay symbolic, because resolving them is the view's job at run time
(Open point C-13). `--at REV` and the MCP `use` parameter supply a `use` for every part that has none, before
canonicalisation ([50 §3.9] item 1).

### 5.7 Variables and scopes

**Index assignment.** Each root (§5.9) has one counter starting at 0. The encoder writes the fields of §6.3 in order;
the first time it writes a field that refers to a binding — a binding site (a node or edge pattern variable, a `YIELD`
variable, an `UNWIND` variable, a `WITH` item variable, a list-predicate variable, a `CREATE` variable) or a `VAR`
reference — the binding gets the counter's value, and the counter increases. Every later reference to the same binding
writes that index. Anonymous pattern elements get no index. Which occurrence refers to which binding is decided by the
scope rules below — the binder's name resolution, stated here because renaming depends on it (Open point C-14):

| # | Rule |
|---|---|
| V1 | A query part is a scope. Each part of a composite query, the query of a `RESOLVE (…)`, and a `DEFINE` body have their own scopes. |
| V2 | In the patterns of `MATCH`, `OPTIONAL MATCH` and a `TX` `MATCH`, a named node or edge variable that is not visible creates a binding; one that is visible refers to it (a join). The clause's patterns and its `WHERE` see its new bindings; later clauses see them too. |
| V3 | In a quantified group every named variable of the group's path is a new binding local to the group (its path and its `WHERE`); the group's `WHERE` may also refer to bindings visible before the clause ([50 §3.7] rule 3; outside the group they are E116). |
| V4 | `WITH`: its items are evaluated in the scope before it. Afterwards the visible bindings are its items — an aliased item is a new binding named by the alias; an unaliased item that is a bare variable re-exports that same binding (same index); an unaliased item that is not a bare variable is a bind error, as in Cypher (Open point C-21) — plus, with `*`, every binding visible before. The `WITH`'s `WHERE` and `ORDER BY` resolve a name first among its items, then, if it has no `DISTINCT` and no aggregate, among the bindings visible before it. |
| V5 | `RETURN`: items and `GROUP BY` are evaluated in the scope before it. Its `ORDER BY` resolves a name first to an aliased item of the same `RETURN` — encoded `ITEMREF(i)`, i the 0-based item index — then among the bindings visible before it. |
| V6 | `CALL … YIELD` (clause and standalone) and `CALL tx.… YIELD`: each yield item is a new binding named by its alias or else its field name; a standalone call's `WHERE` and `ORDER BY` see them. |
| V7 | `UNWIND … AS x`: a new binding. |
| V8 | A subquery (`EXISTS {}`, `COUNT {}`, `UNLESS EXISTS {}`, a pattern predicate) sees every binding visible where it stands; its new bindings are local to it. In the `UNLESS EXISTS` subquery of a `create_stmt`, a binding whose name equals the created variable's name gets the created variable's index: create-or-bind links the two by name ([50 §3.10] table, `CREATE` row). |
| V9 | A list predicate `all(x IN l WHERE p)`: `x` is a new binding local to `p`. |
| V10 | A `TX` block is one scope: its statements run in order ([50 §3.10] item 2); the pattern variables of a `MATCH … EXPECT`, the variable of a `CREATE` and the yields of a `CALL tx.…` are visible to the rest of that statement and to every later statement. |
| V11 | A name that resolves to no binding is a coerced bare word (§5.5) or a bind error; it gets no index. |

Example: `MATCH (a)-[:BLOCKS]->(b) WITH b AS x MATCH (x)<-[:CHILD_OF]-(c) RETURN c` numbers `a` 0, `b` 1, `x` 2, `c` 3;
the same query with every variable renamed has the same indexes.

### 5.8 Structural normalisations

| # | Normalisation |
|---|---|
| N1 | A clause-form subquery (`subq`) whose only clause is a non-optional `MATCH` and which has no `RETURN` becomes the pattern form (`SUBP` with that clause's patterns and `WHERE`): `EXISTS { MATCH (a)-->(b) }` = `EXISTS { (a)-->(b) }`. |
| N2 | Arguments of a call to a **relation, named query or named mutation** are named by the callee's parameters ([LQ/std] signatures; positional arguments bind the parameters in declaration order) and listed in the callee's declaration order. An argument the caller omitted stays omitted: defaults are not filled in. Arguments of scalar and aggregate functions stay positional, in written order. (Open point C-5.) |
| N3 | `DELETE` options go into fixed slots (`policy`, `replaced_by`, `release`, `reason`), so their written order does not matter; a repeated option is E001 at parse time. |
| N4 | `MOVE … [BEFORE y \| AFTER y \| FIRST \| LAST]` becomes a position code and an optional relative target. |
| N5 | `EXPECT` becomes (min, max): `n` → (n, n); `m..n` → (m, n); `<= n` → (0, n); `>= n` → (n, unbounded); `$p` → by its value: an int n → (n, n), a `range<int>` → (lo or 0, hi). |
| N6 | Created edges take the stored direction (§5.4). |
| N7 | Kept exactly as written: the order of patterns, pattern elements, labels, edge types, map and property-map entries, list elements, `SET` assignments and `REMOVE` items, statements, `CASE` arms, yield items, projection items and sort keys, and the operands of every operator ([50 §5.3]). |

### 5.9 Roots and entry forms

| # | Input | Root of the C-AST |
|---|---|---|
| R1 | `read_input` (`moirai q`, MCP `query` with `q`) | the `QUERY` node; the mode is dropped (§5.2) |
| R2 | a named query by name with `k=v` parameters (`moirai q NAME k=v`, MCP `query` with `name` and `params`, a read verb, [50 §6.1], [90 §6.6]) | the `QUERY` of the standalone call `CALL <name>(k: v, …)` with no `YIELD`, the values being the parameters coerced by the signature |
| R3 | `write_input` (`moirai tx`, MCP `write` with `tx`) | the `TX` node. CLI flags and MCP fields first join the block: `--if-tip`/`if_tip` as `IF TIP`, `--dry-run`/`dry_run` as `DRY`, `--idempotency-key`/`idempotency_key` as `KEY`, `--lease`/`lease` as `LEASE`, `--branch`/`branch` as `ON` |
| R4 | a named mutation by name with parameters (MCP `write` with `name` and `params`) | the `TX` of `TX { CALL tx.<name>(k: v, …) }`, joined with the tool's fields as in R3 |
| R5 | a write verb (`moirai set`, `complete`, `rm`, …) | the `TX` of the verb's expansion in [LQ/std] ([50 §4.2]), joined with its flags as in R3 |
| R6 | a named-query definition, for its F3 hash | the `DEFINE` node, over the portable form (§8) |

A `DEFINE` inside a `TX` is also a statement of that `TX`'s C-AST. By R2–R5, the same write reaches the same hash
through the CLI verb, `moirai tx` and MCP `write`, which is what makes one idempotency key work across the three doors
([AR §6.4]: "every write verb, `apply` batch and MCP `write` compiles to one LQ `TX` block").

## 6. Binary encoding

### 6.1 Header

The encoding is the fixed 22-byte header below followed by exactly one root node (§5.9): a `QUERY`, `TX` or `DEFINE`
node. All integers are little-endian ([F01 §4.1]).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 4 | `u32` | `magic_len` | 16, the byte length of `magic` |
| 4 | 16 | `[16]u8` | `magic` | the ASCII bytes `moirai-lq-ast-v1`; offsets 0–19 are `lp("moirai-lq-ast-v1")`, the domain-separation prefix [F01 §7.1] asks for |
| 20 | 2 | `u16` | `lq` | the grammar version whose canonical-AST algorithm this is: 1 |
| total | 22 | | | |

The encoding has no padding, no alignment, no reserved byte and no trailing byte after the root. It is never stored:
it exists only as the input of the hash of §7, which is why it may use `lp()` ([F01 §6.3]).

**Why the framing is unambiguous** ([F01 §7.3]). Every node begins with a tag that fixes its field sequence (§6.3); every
variable-length field is length-prefixed (`lp()` strings), count-prefixed (lists) or presence-prefixed (`opt`); no field
is optional without a presence byte. The decoding of an encoding is therefore unique, two different C-ASTs never have
the same encoding, and the header's `magic` separates this input from every other BLAKE3 input of the format.

### 6.2 Primitive encodings

| Notation | Encoding |
|---|---|
| `u8`, `u16`, `u32`, `u64`, `i64` | [F01 §5.1]: fixed width, little-endian; `i64` two's complement |
| `bool8` | [F01 §5.4]: `00` false, `01` true; any other value is not an encoding |
| `f64` | [F01 §5.5]: the IEEE 754 binary64 pattern as a `u64`; in a C-AST never a NaN and never −0.0 (§5.5) |
| `str` | `lp()` of [F01 §6.3]: `u32` byte length, then the UTF-8 bytes, with no terminator and no normalisation |
| `opt<T>` | `u8` 0 (absent), or `u8` 1 followed by T |
| `list<T>` | `u32` count, then the elements |
| `(A, B, …)` | a tuple: A, then B, …, with no tag and no length |
| `node` | `u8` tag (§6.3), then the tag's fields in order |
| `var` | `u32` variable index (§5.7) |
| `b16` | [F01 §5.6]: a node's uid, 16 bytes in byte order |
| `b32` | [F01 §5.6]: a full commit id, 32 bytes in byte order |

### 6.3 Tags

A field written `node` holds a node of any tag that the S-AST position allows after §5; `expr` means an expression,
constant or revision node.

This table is a set of sequence tables ([F01 §2.6]) written one row per tag: the fields follow each other in the order
given, with no padding, and every field is always present (an optional part is an `opt<T>`, whose presence byte is part
of the encoding). The departure from one table per structure, with columns `order | name | encoding | present when |
meaning`, is for compactness only; it is stated here as [F01 §2.4] rule 5 requires.

| Tag | Name | Fields, in order | From |
|---|---|---|---|
| 0x01 | `QUERY` | first: node `PART`; rest: list<(op u8, node `PART`)> | `query` |
| 0x02 | `PART` | use: opt<node rev>; body: node `CLAUSES` or `SCALL` | `part` |
| 0x03 | `CLAUSES` | clauses: list<node>; ret: node `RETURN` | `part` |
| 0x04 | `SCALL` | proc: str; args: list<node `ARG`>; ymode: u8; items: list<node `YIELD`>; where: opt<expr>; order: list<node `SORT`>; limit: opt<expr> | `scall` |
| 0x10 | `MATCH` | optional: bool8; patterns: list<node `PATH`>; where: opt<expr> | `match` |
| 0x11 | `CALL` | proc: str; args: list<node `ARG`>; items: list<node `YIELD`>; where: opt<expr> | `call` |
| 0x12 | `UNWIND` | expr: expr; var: var | `unwind` |
| 0x13 | `WITH` | distinct: bool8; star: bool8; items: list<node `WITEM`>; where: opt<expr>; order: list<node `SORT`>; limit: opt<expr> | `with` |
| 0x14 | `RETURN` | distinct: bool8; star: bool8; items: list<node `RITEM`>; order: list<node `SORT`>; limit: opt<expr> | `return` |
| 0x15 | `WITEM` | expr: expr; var: var | `item` of a `WITH` |
| 0x16 | `RITEM` | expr: expr; alias: opt<str> | `item` of a `RETURN` |
| 0x17 | `YIELD` | field: str; var: var | `yitem` |
| 0x18 | `SORT` | expr: expr; desc: bool8 | `sort` |
| 0x19 | `ARG` | name: opt<str>; value: expr | `arg` |
| 0x20 | `PATH` | start: node `NODEP`; steps: list<node `ESTEP` or `GSTEP`> | `path` |
| 0x21 | `NODEP` | var: opt<var>; labels: list<str>; props: list<(str, expr)>; where: opt<expr> | `npat` |
| 0x22 | `ESTEP` | edge: node `EDGEP`; node: node `NODEP` | `estep` |
| 0x23 | `GSTEP` | group: node `GROUP`; node: node `NODEP` | `gstep` |
| 0x24 | `EDGEP` | var: opt<var>; dir: u8; types: list<(str, u8)>; quant: opt<node `QUANT`>; props: list<(str, expr)>; where: opt<expr> | `epat` |
| 0x25 | `GROUP` | path: node `PATH`; where: opt<expr>; quant: node `QUANT` | `group` |
| 0x26 | `QUANT` | min: u32; max: opt<u32> | `quant` |
| 0x30 | `OR` | l: expr; r: expr | `or` |
| 0x31 | `AND` | l: expr; r: expr | `and` |
| 0x32 | `NOT` | e: expr | `not` |
| 0x33 | `CMP` | op: u8; l: expr; r: expr | `cmp` |
| 0x34 | `ISNULL` | neg: bool8; e: expr | `isnull` |
| 0x35 | `IN` | l: expr; r: expr | `in` |
| 0x36 | `STRPRED` | op: u8; l: expr; r: expr | `strpred` |
| 0x37 | `LABELTEST` | e: expr; labels: list<str> | `labeltest` |
| 0x38 | `ARITH` | op: u8; l: expr; r: expr | `arith` |
| 0x39 | `NEG` | e: expr | `neg` |
| 0x3A | `PROP` | e: expr; name: str | `prop` |
| 0x3B | `VAR` | idx: var | `ident` naming a binding |
| 0x3C | `PARAM` | idx: u32 | `param` inside a `DEFINE` body |
| 0x3D | `ITEMREF` | idx: u32 | `ident` in a `RETURN`'s `ORDER BY` naming an alias (V5) |
| 0x3E | `EXISTS` | sub: node `SUBC` or `SUBP` | `exists` |
| 0x3F | `COUNTSUB` | sub: node `SUBC` or `SUBP` | `countsub` |
| 0x40 | `SUBC` | clauses: list<node>; ret: opt<node `RETURN`> | `subq` |
| 0x41 | `SUBP` | patterns: list<node `PATH`>; where: opt<expr> | `subp`, N1 |
| 0x42 | `FUNC` | name: str; distinct: bool8; args: list<node `ARG`> | `fn` |
| 0x43 | `COUNTSTAR` | (none) | `countstar` |
| 0x44 | `LISTPRED` | kind: u8; var: var; list: expr; pred: expr | `listpred` |
| 0x45 | `LIST` | elems: list<expr> | `list`; list-typed parameter values |
| 0x46 | `MAP` | entries: list<(str, expr)> | `map` |
| 0x47 | `CASE` | subject: opt<expr>; whens: list<(expr, expr)>; else: opt<expr> | `case` |
| 0x50 | `NULL` | (none) | `null`; absent parameter values |
| 0x51 | `BOOL` | v: bool8 | `bool` |
| 0x52 | `INT` | v: i64 | `int`; coerced priorities |
| 0x53 | `FLOAT` | v: f64 | `float` |
| 0x54 | `TEXT` | v: str | `str` |
| 0x55 | `DURATION` | ms: i64 | `dur` |
| 0x56 | `TIMESTAMP` | ms: i64 | a coerced `str` |
| 0x57 | `NODE` | uid: b16 | `nid`, `uid`, a coerced `int` |
| 0x58 | `ENUM` | name: str | a coerced `str` or `ident` |
| 0x59 | `RANGEINT` | lo: opt<i64>; hi: opt<i64> | a `range<int>` parameter value |
| 0x60 | `RHEAD` | (none) | `rhead` |
| 0x61 | `RREF` | name: str | `rref` |
| 0x62 | `RCOMMIT` | id: b32 | `rcommit`, `rseq` |
| 0x63 | `RSUF` | base: node rev; kind: u8; n: i64 | `rsuf` (n is the count, or the milliseconds for kind 4) |
| 0x64 | `RRANGE` | from: node rev; op: u8; to: node rev | `rrange` |
| 0x65 | `RLIST` | elems: list<node rev> | `rlist` |
| 0x70 | `TX` | if_tip: opt<node rev>; if_targets: opt<str>; message: opt<str>; stmts: list<node> | `tx` |
| 0x71 | `SMATCH` | patterns: list<node `PATH`>; where: opt<expr>; expect: node `EXPECT`; muts: list<node> | `smatch` |
| 0x72 | `SMUTS` | muts: list<node> | `smuts` |
| 0x73 | `SCREATE` | var: var; kind: str; props: list<(str, expr)>; edges: list<(dir u8, kind str, props list<(str, expr)>, target node)>; under: opt<node>; unless: opt<node `SUBC` or `SUBP`> | `screate`, `cedge` |
| 0x74 | `STXCALL` | name: str; args: list<node `ARG`>; items: list<node `YIELD`> | `stxcall` |
| 0x75 | `SASSERT` | expr: expr; else: opt<str> | `sassert` |
| 0x76 | `SRESOLVE` | key: node `TEXT` or `RESOLVEQ`; take: u8; operand: opt<node> | `sresolve` (operand: the value expr for take 4, the target for take 5) |
| 0x77 | `RESOLVEQ` | query: node `QUERY`; expect: node `EXPECT` | `sresolve` with a query |
| 0x78 | `SDROP` | name: str | `sdrop` |
| 0x80 | `MSET` | assigns: list<(target node, prop str, value expr)> | `mset`, `assign` |
| 0x81 | `MREMOVE` | items: list<(target node, prop str)> | `mremove`, `tprop` |
| 0x82 | `MDELETE` | targets: list<node>; policy: u8; replaced_by: opt<node>; release: bool8; reason: opt<expr> | `mdelete`, N3 |
| 0x83 | `MMOVE` | target: node; under: node; pos: u8; rel: opt<node> | `mmove`, N4 |
| 0x84 | `MEDGE` | src: node; kind: str; props: list<(str, expr)>; dst: node | `medge`, §5.4 |
| 0x85 | `MREOPEN` | target: node; reason: expr | `mreopen` |
| 0x86 | `MPATCH` | target: node; field: str; remove: expr; add: expr | `mpatch` |
| 0x87 | `EXPECT` | min: u64; max: opt<u64> | `expect`, N5 |
| 0x90 | `DEFINE` | name: str; params: list<node `PDECL`>; shape: opt<str>; budget: opt<str>; body: node `QUERY` | `define` |
| 0x91 | `PDECL` | name: str; type: node `TYPE`; optional: bool8; default: opt<expr> | `pdecl` |
| 0x92 | `TYPE` | name: str; arg: opt<str> | `ptype` |

Every other tag value is not an encoding. A target (`MSET`, `MDELETE`, `MMOVE`, `MEDGE`, `MREOPEN`, `MPATCH`,
`SCREATE`, `SRESOLVE`) is a `VAR` or a `NODE`.

### 6.4 Enumerations

Every enumeration is a `u8` ([F01 §5.4]); a value not listed is not an encoding. The name is the atom §4.3 prints. The
enumeration tables of [F01 §2.6] are merged into one table with a leading `enumeration` column, again for compactness
([F01 §2.4] rule 5).

| enumeration | value | name | meaning |
|---|---|---|---|
| `QUERY.rest[].op` | 1 | `union` | `UNION` |
| | 2 | `union_all` | `UNION ALL` |
| | 3 | `except` | `EXCEPT` |
| | 4 | `intersect` | `INTERSECT` |
| `SCALL.ymode` | 0 | `none` | no `YIELD` |
| | 1 | `star` | `YIELD *` |
| | 2 | `items` | `YIELD` with items |
| `EDGEP.dir` | 0 | `typed` | the pattern has types; each carries its direction |
| | 1 | `right` | an any-kind pattern `-->` |
| | 2 | `left` | an any-kind pattern `<--` |
| | 3 | `both` | an any-kind pattern `--` |
| `EDGEP.types[].dir` | 1 | `right` | from the pattern's left node to its right node, in the stored direction |
| | 2 | `left` | from the pattern's right node to its left node |
| | 3 | `both` | either direction (undirected, or a symmetric kind) |
| `CMP.op` | 1–6 | `=` `<>` `<` `<=` `>` `>=` | in this order |
| `STRPRED.op` | 1 | `starts` | `STARTS WITH` |
| | 2 | `ends` | `ENDS WITH` |
| | 3 | `contains` | `CONTAINS` |
| `ARITH.op` | 1–4 | `+` `-` `*` `/` | in this order |
| `LISTPRED.kind` | 1 | `all` | `all(…)` |
| | 2 | `any` | `any(…)` |
| | 3 | `none` | `none(…)` |
| `RSUF.kind` | 1 | `tilde` | `~n` |
| | 2 | `caret` | `^n` |
| | 3 | `at` | `@n` (reflog position) |
| | 4 | `attime` | `@<datetime>` (reflog time) |
| `RRANGE.op` | 1 | `two` | `..` |
| | 2 | `three` | `...` |
| `SCREATE.edges[].dir` | 1 | `out` | the created node is the source |
| | 2 | `in` | the created node is the destination |
| `SRESOLVE.take` | 1 | `ours` | `TAKE OURS` |
| | 2 | `theirs` | `TAKE THEIRS` |
| | 3 | `base` | `TAKE BASE` |
| | 4 | `value` | `TAKE VALUE <expr>` |
| | 5 | `repoint` | `TAKE REPOINT <target>` |
| `MDELETE.policy` | 0 | `none` | no `POLICY` |
| | 1 | `restrict` | `POLICY RESTRICT` |
| | 2 | `cascade` | `POLICY CASCADE` |
| | 3 | `reparent` | `POLICY REPARENT` |
| `MMOVE.pos` | 0 | `none` | no position |
| | 1 | `before` | `BEFORE <target>` |
| | 2 | `after` | `AFTER <target>` |
| | 3 | `first` | `FIRST` |
| | 4 | `last` | `LAST` |

### 6.5 Worked example

The C-AST of §4.4 encodes to these 174 bytes (hex, 32 per line):

```
10 00 00 00 6d 6f 69 72 61 69 2d 6c 71 2d 61 73 74 2d 76 31 01 00 01 02 00 03 01 00 00 00 10 00
01 00 00 00 20 21 01 00 00 00 00 01 00 00 00 04 00 00 00 74 61 73 6b 00 00 00 00 00 00 00 00 00
01 31 33 01 3a 3b 00 00 00 00 06 00 00 00 73 74 61 74 75 73 58 04 00 00 00 6f 70 65 6e 33 04 3a
3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 52 01 00 00 00 00 00 00 00 14 00 00 01 00 00
00 16 3b 00 00 00 00 00 01 00 00 00 18 3a 3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 00
01 52 05 00 00 00 00 00 00 00 00 00 00 00
```

Read from the start: the header (22 bytes); `01` `QUERY`; `02` `PART`, `00` no `USE`; `03` `CLAUSES` with one clause;
`10` `MATCH`, not optional, one `PATH`; `21` `NODEP` with variable 0, the label `task`, no properties, no `WHERE`; no
steps; the `WHERE`: `31` `AND` of `33 01` (`=`) `PROP(VAR 0, "status")` `ENUM "open"` and `33 04` (`<=`)
`PROP(VAR 0, "priority")` `INT 1`; `14` `RETURN`, neither `DISTINCT` nor `*`, one `RITEM(VAR 0)` without alias, one
`SORT` ascending on `PROP(VAR 0, "priority")`, `LIMIT` `INT 5`; and the empty `rest` list of `QUERY`. The query hash of
both texts of §4.4 is BLAKE3-128 of these bytes; WP-22 records it in the fixture (§7.1).

## 7. Hashes derived from the encoding

### 7.1 The query hash

`H(x)` = BLAKE3-128 of the encoding of x ([F01 §7.1]): the first 16 bytes of the BLAKE3 output over the encoding in
BLAKE3's default, unkeyed mode, so `H(x)` = BLAKE3-256(encoding)[0..16], which is what a fixture assembled with
`xtask hex`'s `{blake3_256 a..b}` directive gives ([PLAN §3.2] WP-20). `H` is a `b16` ([F01 §5.6]). The encoding's
leading `lp("moirai-lq-ast-v1")` separates this hash from every other BLAKE3 derivation of the format.

### 7.2 Values

| Value | Definition |
|---|---|
| `idem_payload` | `H` of the `TX` root (R3–R5) ([AR §4.3]: "BLAKE3-128 … of the payload"; [AR §6.4]) |
| `stmt_hash` (F10) | the same `H` for a commit whose `stmt_origin` is verb, named mutation, `tx` or MCP write; for the origins merge, import and file verb there is no LQ statement and the field is absent (the presence bit of [F06], Open point C-9) |
| default idempotency key | [AR §6.4]'s BLAKE3 over the namespaced session, the attested thread or agent, and the canonical bound AST, where the last input is `H` (16 bytes); the framing of the three inputs is [F06]'s (Open point C-10) |
| cursor query hash | `H[0..8]` as a u64, little-endian, of the `QUERY` root ([50 §3.5]) |
| EXPLAIN id | the ASCII text `q:` followed by the lower-case hex of `H[0..4]` (8 digits) |
| F3 canonical-AST hash | `H` of the `DEFINE` root over the portable form (R6), bound against the schema of the commit that stores the item; stored unhashed in the `QUERIES` item and recomputed on import and by `doctor --verify` ([50 §4.4]) |
| F3 hash in a merge | when both sides changed a definition, each side's `DEFINE` is bound against the **merge result's schema** — the schema the post-merge validator binds every touched definition against ([50 §4.4]) — and the two `H` values are compared; equal is not a conflict and dst's text lands, different is a `FieldEdit`, a side that does not bind is `QueryInvalid` ([50 §4.4] as amended, S-10; the merge row is [RULES/merge-table]'s; Open point C-17) |

The `IF TARGETS` target-set digest is not a query hash; Open point C-11 proposes its encoding.

## 8. Portable form of definitions (F3)

### 8.1 The definition-time rewrite

When a `TX` defines a named query, the binder computes the stored text from the author's text ([50 §4.4] Portable form,
as amended after the A1 re-review, and Storage):

1. remove a leading byte-order mark;
2. replace each CR LF by LF, and each remaining CR by LF;
3. remove every SP and HT that stands before an LF or at the end;
4. replace, each over the exact byte span of its token, wherever it stands (a `param_decl` default included):
   - every node literal `#N` by `#u:<32 hex>` of that node;
   - every sequence literal, and every commit literal of fewer than 64 hex digits, in revision mode by `c<64 hex>`;
   - every literal that the binder coerced to a node (an integer compared with a node or with `id`, `{id: 40}`,
     `t IN [40, 41]`, `id(t) = 40`, an integer default of a `node` parameter) by `#u:<32 hex>`;
   - every literal outside revision mode that the binder coerced to a revision whose base is a sequence number or a
     commit prefix (an integer, `t.rev = 4466`, `t.updated > 4400`; a bare word or string of revision shape; a default
     of a `rev` parameter) by the string `'c<64 hex>'` followed by the literal's own suffixes inside the same quotes
     (`t.rev = 4466` → `t.rev = 'c…'`; Open point C-4);
5. refuse every reflog revision (`REF@n`, `REF@<datetime>`) with E117, and every anchor handle (a string compared with or
   bound to an `AT` edge's `anchor` field, `a.anchor = 'a17'`: `aN` comes from `HEAD.next_anchor` and is store-local,
   [40] R-6) with E117 — neither has a portable equivalent (the A1 review's S-02; [LQ/errors] carries the anchor case).

Comments are kept and nothing else changes. The result is the text blob of the `QUERIES` item and the body of
`schema/queries/<q>.moi`. Its C-AST equals the C-AST of the author's text bound in the defining store, because the
C-AST already carries uids and full commit ids (§5.5, §5.6); the F3 hash is therefore the same in every store that
imports the definition ([50 §4.4], [50 §8.3] two-store property).

### 8.2 Checks and display

The exporter and importer apply the portable-text check of [LQ/lexical §10.2]. Its authoritative condition is a binding
check: the definition, bound against the schema of the ref that carries it, must hold no node-typed constant other than
a `#u:` literal, no revision-typed constant whose base is a sequence number or a commit prefix, no reflog revision and
no anchor handle — exactly the constants step 4 and step 5 of §8.1 remove or refuse. Displays (`--show-query`,
`CALL queries()`, errors) render `#u:…` back as the local `#N` when the store knows the uid, and as `#u:…` otherwise;
binding a definition whose uid the store does not know is E111 ([50 §4.4]).

## 9. Fixture binding context

A C-AST fixture (WP-22) states the context its C-AST was bound in, one item per line, before the expected tree:

```
schema core                                       the core schema of [F08], with its F1 and F2 rows
node 88 018f3c2e7a117b3c9d5e4c2f1a0b9e88          a #N and its uid
commit 4466 <64 lower-case hex>                   a store sequence number and its commit id
param scope node 88                               a bound parameter: name, type, value in k=v text
```

A fixture that asserts the hash carries the encoding in hex and the hash as `{blake3_256 …}` of it, first 16 bytes
(§7.1).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [50] F3 (and its [60 §2.5] R5-reservations row) | the canonical-AST hash: the algorithm (C-AST rules and encoding), `H` over the `DEFINE` root in portable form, its recomputation on import, its use in the merge; the portable text of the item (the definition-time rewrite) and its binding check. The item layout is [F08]'s, the query file's name and ABNF [F14]'s, the merge row [RULES/merge-table]'s | §5, §6, §7, §8 |
| [50] F10 (and its [60 §2.5] row) | the value of `stmt_hash`: `H` of the `TX` root; absent for the merge, import and file-verb origins. The header layout and presence bit are [F06]'s; `stmt_sym`'s symbol class is [F01 §8.2]'s | §7.2, C-9 |
| [60 §2.5] "Commit body" row ([AR §4.3] idempotency pair) | the value of `idem_payload`: `H` of the `TX` root. The field layout is [F06]'s; the idempotency record is [F11]'s | §7.2 |
| [50] F1 | how the C-AST uses `lq_name`, the forward names, the stored name, `reverse_names` and `symmetric`. The schema row is [F08]'s | §5.4, C-8 |
| [50] F2 | how the C-AST uses `coerce` (priority, revision-integer). The schema row is [F08]'s | §5.5 |
| [90 §10.1] "LQ card" row (and [60 §2.5] "Harness-agnostic interface": the card's display spelling) | that the canonical form, its encoding and every hash are independent of the display spelling. The spelling and its ablation are [LQ/gql-spelling §4]'s and [LQ/card]'s | §1.3 |

No other [60 §2.5] row, R-1…R-18 item, F-item, X-F item or [90 §10.1] item is specified here.

## Holes

None. No byte, tag, code or rule of this chapter waits on an M0 measurement. The chapter freezes with the query surface
after WP-72; a WP-73 remedy that changes the grammar or the semantics updates §3 and §5 before the tag
([LQ/grammar-v1.ebnf] O-11). The display spelling (the L1 ablation, `HOLE(LQ-display-spelling)` of [LQ/gql-spelling])
does not touch this chapter (§1.3).

## Open points for the review

| # | Point | Resolution here |
|---|---|---|
| C-1 | **PLAN §3.3 gap "Canonical-AST encoding" (WP-19).** [50 §5.3] lists what the canonical form normalises and that BLAKE3-128 of "its encoding" is the query hash, but defines no encoding. | Resolved: a tagged pre-order binary encoding with fixed-width little-endian integers, `lp()` strings, explicit list counts and presence bytes, under an `lp("moirai-lq-ast-v1")` + `lq` header (§6); every tag, field and code is fixed; a text form mirrors it for fixtures (§4). Fixed widths rather than varints keep two independent encoders (the model's LQ-3 and the product's LQ-2) trivially identical. |
| C-2 | Which `TX` options belong to the idempotency payload. [50 §3.10] item 8 lists what is normalised away but not the options. | `ON`, `KEY`, `LEASE` and `DRY` are dropped: the branch and the key are bound separately by the idempotency record ([AR §6.4]); the lease is authority, checked by fencing, and a retry after a re-claim must still replay; a `DRY` and its apply then share one hash. `IF TIP`, `IF TARGETS` and `MESSAGE` stay: they are part of the request, and the message is part of the commit. The CLI flags and MCP fields join the block first (R3), so the flag and clause forms hash alike. |
| C-3 | Column names. [50 §5.3] renames variables, but an unaliased `RETURN t` names its column after the variable. | `RETURN` aliases are kept as strings (they are output names, like property names, and an alias change is a real change of output); the display name of an unaliased column is not hashed. Consequence: two definitions that differ only in the variable behind an unaliased column have one hash, so a merge that changed both sides that way does not conflict, and the landing side's column name wins. |
| C-4 | [50 §4.4] rewrites `#N`, `s<seq>` and commit prefixes, but a literal the binder coerces (`{id: 40}`, `t.rev = 4466`, `t.rev = s4466`) is just as store-local. | The rewrite covers coerced node and revision literals too (§8.1 step 4), and the binding check of §8.2 verifies it on export and import (C-18). |
| C-5 | "Parameter order normalised away" ([50 §3.10] item 8). The CLI verb passes `complete 89` positionally while MCP passes `id=89` by name. | Calls to relations, named queries and named mutations have their arguments named and ordered by the callee's signature (N2), so all doors hash alike; defaults are not filled in, so a later default change does not change a stored hash. Depends on [LQ/std] naming every parameter, including the first positional one of each relation (`blockers(n, …)`, `search(terms, …)`). |
| C-6 | Name resolution for `CALL`. [50 §4.4] says "std first, then the project" for named queries; relations share names with std named queries (`blockers`, `diff`, `history`). | Relations first, then `std.<name>`, then project names; `std.x` explicitly names the std query. A std named query whose body calls the relation of the same name (`std.blockers`) would otherwise call itself. [LQ/std] and the binder confirm. |
| C-7 | Symmetric kinds. | Canonicalised to `both` in patterns only; never in created edges, whose stored direction is part of the edge key. |
| C-8 | [50 §2.5] lists `SUBTASK_OF` as a synonym of `CHILD_OF`, but F1 has `lq_name` and `reverse_names` only. | Resolved here as a forward synonym (no direction flip). **Closed at pass 1 (A1-56):** §5.4's synonym table owns it; [F08] adds no forward-name list. |
| C-9 | F10's `stmt_hash` for commits not produced by an LQ statement. | Absent (presence bit clear) for `stmt_origin` merge, import and file verb. WP-12 ([F06]) owns the presence bitmap and confirms. |
| C-10 | [AR §6.4]'s default key hashes "the canonical bound AST" with other inputs. | This chapter supplies `H`; the framing (the `lp()` of each input and their order) is [F06]/[API]'s. |
| C-11 | **Unassigned gap: the `IF TARGETS` target-set digest** ([50 §3.10] items 4, 9; [72 m1]). [PLAN §3.3] assigns it to no WP, yet the model (WP-93b) must compute it and fixtures must print it. | Proposal for WP-19's envelope part or WP-80a to adopt: digest = BLAKE3-128 of `lp("moirai-lq-targets-v1")` ‖ `lq` u16 ‖ for each `MATCH … EXPECT` statement in order: its 1-based index u32 ‖ its binding count u32 ‖ the bindings sorted bytewise, each the concatenation, in variable-index order, of the values of the variables its mutations use (a node as its 16-byte uid; an edge as source uid, `lp(lq_name)`, destination uid and the 16-byte discriminator or zeros); printed and written as 32 lower-case hex digits in `IF TARGETS '<hex>'`. |
| C-12 | Two normalisations not listed in [50 §5.3]: `(#N)` = `({id: #N})` (§3.1 item 7) and a subquery that is one `MATCH` = its pattern form (N1). | Adopted: each maps two spellings of one meaning to one tree, which [50 §5.3] asks for ("Cypher and GQL spellings mapped to one form"). |
| C-13 | Which revisions the C-AST resolves. | Sequence numbers and commit prefixes become full ids (they are store-local); refs, `HEAD` and suffixes stay symbolic. So `USE main~5` hashes the same whatever `main` points to, and the pinned cursor records the view's commit separately ([50 §3.5]). |
| C-14 | §5.7's scope rules are the binder's name resolution; [50] states them only in part (Cypher's `WITH` and `ORDER BY` visibility is implied by "as in Cypher"). | Stated here in full because the renaming depends on them; LQ-3 (WP-93a) and LQ-2 (M7) implement the same table. The review should check V4 and V5 against Cypher's visibility rules. |
| C-15 | The S-AST keeps float and duration text. | Needed for the printer property; the numeric value enters only the C-AST. The JSON IR reader must therefore keep a number's text ([LQ/json-ir] J-4). |
| C-16 | A `CREATE (x:kind …)` with `UNLESS EXISTS` names its bind candidate by variable name ([50 §3.10] `CREATE` row). | V8 gives the subquery's same-name binding the created variable's index, so renaming both keeps the link. |
| C-17 | **The A1 review's S-10**: the canonical AST is the bound AST, binding reads F1 aliases and field types from schema data that can differ per branch, so the F3 hash of one text can differ by branch and a merge decision could depend on which schema binds. | As amended [50 §4.4] now states (S-10 fixed in `docs/spec/reviews/a1-dispositions.md`): the merge compares hashes computed against one schema, the merge result's (§7.2, "F3 hash in a merge"), and with equal hashes dst's text lands. The stored hash stays a cache for the common case of equal schemas. S-10's alternative — a frozen alias table per grammar version — was not taken, because project edge kinds bring their own F1 aliases and field types, which no frozen table can list. |
| C-18 | **The A1 review's S-02** (major): the portable form missed store-local constants spelled without `#` or `s`. | Fixed in amended [50 §4.4] (`docs/spec/reviews/a1-dispositions.md`, FS-2); this chapter carries the bytes: the rewrite works on the bound AST by type (§8.1 step 4: node-typed and revision-typed constants, whatever their spelling, parameter defaults included, `'c<64 hex>'` outside revision positions); anchor handles in a definition are E117 (step 5); the importer and exporter check by re-binding, never by a character pattern (§8.2, [LQ/lexical §10.2]). |
| C-19 | **Conflict with [LQ/gql-spelling §2.1]**, which says "`{1,1}` canonicalises to a plain hop". | Not adopted: the C-AST keeps `QUANT(1, 1)`. A quantified part binds endpoint pairs and a fixed hop binds one row per edge ([50 §3.4] items 1 and 4), N08 fires only on the former, and an edge variable is allowed only on the latter; with `[:A\|B]{1,1}` or parallel edges of two kinds the two count differently. The spelling chapter should drop that sentence. |
| C-20 | [F01]'s layout conventions. | §6.1 is an offset table with a `total` row; §6.3 and §6.4 merge the per-structure sequence and enumeration tables into one table each, a departure stated in place as [F01 §2.4] rule 5 requires. The encoding uses `lp()` for strings because it is a hash input that is never stored ([F01 §6.3]); its framing is argued unambiguous in §6.1 as [F01 §7.3] asks. |
| C-21 | An unaliased `WITH` item that is not a bare variable (`WITH t.x WHERE …`) has no name to bind; Cypher refuses it ("expression in WITH must be aliased"), and [50] is silent. | A bind error, so the C-AST never meets it (V4). [LQ/errors] has no row for it yet; proposed: E001 with the rewrite `WITH <expr> AS <name>`. |
