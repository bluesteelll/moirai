# fixtures/moi: the `.moi` format version 1

| | |
|---|---|
| Title | The golden files of [F14]'s `.moi` format v1 with their expected parse: node files of every kind, every value form, blocks, ledgers, edges, anchors, conflicts, bodies, tombstones, R4 file and root nodes, schema tables, named-query files, the marker and the side-ref files, whole trees, the superset an importer accepts, and one `ImageParse` negative per rule of [F14 §9.2] |
| Work package | WP-21, second part: `moi/` (R-FIX; [PLAN §3.2] item 1, the catalogue of [F14 §16]). `carrier/` is the other half of this part |
| Acceptance | E3: WP-95's ABNF conformance check parses every positive file here and refuses every negative; the expected parse is for the M5 codec and any harness that maps a file to canonical keys ([F14 §11.1]) |
| Separation | S1 ([PLAN §3.1]): written from the specification text only. The author read no line of `moirai-format-oracle`, `moirai-model`, `moirai-toylog` or any product crate, and ran no project code. The files were produced by a throw-away encoder of [F14] and checked by a throw-away validating decoder written from the same chapter: every positive file decodes to its expected keys and re-encodes to its own bytes, every superset file decodes to the keys of its canonical twin, every negative is refused for the rule it is named for. The same encoder reproduces [F14 §17.2]'s computed uids, digests, `captured` values and anchor lines byte for byte |
| Sources | [F14] (normative for every byte here); [F07] (keys and values), [F08] (data model, core schema, anchor record, derivations), [F18 §2.8], §2.9, §5 (I-F8, I-F9, R-17), [OS/path §2] (path grammars), [LQ/lexical §10.2], [LQ/canonical-ast §8] (portable query text), [LQ/std §2.3]–§2.4 (shape and budget words) |
| Status | Written against the specification after review pass 1 (owner answers of 2026-09-28); updated for spec sync 2b (`docs/spec/reviews/spec-sync-2b.md`): `schema/policy.moi` and its negative (S2B-R-28), `query/no-shape.moi` (S2B-R-18), and §6's findings settled. A specification change that moves a byte updates the files it touches (commit subject `WP-21:` or `WP-73:`) |

Every value is synthetic: no owner data, no real paths, users or hosts. `.gitattributes` gives `fixtures/** -text`: every byte is kept as written, the CR LF, lone CR, BOM, NUL and FF inputs included.

## 1. Layout

| Path | What it holds |
|---|---|
| `node/<name>.moi` | one node file; parsed under the core schema of version 1 ([F08 §9]). File names are descriptive: the rule that a node file's uid equals its file name is a tree rule ([F14 §3.4]) |
| `schema/<table>.moi` | the four schema tables, as `schema/kinds.moi`, `schema/fields.moi`, `schema/edges.moi`, `schema/policy.moi` of a tree |
| `query/<name>.moi` | named-query files; in a tree each is `schema/queries/<q>.moi` (its expect names q) |
| `side/…` | the marker (`side/marker-<format>/.moirai-image`), `refs/heads.moi` and `refs/tags.moi` rows, and the side-ref files `meta.moi`, `aliases/<h1>.moi`, `ops.moi` ([F14 §4], §8, §14) |
| `tree/<name>/` | whole image trees, read as §11.1 reads a commit's tree |
| `superset/<name>.moi` | inputs an importer accepts and normalises ([F14 §9.1]); each names its canonical twin |
| `neg/<rule>--<variant>.moi` | single-file `ImageParse` negatives (node, query and schema files) |
| `neg-tree/<rule>--<variant>/` | tree negatives |
| `neg-commit/<rule>--<variant>.commit` | git commit objects whose trailer paragraph stages `ImageParse` |
| `<fixture>.expect` | beside every fixture: its expected parse or its refusal (§2) |

## 2. The `.expect` files

An `.expect` file uses the framing of `fixtures/canonical/INDEX.md` §2.1 (UTF-8, LF, `%%` directives; `#` lines are comments; a block runs to the next line that starts with `%% `) and holds one record ending in `%% end`.

| Directive | Kind | Meaning |
|---|---|---|
| `fixture` | line | the fixture's path under `fixtures/moi/` (a tree ends in `/`) |
| `file` | line | `node`, `schema`, `query`, `marker`, `refs`, `meta`, `aliases`, `ops`, `tree` or `commit` |
| `context` | line | what the parse needs besides the file: `core` (the core schema), `destination object format <f>` (the marker's and the tree's check), `carrier destination A` (a commit negative) |
| `source`, `note` | line, repeatable | the sections the fixture rests on; commentary |
| `canonical` | line | positives: `self` when an exporter writes exactly these bytes for the parse, otherwise the fixture whose bytes it writes (superset inputs), or `none` when the parse holds no key an exporter would write |
| `keys` | block | the parse as canonical keys, in the state notation of `fixtures/canonical/INDEX.md` §3 with the extensions of §3 below; the keys of one node for a node file, of the whole tree for a tree |
| `image` | block | the image-only data the importer keeps outside the keys ([F14 §11.3]): `created <value>`, `updated <value>`, `deleted <value>` (commit id and time text as written), `ledger <field> <±k> <token>` per ledger line, and flags: `flag text-unavailable <anchor uid>` for an anchor imported without its texts, `flag foreign-uid` or `flag foreign-anchor-uid <uid>` for a uid that does not match its derivation ([F14 §9.3]). In a tree each line starts with the node's uid |
| `items` | block | schema tables: the items in the notation of `fixtures/canonical/INDEX.md` §3.6 |
| `query` | block | query files: the item as a `schema query` line, or its conflict value as in §3 below |
| `fields` | block | the marker: its three values |
| `refuse` | line | negatives: `ImageParse <rule>` ([F19] code 75); `<rule>` is an id of §4.2 |

## 3. Notation extensions

- **Anchor texts that are not UTF-8** ([F14 §5.7]) are written `quote=%<base64url>` (likewise `prefix`, `suffix`, `end`) in an `at` line, as the image writes them.
- **A conflicted named query** ([F14 §7.2.4]) is written

  ```
  schema query <name> conflict <class>
    base lq=… params=… shape=… budget=… text=…     or  base absent
    ours …
    theirs …
  ```

- The keys block lists set elements in their canonical order ([F07 §2.4]), edges by (kind, dst), anchors by (dst, anchor uid) and conflicts by key text; the order carries no meaning.
- An existence conflict's `prov` ([F07 §7.3]) is the side whose existence matches the file's form: a tombstone file gives the `deleted` side, a live file the `live` side ([F14 §6.8.1]; §6 finding M-3).
- **A policy row** ([F08 §8.5.6], [F07 §9.7]; spec sync 2b) is the item line `schema policy <name> value=<json>`: the row's name and the JSON string of its canonical value.

## 4. Contents

### 4.1 Positive files by the groups of [F14 §16]

| Group | Fixture | What it asserts |
|---|---|---|
| node kinds | `node/area.moi` | Kind area (not a root node: no root field): a side status, a parent and globs. |
| node kinds | `node/artifact.moi` | Kind artifact, a file node with every field but origin_pred (its derivation had no predecessor): no title line; implied-root paths; an alias that needs JSON inside the set (a comma). |
| node kinds | `node/decision.moi` | Kind decision: alternatives is a record list of two records (a block); a relates edge to a larger uid. |
| node kinds | `node/doc.moi` | Kind doc: a one-record list holding HT is a JSON string (targets); a several-record list is a block whose lines keep their HT bytes (readiness, whose first record ends in an empty member). |
| node kinds | `node/finding.moi` | Kind finding: a resolution on a non-open finding and confidence confirmed (findings only). |
| node kinds | `node/lane.moi` | Kind lane: an abs path in the Unix form and one in the drive form; oids of both algorithms. |
| node kinds | `node/measurement.moi` | Kind measurement at its initial status: `status: current` is written and reads as absent; f64 values 28.6 and 40.0; a sha256 oid; a ref. |
| node kinds | `node/note.moi` | Kind note: a side status (superseded), the tagged applies_to set, an sha1 oid, a counter. |
| node kinds | `node/question.moi` | Kind question: options is a three-record list of one member each (a block). |
| node kinds | `node/rule.moi` | Kind rule with authority owner and its owner_quote; the symmetric contradicts edge is written here because this uid is the smaller endpoint. |
| node kinds | `node/run.moi` | Kind run: explicit-root abs paths in the Windows drive and UNC forms; a set whose elements need JSON (a comma, brackets, a leading SP, a leading quote, an HT) next to bare ones (`{brace}` is bare inside a set). |
| node kinds | `node/task.moi` | Kind task with every header line it can carry (title, status, resolution, the four header enumerations off their defaults, parent, order, created, updated, flags), every task field, labels, two ledger lines, edges (a pinned cites, an unpinned implements, blocks, discovered_from, mentions) and a body. The acceptance text holds LF, so it is a block. |
| node kinds | `node/verdict.moi` | Kind verdict (fields immutable after Create): outcome, return_to, gates and verifies edges. |
| node kinds; ledgers | `tree/project-schema/` | A whole tree with project schema items: a node of the project kind experiment with every header line it can carry (priority P5 is a project value of the core enumeration), project fields of types text (a block), int with a range, path with an explicit project root, commitref, set of int (written sorted as text: [11, 3, 42]), enum, bool and sym, a counter; a project edge kind and a symmetric one; project fields on the core kind task (risk, retries); ledgers of two counters with several lines, a negative delta and tokens that are not commit ids; two named-query files named by their hashes. |
| values | `node/f64-shortest.moi` | Values: f64 0.30000000000000004 and 123456789012345680000.0 written by Number::toString with .0 appended. |
| values | `node/f64-small-negative.moi` | Values: f64 1.5e-7 and -3.0 written by Number::toString with .0 appended. |
| values | `node/f64-zero-large.moi` | Values: f64 0.0 and 1e+21 written by Number::toString with .0 appended. |
| values | `node/text-json-forms.moi` | Values: a bare text with an SP inside; texts that begin with SP, `"`, `[` and `{` are JSON strings. |
| values | `node/text-lengths-controls.moi` | Values: `<<` as a JSON string; a 4,096-byte text bare and a 4,097-byte text as a JSON string; control characters (U+0007 escaped as \u0007, U+007F and U+0085 written raw inside the JSON string); non-ASCII text bare; a trailing SP. |
| blocks | `node/blocks.moi` | Blocks: a two-line text; a text ending in LF (last line two SP only); an empty inner line; an inner line `>>` (written `  >>`, never the end); trailing SP and HT inside a block kept; leading LFs. |
| edges | `node/edges.moi` | Edges: pinned and unpinned historical edges, mentions (read from lines only), a symmetric relates from the smaller uid; sorted by (kind, dst, rest of the line). |
| anchors | `node/anchors-full.moi` | Anchors in full mode: every kind (file, heading, symbol with a Rust and with a TOML scope, quote, range in pinned mode, lines with a one-line hint and watch header); scopes of the three languages, a Markdown name escaping `/`, `[`, `]` and `%`, a qualifier; occurrence, marker, pred; an empty prefix and suffix (`""`); a Latin-1 quote written as `%` and base64url. |
| anchors | `node/anchors-hash-only.moi` | The same anchors written in hash-only mode: digests only; every anchor with a quote is imported text-unavailable (the image block flags it). Same keys as anchors-full. |
| anchors | `node/anchors-mixed.moi` | A full-mode file whose store held some anchors without their texts: those lines carry digests only and are imported text-unavailable; the others carry their texts. Same keys again. |
| conflicts | `node/conflict-existence-live.moi` | DeleteVsModify on a note (existence policy resurrect): the file is live, so the provisional side is ours. The ours snapshot carries a status, header, flag and field keys sorted by field name (title among them), labels, a counter total and a body; the base snapshot writes the initial status. |
| conflicts | `node/conflict-existence-tombstone.moi` | DeleteVsModify on a task (delete-wins): a tombstone file carrying the conflict line; the live side carries status, fields, labels, a counter total and a body. |
| conflicts | `node/conflict-observation.moi` | An observation PathClaim on a file node created by the merge: base and ours absent (empty sides), theirs the composite; the six member field lines are omitted. |
| conflicts | `node/conflicts.moi` | Conflicts on a live task: a scalar FieldEdit with an absent side (theirs removed the estimate), a StatusFork with resolutions, a body TextHunk (bodies with LF and `"`: no `---` section), a parent conflict (`-,a2`: no parent), an edge conflict (ours removed the pinned edge, theirs re-pinned it), an edge.at conflict (anchor sides as JSON strings of the line's properties), a header conflict (field.priority: no `priority:` line) and a set conflict (field.labels: no `label` line; a set side is the JSON string of its `[...]` form). Lines are sorted bytewise by key. |
| bodies | `node/body-inner-dashes.moi` | Bodies: an inner `---` line is body text. |
| bodies | `node/body-no-final-lf.moi` | Bodies: a body without a final LF: the file ends in its LF. |
| bodies | `node/body-none.moi` | Bodies: no body: no `---` line. |
| bodies | `node/body-nul.moi` | Bodies: a body may hold U+0000 (it is the only place). |
| bodies | `node/body-one-lf.moi` | Bodies: a body with one final LF: the file ends in two LF. |
| bodies | `node/body-three-lf.moi` | Bodies: three final LFs survive. |
| bodies | `node/body-trailing-spaces.moi` | Bodies: trailing double SP kept. |
| bodies | `node/body-unicode.moi` | Bodies: non-ASCII and C1 bytes kept. |
| tombstones | `node/tomb-artifact.moi` | An artifact tombstone: its title line is its last path text. |
| tombstones | `node/tomb-edges-anchors.moi` | A tombstone with a flagged blocks edge, historical cites (pinned) and mentions edges, and a retained anchor line. |
| tombstones | `node/tomb-no-reason.moi` | A tombstone without reason and replacement: `deleted(note, "", none)`. |
| tombstones | `node/tomb-replaced.moi` | A tombstone with a reason and replaced_by (both carry the existence value). |
| R4 | `node/artifact-every-field.moi` | Row R4: a file node with every field, origin_pred included; its uid is uid_file(project, docs/storage/lock.md, origin_pred), which the importer recomputes (§9.3). |
| R4 | `node/artifact-planned.moi` | Row R4: a planned file node: no oid, bytes or observation yet. |
| R4 | `node/artifact-removed.moi` | Row R4: a removed file node (status removed, a reason); still a live node file. |
| R4 | `node/root-node.moi` | Row R4 and blocks: the root node of project; a path_moves block of three entries sorted by (hlc, from, to, class, git): two with one hlc, one with an empty git and one whose to holds a comma (JSON arrays need no other escape). |
| schema | `schema/edges.moi` | The edges table: readings hold SP, so they are JSON strings, one holding `"`; kind sets `*` and names; flags symmetric and same_kind; reverse names. |
| schema | `schema/fields.moi` | The fields table: field rows with a default and a range (min, max), a sym default that needs JSON (an SP), flags, a retired row; value rows with covers and flags, and a value of the core enumeration priority under kind `*` (sorted first). |
| schema | `schema/kinds.moi` | The kinds table: a project kind with flags and a retired kind (it stays in the file). |
| schema | `schema/policy.moi` | The policy table (spec sync 2b, S2B-R-28): a row of a parameterised name (`policy.role.developer.mcp-write yes`, off its default `no`) and a row with a list value (`policy.role.developer.fields files_owned,title`, off its default `files_owned`), in name order. |
| queries | `query/conflict-absent-side.moi` | A conflicted definition with an absent side. |
| queries | `query/no-params.moi` | A query without parameters (no `params:` line) and without BUDGET (`budget: medium`). |
| queries | `query/no-shape.moi` | A query without SHAPE and without BUDGET: `shape: table` and `budget: medium`, the stored words ([F14 §7.2.2]; spec sync 2b, S2B-R-18). |
| queries | `query/params-ht.moi` | A string default holding a raw HT: the `params:` line value is a JSON string. |
| queries | `query/with-params.moi` | A query with parameters: `params:` is the rendered param_decl list. |
| side refs and rows | `side/aliases/1a.moi` | aliases/1a.moi: rows of uids that begin with byte 1a, by uid. |
| side refs and rows | `side/marker-sha1/.moirai-image` | The .moirai-image marker, sha1. |
| side refs and rows | `side/marker-sha256/.moirai-image` | The .moirai-image marker, sha256. |
| side refs and rows | `side/meta.moi` | meta.moi of the side ref: store id, format, mode, cursor, ref rows sorted. |
| side refs and rows | `side/ops.moi` | ops.moi: ref events and client-head events (a session key that needs JSON) in log order. |
| side refs and rows | `side/refs/heads.moi` | refs/heads.moi of a checkpoint of main. |
| side refs and rows | `side/refs/tags.moi` | refs/tags.moi of a checkpoint of tags/v1. |
| side refs and rows | `tree/sha256/` | A tree of a sha256 destination. |
| superset inputs | `superset/bom.moi` | Item 1: one leading byte-order mark is skipped. |
| superset inputs | `superset/crlf.moi` | Item 2: CR LF everywhere, in the block and the body too. |
| superset inputs | `superset/defaults-explicit.moi` | Item 7: written defaults (the initial status, priority P2, criticality normal, confidence unset, authority agent), an empty text, -0.0, a positional f64 for 1e+21 and an empty body read as their canonical values. |
| superset inputs | `superset/empty-values.moi` | Empty values read as absent. |
| superset inputs | `superset/escapes-solidus.moi` | Item 6: `\/` inside a JSON set element decodes to `/`. |
| superset inputs | `superset/escapes-surrogates.moi` | Item 6: a surrogate pair, and \u escapes of U+007F, U+0085 and `<`. |
| superset inputs | `superset/escapes.moi` | Item 6: escapes an exporter never writes: upper-case \u digits, \u escapes of printable characters, a bare-able text as a JSON string. |
| superset inputs | `superset/lone-cr.moi` | Item 2: lone CR line ends. |
| superset inputs | `superset/missing-final-lf.moi` | Item 4: a file without its final LF. |
| superset inputs | `superset/no-provenance.moi` | Item 8: a live file without created: and updated: (a file written by hand); the store fills them. |
| superset inputs | `superset/reordered.moi` | Item 5: lines in reverse order; the magic line first, the block attached to its field line, the body last. |
| superset inputs | `superset/set-order.moi` | A set in another order with a bare-able element as a JSON string ([F14 §5.3]: the importer re-sorts). |
| superset inputs | `superset/trailing-sp.moi` | Item 3: SP and HT at the end of every line outside the block and the body. |
| superset inputs | `tree/root-extra/` | Root entries outside the layout are ignored. |

The group `ledgers` is `tree/project-schema/` (two counters, several lines, a negative delta, tokens that are not commit ids) with `node/task.moi` and `node/note.moi`; the project kind of `node kinds` is the same tree's `experiment` nodes. The anchor digests, `captured` values and anchor uids are derived by [F08 §11.4] over the stated texts; the texts themselves and the `hint`, `occurrence`, `window` and `span` values are stated values of the record, not the output of a [F20 §6.1] capture over any file content, and no capture or resolve check applies to them (as in `fixtures/canonical/INDEX.md` §3.4 and §7 G-5).

### 4.2 `ImageParse` rules and negatives

| Rule | [F14 §9.2] clause | Section | Negatives |
|---|---|---|---|
| `marker` | the marker is missing or breaks §4 | [F14 §4] | `neg-tree/marker--missing/`, `neg-tree/marker--version/`, `neg-tree/marker--object-format/`, `neg-tree/marker--schema-version/` |
| `layout` | an entry under `nodes/`, `schema/` or `refs/` breaks §3.4 | [F14 §3.4] | `neg-tree/layout--h1-uppercase/`, `neg-tree/layout--prefix-mismatch/`, `neg-tree/layout--stray-file/`, `neg-tree/layout--schema-file/`, `neg-tree/layout--refs-file/` |
| `uid-file-name` | a node file's `uid:` differs from its file name | [F14 §3.4] | `neg-tree/uid-file-name--uid-mismatch/` |
| `query-file-name` | a query file's name does not hash to its file name | [F14 §7.2.1] | `neg-tree/query-file-name--hash-mismatch/` |
| `not-utf8` | a file that is not valid UTF-8 after §9.1 | [F14 §2.1] | `neg/not-utf8--title-byte-ff.moi` |
| `nul-outside-body` | a file that holds U+0000 outside a body or a block | [F14 §6.9] | `neg/nul-outside-body--field-line.moi` |
| `magic-version` | a file that begins with an unknown magic line or version | [F14 §6.1] | `neg/magic-version--node-2.moi`, `neg/magic-version--unknown-magic.moi` |
| `no-production` | a line that matches no production of its file | [F14 §6.1], §7.1 | `neg/no-production--stray-line.moi`, `neg/no-production--field-no-space.moi`, `neg/no-production--edge-arrow.moi`, `neg/no-production--schema-order.moi`, `neg/no-production--schema-row.moi` |
| `unknown-header-key` | an unknown header key | [F14 §6.2] | `neg/unknown-header-key--owner.moi` |
| `required-line-missing` | a required line missing (`uid:`, `kind:`, `title:` of a kind that requires one and of every tombstone) | [F14 §6.2], §6.10 | `neg/required-line-missing--uid.moi`, `neg/required-line-missing--kind.moi`, `neg/required-line-missing--title-live.moi`, `neg/required-line-missing--title-tombstone.moi` |
| `line-repeated` | a line repeated (a header key, a field, an (edge kind, dst), an anchor uid, a conflict key, a (field, token) ledger pair; a schema item; two `policy` rows of one name) | [F14 §6.1], §7.1, §9.2 | `neg/line-repeated--header.moi`, `neg/line-repeated--field.moi`, `neg/line-repeated--edge.moi`, `neg/line-repeated--anchor.moi`, `neg/line-repeated--conflict.moi`, `neg/line-repeated--ledger.moi`, `neg/line-repeated--schema-repeat.moi`, `neg/line-repeated--policy-repeat.moi` |
| `line-not-allowed` | a line not allowed in its form (a `title:` in a live artifact, a counter as a `field` line, a derived field, `flagged` on a live node's edge, an `edge at` line; a tombstone's status and field lines) | [F14 §6.10], §6.11 | `neg/line-not-allowed--title-live-artifact.moi`, `neg/line-not-allowed--counter-field-line.moi`, `neg/line-not-allowed--derived-field.moi`, `neg/line-not-allowed--flagged-live.moi`, `neg/line-not-allowed--edge-at.moi`, `neg/line-not-allowed--tombstone-status.moi`, `neg/line-not-allowed--tombstone-field.moi` |
| `ordinary-and-conflict` | an ordinary line and a `conflict` line for one key | [F14 §6.8] | `neg/ordinary-and-conflict--status.moi`, `neg/ordinary-and-conflict--field.moi` |
| `body-and-conflict` | a body with a body conflict | [F14 §6.8] | `neg/body-and-conflict--body.moi` |
| `unknown-name` | an unknown kind, field, enumeration value, edge kind or conflict class (`DATA` included) | [F14 §6.8] | `neg/unknown-name--kind.moi`, `neg/unknown-name--field.moi`, `neg/unknown-name--enum-value.moi`, `neg/unknown-name--edge-kind.moi`, `neg/unknown-name--conflict-class-data.moi` |
| `value-type` | a value that does not parse by its field's type | [F14 §5.1] | `neg/value-type--int-word.moi`, `neg/value-type--int-leading-zero.moi` |
| `value-constraint` | a value that breaks its field's constraints (range, one line, record-list shape, glob grammar) | [F08 §5.3], §5.4, §8.5.2 | `neg/value-constraint--range.moi`, `neg/value-constraint--one-line.moi`, `neg/value-constraint--record-list.moi`, `neg/value-constraint--glob.moi` |
| `path-rules` | a `path` that breaks I-F8 | [F18 §2.8] | `neg/path-rules--dotdot.moi`, `neg/path-rules--backslash.moi`, `neg/path-rules--abs-root.moi` |
| `relink-grammar` | a `relink` outside R-17's grammar | [F18 §5.7] | `neg/relink-grammar--lazy-magic.moi` |
| `number-range` | NaN, an infinity, an out-of-range number, a ledger whose sum leaves `i64` | [F14 §2.8], §6.5 | `neg/number-range--i64-overflow.moi`, `neg/number-range--nan.moi`, `neg/number-range--infinity.moi`, `neg/number-range--ledger-sum.moi` |
| `anchor-text-digest` | an anchor line whose texts do not match their digests | [F14 §6.7] | `neg/anchor-text-digest--text-digest.moi` |
| `anchor-texts-partial` | an anchor line whose texts are partly present | [F14 §6.7] | `neg/anchor-texts-partial--texts-partial.moi` |
| `anchor-digests` | an anchor line that lacks the digests its kind requires | [F14 §6.7] | `neg/anchor-digests--digests-missing.moi` |
| `anchor-if9` | an anchor line that breaks I-F9 | [F18 §2.9] | `neg/anchor-if9--lines-empty-window.moi` |
| `anchor-end-h` | `end_h` on a kind other than `range` | [F14 §6.7] | `neg/anchor-end-h--quote.moi` |
| `base64url` | non-canonical base64url (unused bits, padding, alphabet, length) | [F14 §2.6] | `neg/base64url--unused-bits.moi`, `neg/base64url--padding.moi`, `neg/base64url--alphabet.moi`, `neg/base64url--length.moi` |
| `anchor-hint-order` | a hint whose first line is greater than its last | [F14 §6.7] | `neg/anchor-hint-order--first-greater.moi` |
| `anchor-v0` | `v=0` | [F14 §6.7] | `neg/anchor-v0--v0.moi` |
| `json-lone-surrogate` | a lone surrogate in a JSON string | [F14 §2.4] | `neg/json-lone-surrogate--lone-surrogate.moi` |
| `json-string` | a JSON string with an escape RFC 8259 does not have (a line that matches no production) | [F14 §2.4] | `neg/json-string--bad-escape.moi` |
| `scope-text` | a scope text that is not the image of a scope value (a line that matches no production) | [F14 §5.6] | `neg/scope-text--scope-unescaped.moi` |
| `query-consistency` | a named-query file that breaks §7.2.2's consistency rules | [F14 §7.2.2] | `neg/query-consistency--name.moi`, `neg/query-consistency--params.moi`, `neg/query-consistency--shape.moi`, `neg/query-consistency--budget.moi`, `neg/query-consistency--not-define.moi` |
| `query-portability` | a named-query file that breaks §7.2.2's portability rules | [F14 §7.2.2], [LQ/lexical §10.2] | `neg/query-portability--node-literal.moi`, `neg/query-portability--trailing-space.moi` |
| `merge-markers` | leftover merge markers | [F14 §9.2] | `neg/merge-markers--markers.moi` |
| `trailer-malformed` | a malformed value of a known trailer | [F14 §10.4], §10.9 | `neg-commit/trailer-malformed--hlc.commit` |
| `trailer-repeated` | a repeated trailer | [F14 §10.9] | `neg-commit/trailer-repeated--kind.commit` |
| `trailer-algorithms` | `Moirai-Git-Head` and `Moirai-Git-Base` of different algorithms | [F14 §10.4] | `neg-commit/trailer-algorithms--git-algorithms.commit` |
| `trailer-sync-base` | a `Moirai-Sync-Base` that differs from its second parent's stated id | [F14 §10.4] | `neg-commit/trailer-sync-base--differs.commit` |
| `trailer-schema` | a schema version other than 1 | [F07 §15] | `neg-commit/trailer-schema--two.commit` |
| `foreign-parents` | a foreign commit with more than two parents | [F07 §12.3] | `neg-commit/foreign-parents--three.commit` |

Each negative is a positive fixture with one change (the `.expect` note says which). A harness asserts that the import stages `ImageParse`; comparing its own diagnosis with the rule id is optional. The commit negatives use the trees and parents of the carrier cases their `.expect` names (`fixtures/carrier/`, destination A).

## 5. How a harness uses the files

1. **Grammar** (WP-95): every file under `node/`, `schema/`, `query/`, `side/`, `tree/`, `superset/` parses by the ABNF of [F14] for its kind, after §9.1's normalisation for `superset/`; every file under `neg*/` is refused.
2. **Parse** ([F14 §11.1]): a positive file maps to the `keys` of its `.expect` (and the `image` data); a harness compares canonical encodings ([F07 §7]), not the notation's text.
3. **Re-encoding** ([F14 §15] rule 3): encoding the parse writes the bytes of the file named by `canonical` (`self`: the file itself).
4. **Equivalences**: `node/anchors-full`, `node/anchors-hash-only` and `node/anchors-mixed` have equal keys ([F07 §8.3]); every `superset/` file has the keys of its canonical twin.

## 6. Findings and gaps

Found while authoring and filed with the review. Every finding is settled by spec sync 2b (`docs/spec/reviews/spec-sync-2b.md`); the last column names the row, and the files follow the settled text.

| # | Where | Finding or gap | Files | Status |
|---|---|---|---|---|
| M-1 | [F14 §5.4] | A `path_moves` block is "sorted by (`hlc`, `from`, `to`, `class`, `git`), which for well-formed values is the bytewise order of the lines", but the JSON array puts the class before `from` (§5.2), so the bytewise order of the lines sorts by class before `from`: the two orders differ when two entries share an `hlc`. The files hold only entries on which both orders agree | `node/root-node.moi` | settled by S2B-R-15 (the lines are sorted bytewise: `hlc`, then the class before `from`; the importer re-sorts) and S2B-F-17 (the stored order is (`hlc`, `root` id, `from`, `to`, `class`, `git`)); the file's three lines are in bytewise order, as S2B-R-15 writes them |
| M-2 | [F08 §8.5.1], [F14 §6.2] | A project kind's initial status is "the default of its `status` field", but a project kind has no field row for `status` (the common field cannot be shadowed, [F08 §8.2]), so no text says which status value is initial, and the `status:` line of a project-kind node cannot be read as absent or not. The fixtures read the value of least `sort_rank` as initial and give their project-kind nodes statuses that are initial under no reading | `tree/project-schema/` | settled by S2B-F-19 ([F08 §8.5.1]: the initial status is the non-retired value with the least `sort_rank`, ties by value name bytewise). Re-checked: `experiment`'s initial status is `planned` (rank 0); its nodes hold `running` and `concluded`, so neither `status:` line is a written default. No carrier case holds a project-kind node |
| M-3 | [F14 §6.8], §6.8.1, §12.2 | The `prov` of an existence conflict is hashed ([F07 §7.3]) but has no carrier of its own; §6.8.1's last bullet implies it (a provisionally deleted node is a tombstone file). The chapter should state that `prov` is the side whose existence matches the file's form, and §12.2 should list it | `node/conflict-existence-*.moi` | settled by S2B-R-19 ([F14 §6.8.1], §12.2); the files already read it so |
| M-4 | [F14 §6.8.1] | "its title, header, flag and field keys as `title:` and `field <name>:` lines … sorted by field name" leaves open whether `title:` is first or sorted among the field lines. The files sort every one of them by field name (`field priority`, `title`, `field work_kind`) | `node/conflict-existence-*.moi` | settled by S2B-R-20 (`title:` sorts among the `field` lines under the name `title`); the files already sort so |
| M-5 | [F14 §6.8] | An observation side has one text for the absent composite and none for a present composite whose six members are all absent, which [F07 §7.2] class 5 tells apart (`of` 0 and 1). The files write the absent composite (a node absent on that side) as an empty side; the other case needs a live artifact without a `path`, which [F08 §8.6] rule 1 forbids | `node/conflict-observation.moi` | settled by S2B-R-21 (the empty side is the absent composite; a present one always holds `path`) |
| M-6 | [F14 §6.6], §9.1 | A symmetric edge written in the file of its larger endpoint is one key ([F07 §6.6]) but is neither in §9.1's superset nor in §9.2's refusals. The fixtures accept it (`fixtures/carrier/checkpoint-first-sym`) | — | settled by S2B-R-22 ([F14 §6.6], §9.1 rule 10: one key; different properties are a line repeated) |
| M-7 | [F14 §6.8] `edge.at` sides | "the JSON string of the anchor line's properties … as §6.7 writes them": the files read this as including the texts where the destination's mode and the store allow them, as the line does | `node/conflicts.moi` | settled by S2B-R-23 (the side carries the texts exactly where §6.7 writes them on the line) |
| M-8 | [F14 §7.2.2] | The default shape of a definition without `SHAPE` is still open ([F14] open point 28); every text here writes `SHAPE`. A definition without `BUDGET` stores `medium` ([LQ/std §2.4]) | `query/no-params.moi`, `query/no-shape.moi` | settled by S2B-R-18 (a definition without `SHAPE` stores `table`); `query/no-shape.moi` is that case |
| M-9 | [F14 §9.2] | Two `label` lines of one text are not in the "line repeated" list; the checking decoder refused them as a set with a repeated element. No fixture depends on it | — | settled by S2B-R-14 ([F14 §9.2]: two `label` lines of one text are a line repeated) |

## 7. Rows of `docs/spec/COVERAGE.md`

The fixture column of these rows can cite (R-SPEC fills the column):

| Row | Fixture |
|---|---|
| 60-AR-Image (image format v1: `.moi` ABNF, marker, side-ref layout) | `fixtures/moi/node/`, `fixtures/moi/schema/`, `fixtures/moi/query/`, `fixtures/moi/side/`, `fixtures/moi/tree/` |
| 60-AU-Image (digests on every `anchor` line, texts verified, `text-unavailable`) | `fixtures/moi/node/anchors-full.moi`, `fixtures/moi/node/anchors-hash-only.moi`, `fixtures/moi/node/anchors-mixed.moi`, `fixtures/moi/neg/anchor-text-digest--text-digest.moi` |
| R-1 (`path`, `oid`, `pathmove` text forms) | `fixtures/moi/node/run.moi`, `fixtures/moi/node/lane.moi`, `fixtures/moi/node/artifact.moi`, `fixtures/moi/node/root-node.moi` |
| R-2, R-17 (artifact fields, `planned`/`removed`, observation, `relink`) | `fixtures/moi/node/artifact-every-field.moi`, `fixtures/moi/node/artifact-removed.moi`, `fixtures/moi/node/artifact-planned.moi`, `fixtures/moi/node/conflict-observation.moi`, `fixtures/moi/neg/relink-grammar--lazy-magic.moi` |
| R-4, R-10, R-11 (anchor lines) | `fixtures/moi/node/anchors-full.moi`, `fixtures/moi/node/anchors-hash-only.moi`, `fixtures/moi/node/anchors-mixed.moi` |
| R-5 (root node, `path_moves`) | `fixtures/moi/node/root-node.moi` |
| F3 (named-query files) | `fixtures/moi/query/with-params.moi`, `fixtures/moi/query/no-params.moi`, `fixtures/moi/query/params-ht.moi`, `fixtures/moi/query/conflict-absent-side.moi` |
| X-F9 P11 (a) (hashed query file names) | `fixtures/moi/tree/project-schema/`, `fixtures/moi/neg-tree/query-file-name--hash-mismatch/` |

