# fixtures/carrier: the gate-0 carrier fixtures

| | |
|---|---|
| Title | Git commits of the image, one or more per commit kind and carrier row of [F14 §12.6]: native exports (ordinary, merge, sync, revert, cherry-pick, import-checkpoint), foreign commits, checkpoint commits, re-exports with `Moirai-Foreign-Git` and `Moirai-Parent`, a demoted commit, both anchor-text modes and a SHA-256 destination; each with its commit object, both trees and the canonical bytes its carriers rebuild |
| Work package | WP-21, second part: `carrier/` (R-FIX; [PLAN §3.2] item 1). `moi/` is the other half of this part |
| Acceptance | E3: WP-95's ABNF check passes every `.moi` file under `carrier/*/new/` and `carrier/*/old/` and re-derives the canonical items from the trailers; WP-91 reproduces every `changeset-digest` and `commit-id`. Gate 0 ([F14 §12.6], [AR §5b.7]) runs on these cases at M5. **Frozen for M5** ([PLAN §3.2] WP-21): a change needs a specification change that moves a byte |
| Separation | S1 ([PLAN §3.1]): written from the specification text only. The author read no line of `moirai-format-oracle`, `moirai-model`, `moirai-toylog` or any product crate, and ran no project code. The cases were produced by throw-away scripts: an exporter of [F14] (trees, provenance and ledgers by §6.3 and §6.5, commit objects and trailers by §10) over the synthetic histories of `fixtures/canonical/` and new ones, and an importer that rebuilds every item from the carriers of §12 alone. Every native case verifies (its rebuilt id equals its `Moirai-Commit`), every native case of `fixtures/canonical/` keeps its id there, and git itself agrees with every tree and commit id: each destination was written to a scratch bare repository with `git hash-object -w` and passed `git fsck --strict` (SHA-1 and SHA-256) |
| Sources | [F14 §10]–§12 (normative for the commit objects, trailers and the reconstruction), §3–§9 (trees and `.moi` bytes); [F07] (items, entries, C, `changeset_digest`, `commit_id`; §5 message normalisation; §12 ids per kind); [F06 §4.4.4] (the foreign `hlc`); [F08 §11] (derivations); [RULES/link-merge-rules] PC-002 (PathClaim); [RULES/merge-table] MC-014 (named queries); [git-objects] |
| Status | Written against the specification after review pass 1 (owner answers of 2026-09-28) |

Every value is synthetic: no owner data, no real paths, users or hosts; the store ids, the other store's commit ids and the observed git ids are labels hashed. `.gitattributes` gives `fixtures/** -text`: every byte is kept.

## 1. Destinations and stores

A destination is one git repository ([F14 §13.2]). The cases come from six:

| Destination | Object format | Anchor-text mode | Written by | Cases |
|---|---|---|---|---|
| A | sha1 | full | store S1 (native exports) and by hand (`hand-edit`, `demoted`, `demoted-child`) | the local history, R4, R5, trailers, the foreign hand edit, the demotion |
| C | sha1 | hash-only | store S1 | `anchor-full-hash-only`, `anchors-plus-hash-only` (parent chain `root` → `anchor-full-hash-only`) |
| H | sha1 | full | by hand | `defaults-explicit` (parent `root`) |
| F | sha1 | — | people with git (no trailer) | `foreign-root`, `foreign-child`, `foreign-clock-behind`, `foreign-merge` (ref `lane/tracker`) |
| K | sha1 | full | a store S3 at checkpoint granularity (ref `plan/storage`) | `checkpoint-first`, `checkpoint-first-sym`, `checkpoint-next`, `checkpoint-undo` |
| B | sha256 | full | store S2 (native re-exports) | `b-root`, `b-foreign-root`, `b-checkpoint-first`, `b-demoted`, `b-demoted-child` |

- **S1**, the local store, holds the histories of `fixtures/canonical/commits.cases`, `tombstones.cases`, `anchors.cases` and `normalisation.cases` (`defaults-omitted`), and eleven new commits: `anchors-plus`, `r4-register`, `r4-claim`, `r4-merge`, `q-define`, `q-change`, `q-lane`, `q-merge`, `trailers-json`, and the commits P (`pre-demote`) and Y (`demote-child`) whose exports a person rewrote in destination A. Its node files' `created:`, `updated:` and `deleted:` lines and its ledgers are the functions of its commit graph ([F14 §6.3], §6.5). The case `root` is one git commit in A, C and H (its tree has no anchor line).
- **S2** is a fresh store that imports A, C, H, F and K and re-exports five commits to B. S3 (the writer of K) is not in the fixtures: its commit ids in K (`Moirai-Head`, `Moirai-Folded`, provenance lines, ledger tokens) are unhashed labels that an importer keeps ([F14 §11.3]).
- The importer's view of a commit (the `p-state` and `q-state` of a case) is always S2's: the states it builds from the trees ([F14 §11.1]–§11.2).

## 2. A case directory

| File | Contents |
|---|---|
| `commit` | the git commit object's content, byte for byte (without git's `commit <len>` NUL header); its object id is SHA-1 or SHA-256 of `commit <len>` NUL and these bytes, by the destination's format |
| `tree.lst` | the commit's tree, recursively: one line `<mode> <type> <oid>` HT `<path>` per entry, a tree before its entries, in git's entry order. The mode is the one written into tree objects (`40000`, `100644`), not `git ls-tree`'s display form `040000` ([F14 §3.2]) |
| `parent-tree.lst` | the first parent's tree, in the same form (absent for a commit without a parent) |
| `new/<path>` | every blob of `tree.lst` that is not in `parent-tree.lst` with the same id |
| `old/<path>` | every blob of `parent-tree.lst` that is not in `tree.lst` with the same id (changed or removed) |
| `case.txt` | the expectations (§3) |

`new/` and `old/` are the touched blobs: the diff item 10 is built from ([F14 §11.1]: an unchanged blob contributes no entry). The full states of both sides are the `p-state` and `q-state` blocks.

## 3. `case.txt`

The framing is `fixtures/canonical/INDEX.md` §2.1's; blocks are ASCII with JSON escapes.

| Directive | Kind | Meaning |
|---|---|---|
| `case` | line | the case name (the directory name) |
| `source`, `note` | line, repeatable | the sections the case rests on; commentary |
| `destination` | line | `<name> <object format> <anchor-text mode>` (§1) |
| `class` | line | what [F14 §10.9] makes of the commit: `native` (verifies), `demoted` (a native candidate whose rebuilt id differs from its trailer), `foreign`, `checkpoint` (a checkpoint candidate) |
| `git-commit`, `git-tree` | line | the commit's and its root tree's object ids |
| `git-parent` | line, repeatable | a git parent's object id and the case that holds it, in order |
| `trailer-commit` | line | the `Moirai-Commit` value (native candidates) |
| `touched-new`, `touched-old` | line | the number of files under `new/` and `old/` |
| `p-state`, `q-state` | block | the importer's states at the first parent and at the commit, in the notation of `fixtures/canonical/INDEX.md` §3 with `fixtures/moi/INDEX.md` §3's extensions |
| `base-state`, `theirs-state` | block | `foreign-merge` only: the merge base and the second parent, for the typed merge that gives its item 10 |
| `native-entries`, `native-commit`, `native-commit-id` | block, block, line | `demoted` only: the failed native reconstruction |
| `entries` | block | informative: item 10's entries, `fixtures/canonical/INDEX.md` §4.1 |
| `digest-input` | block, hex | normative: the bytes whose BLAKE3-256 is `changeset-digest` ([F07 §10.4]) |
| `entry-count`, `changeset-digest` | line | n; the digest |
| `commit` | block | items 1–9 as values (`fixtures/canonical/INDEX.md` §4.2), as the importer rebuilds them (for a demoted commit: its foreign derivation) |
| `c`, `commit-id` | block (hex), line | normative: C ([F07 §3.1]) and its BLAKE3-256 |
| `canonical` | line | `<file> <case> commit-id`: `fixtures/canonical/cases/<file>` holds the same commit (same id); `<file> <case> changeset-digest`: it holds the same item 10 only (a foreign or checkpoint case whose item 9 there is a label) |
| `same-id`, `same-changeset` | line, repeatable | another case with the same commit id, or the same item 10 |

## 4. How a harness uses the cases (gate 0)

1. **Objects.** Hash `commit` and every file of `new/` and `old/`; rebuild the tree ids of `tree.lst` from its entries: they must equal `git-commit`, `git-tree` and the listing's blob ids.
2. **Classify** the commit by [F14 §10.9] and read its trailers (§10.4); the class must equal `class`.
3. **States.** Parse the `.moi` files of `new/` and `old/` ([F14 §6]–§9); with the unchanged files of the first parent (the `p-state`) they give the `q-state` ([F14 §11.1]–§11.2; for a removed live file, §11.2's foreign row).
4. **Item 10.** The entries of `q-state` against `p-state` ([F07 §10]) must encode to `digest-input`; for `foreign-merge`, the typed merge of `base-state`, `p-state` and `theirs-state` gives the `q-state` first.
5. **Items 1–9** from their carriers ([F14 §12.1]): trailers for a native commit; the git commit and the marker for a foreign or checkpoint commit ([F07 §12.3], §12.4, with [F06 §4.4.4]'s `hlc` over the parents' `hlc`, which the parents' cases give). They must equal the `commit` block; C must equal `c`.
6. **Verdict.** A `native` case's `commit-id` equals `trailer-commit`; a `demoted` case's `native-commit-id` does not, and its `commit-id` is its foreign id. Cases named in `same-id` share `commit-id`; `canonical` names the case of `fixtures/canonical/` with the same id or item 10.

The parents' ids a case needs (item 2 and the foreign `hlc`) are the `commit-id` and `commit` block of the case each `git-parent` names; a stated id that differs from the parent's own id comes from `Moirai-Parent` (`b-demoted-child`) or from the parent's `Moirai-Commit` (`demoted-child`).

## 5. The cases

| Case | Dest | Class | Kind | Parents | Entries | `commit_id` (first 16) | New | Old |
|---|---|---|---|---|---|---|---|---|
| `root` | A | native | ordinary | 0 | 23 | `64930f4cffe947e9` | 7 | 0 |
| `ordinary` | A | native | ordinary | 1 | 6 | `163e88e25904ad16` | 1 | 1 |
| `revert` | A | native | revert | 1 | 6 | `c425a75cab057fa2` | 1 | 1 |
| `main-c3` | A | native | ordinary | 1 | 3 | `e9e5a7d9c6589968` | 3 | 3 |
| `lane-x-1` | A | native | ordinary | 1 | 7 | `0c356fdc2d0fce91` | 4 | 3 |
| `lane-x-2` | A | native | ordinary | 1 | 2 | `1604505e547e1e81` | 1 | 1 |
| `lane-z-1` | A | native | ordinary | 1 | 4 | `cc912a175b764273` | 2 | 1 |
| `cherry-pick` | A | native | cherry-pick | 1 | 4 | `352fde46a717001f` | 2 | 1 |
| `merge` | A | native | merge | 2 | 7 | `7a68640429984a83` | 4 | 3 |
| `lane-y-1` | A | native | ordinary | 1 | 3 | `97e3cd7aad4af5cb` | 3 | 3 |
| `sync` | A | native | sync | 2 | 8 | `9fbedf9a38e44590` | 2 | 2 |
| `delete` | A | native | ordinary | 1 | 9 | `aaa574fb5321518f` | 1 | 1 |
| `undelete` | A | native | revert | 1 | 9 | `7ee5079d1979589c` | 1 | 1 |
| `anchor-full` | A | native | ordinary | 1 | 13 | `c2645145d5e8c99c` | 3 | 1 |
| `anchor-texts-added` | A | native | ordinary | 1 | 1 | `7d9dab189d18bd33` | 1 | 1 |
| `anchors-plus` | A | native | ordinary | 1 | 29 | `271bccbc4ad764b2` | 5 | 3 |
| `defaults-omitted` | A | native | ordinary | 1 | 4 | `6087a9137cdacdab` | 1 | 0 |
| `r4-register` | A | native | ordinary | 1 | 16 | `3452dc704892b196` | 3 | 2 |
| `r4-claim` | A | native | ordinary | 1 | 9 | `0b47a91dcf030bd5` | 1 | 0 |
| `r4-merge` | A | native | merge | 2 | 12 | `685b7617a108ceea` | 2 | 1 |
| `q-define` | A | native | ordinary | 1 | 2 | `7f6ec3a3e5ecf0a6` | 2 | 0 |
| `q-change` | A | native | ordinary | 1 | 2 | `b615381affeff301` | 1 | 2 |
| `q-lane` | A | native | ordinary | 1 | 2 | `3781463b0232de8d` | 2 | 2 |
| `q-merge` | A | native | merge | 2 | 2 | `cb4f7657f01cd024` | 2 | 1 |
| `trailers-json` | A | native | ordinary | 1 | 2 | `6afe1bcfe3813d5d` | 2 | 2 |
| `anchor-full-hash-only` | C | native | ordinary | 1 | 13 | `c2645145d5e8c99c` | 3 | 1 |
| `anchors-plus-hash-only` | C | native | ordinary | 1 | 29 | `271bccbc4ad764b2` | 5 | 3 |
| `defaults-explicit` | H | native | ordinary | 1 | 4 | `6087a9137cdacdab` | 1 | 0 |
| `hand-edit` | A | foreign | ordinary | 1 | 11 | `e6749af45b53a46c` | 2 | 3 |
| `demoted` | A | demoted | ordinary | 1 | 2 | `18464969ee21c7aa` | 1 | 1 |
| `demoted-child` | A | native | ordinary | 1 | 1 | `c79ab57d62c0ac72` | 1 | 1 |
| `checkpoint-first` | K | checkpoint | checkpoint | 0 | 24 | `d8741d715ca09d1b` | 7 | 0 |
| `checkpoint-first-sym` | K | checkpoint | checkpoint | 0 | 24 | `b813431411b2574f` | 7 | 0 |
| `checkpoint-next` | K | checkpoint | checkpoint | 1 | 10 | `3ee71912671f848e` | 3 | 3 |
| `checkpoint-undo` | K | checkpoint | checkpoint | 1 | 10 | `28b801b360454d81` | 3 | 3 |
| `foreign-root` | F | foreign | ordinary | 0 | 6 | `a4cc0fd93e114202` | 3 | 0 |
| `foreign-child` | F | foreign | ordinary | 1 | 1 | `e143819ccd708e83` | 1 | 1 |
| `foreign-clock-behind` | F | foreign | ordinary | 1 | 1 | `1d64fd51baf38e88` | 1 | 1 |
| `foreign-merge` | F | foreign | merge | 2 | 1 | `ba2d655b8c6e7cf3` | 1 | 1 |
| `b-root` | B | native | ordinary | 0 | 23 | `64930f4cffe947e9` | 7 | 0 |
| `b-foreign-root` | B | native | ordinary | 0 | 6 | `a4cc0fd93e114202` | 3 | 0 |
| `b-checkpoint-first` | B | native | checkpoint | 0 | 24 | `d8741d715ca09d1b` | 6 | 0 |
| `b-demoted` | B | native | ordinary | 1 | 2 | `18464969ee21c7aa` | 1 | 1 |
| `b-demoted-child` | B | native | ordinary | 1 | 1 | `c79ab57d62c0ac72` | 1 | 1 |

Twins with one id: `anchor-full` = `anchor-full-hash-only`, `anchors-plus` = `anchors-plus-hash-only`, `defaults-omitted` = `defaults-explicit`, `root` = `b-root`; `demoted-child` = `b-demoted-child`, `demoted` = `b-demoted` (foreign id), `foreign-root` = `b-foreign-root`, `checkpoint-first` = `b-checkpoint-first`. `checkpoint-first-sym` has `checkpoint-first`'s item 10 and another id (item 9 is its own git id).

## 6. Coverage

### 6.1 The gate-0 rows of [F14 §12.6]

| Row | Cases |
|---|---|
| `ordinary` | `ordinary` (one parent; `Moirai-Git-Head` and `Moirai-Git-Base`; a field, a status, a ledger line, a body, `edge cites … pin=`) |
| `merge` | `merge` (`conflict field.estimate class=FieldEdit`; clean keys from each side); also `r4-merge`, `q-merge` |
| `sync` | `sync` (8 entries against a residue of 1 key; `Moirai-Sync-Base`) |
| `revert` | `revert` (the inverse of `ordinary`), `undelete` |
| `cherry-pick` | `cherry-pick` |
| foreign | `hand-edit` (one parent, no trailer: a CR LF file, reordered lines, a ledger line with a non-commit token, a removed node file); `foreign-root`, `foreign-child`, `foreign-clock-behind`; `foreign-merge` (item 10 from the typed merge) |
| import-checkpoint | `checkpoint-first` (no parent), `checkpoint-next` (a previous checkpoint as parent, `Moirai-Folded` n = 2), `checkpoint-undo` (n = 0, +) |
| + re-exports | `b-foreign-root` (a foreign commit with `Moirai-Foreign-Git`), `b-checkpoint-first` (an import-checkpoint commit with `Moirai-Head`, `Moirai-Folded`, `Moirai-Foreign-Git`), `b-demoted-child` (the child of a demoted parent with `Moirai-Parent`); `demoted`, `demoted-child`, `b-demoted` |
| anchors | `anchor-full` and `anchor-full-hash-only` (a `range` anchor and a `quote` anchor with an empty prefix, one id); + `anchors-plus` and `anchors-plus-hash-only` (a `symbol` anchor with a scope and an occurrence, a `lines` anchor with a window, an anchor with `pred` and `marker`, a Latin-1 text, a `heading` and a `file` anchor, a repin) |
| tombstones | `delete` (flagged and historical retained edges), `undelete`, `merge` (X1 landed from the absent state; + T3's tombstone carrying `conflict existence`), `lane-x-1`, `lane-x-2`, `hand-edit` (a foreign removal) |
| normalisation | `defaults-explicit` (explicit defaults in the image and a message part that N normalises: the same id as `defaults-omitted`); `foreign-root` (N_imp of CR LF and trailing SP), `foreign-child` (FF → U+FFFD), `demoted` and `b-demoted` (a message that keeps an old trailer paragraph) |
| order | `checkpoint-first` (FA holds all eight key classes; two nodes; a symmetric edge; a schema field and a named query) and `checkpoint-first-sym` (the symmetric edge written from the other endpoint: the same item 10) |
| + R4 | `r4-register` (a file node with `origin_pred`, `aliases` and `observed_blob`; a root node gaining a `path_moves` entry; a `relink` change), `r4-merge` (`conflict observation class=PathClaim` on two nodes) |
| + R5 | `q-define` (defined), `q-change` (changed and dropped), `q-merge` (`FieldEdit` on a definition and `DeleteVsModify` with an absent side) |
| + trailers | `trailers-json` (`Moirai-Actor: "[bot] ci"`, a `Moirai-Worktree` with an HT); `revert` and others without git provenance; every `b-*` case in the SHA-256 destination B |

### 6.2 Specification sections

| Section | Cases |
|---|---|
| [F14 §3] tree, fan-out, modes and order | every `tree.lst` |
| [F14 §4] marker | every tree; B's markers name sha256 |
| [F14 §6.3] provenance lines | every native tree; the override of an import-checkpoint (`b-checkpoint-first`) |
| [F14 §6.5] ledgers | `ordinary`, `revert`, `delete`, `undelete`, `trailers-json`, `hand-edit`, `checkpoint-*`, `b-checkpoint-first` |
| [F14 §8] refs row files | `checkpoint-*` |
| [F14 §9.1] superset | `hand-edit`, `defaults-explicit`, `foreign-*` (no provenance lines) |
| [F14 §10.1]–§10.2 commit bytes and identities | every case; an empty role (`<@moirai.invalid>`) in `b-demoted` and `b-checkpoint-first` |
| [F14 §10.3] separation of the trailer block | every native case; `demoted`, `b-demoted` |
| [F14 §10.4]–§10.5 trailers and order | every native case; `Moirai-Parent` in `b-demoted-child`; `Moirai-Idem` in `ordinary`, `trailers-json` |
| [F14 §10.7] checkpoint commits | `checkpoint-*` |
| [F14 §10.9] classification | every case (`class`) |
| [F14 §11.2] file transitions | `hand-edit` (live → absent), `merge` (absent → tombstone), `delete`, `undelete` |
| [F14 §12] carrier table | every case |
| [F07 §12.2]–§12.5 ids per kind | native, foreign, checkpoint and demoted cases |

## 7. Rows of `docs/spec/COVERAGE.md`

| Row | Fixture |
|---|---|
| 60-I2-Gate0 | `fixtures/carrier/*/` (§6.1) |
| 60-AR-Image | `fixtures/carrier/*/commit`, `new/`, `old/` |
| 60-AU-Image | `fixtures/carrier/anchor-full*/`, `anchors-plus*/` |
| R-4, R-10, R-11 | `fixtures/carrier/anchors-plus/`, `anchors-plus-hash-only/` |
| R-2, R-5, R-17 | `fixtures/carrier/r4-register/`, `r4-merge/` |
| F3 | `fixtures/carrier/q-define/`, `q-change/`, `q-merge/` |

## 8. Findings and gaps

Found while authoring; to be filed with the review. The cases follow the reading stated here until it is resolved.

| # | Where | Finding or gap | Cases |
|---|---|---|---|
| C-1 | [F07 §5.2], §5.3 | A native import applies N to the message part; if §5.2's refusal of a last paragraph that begins with `Moirai-` applied there, a demoted commit re-exported natively (its message keeps its old trailer paragraph, [F07 §12.5]) could never verify. The cases read N at a native import as steps 1–4 of §5.1, with only step 1 able to demote (§5.3's last paragraph); §5.3 should say so | `b-demoted` |
| C-2 | [F14 §11.2] row live → absent | `repoint-or-flag` edges "stay with `flagged`"; [F08 §8.4.6]'s default policy keeps them flagged only when the source was an open blocker and removes them otherwise. The removed node of `hand-edit` is an unfinished blocker, so both readings agree here | `hand-edit` |
| C-3 | [F14 §10.9], [F10 §7] | A native git commit whose `Moirai-Commit` the store already holds from another git commit of the same destination (a hand-written twin of an exported commit) is not addressed: `gitmap` maps one git id per commit and destination. The twins here sit in different destinations (`defaults-explicit` in H) | `defaults-explicit` |
| C-4 | [F14 §6.6], §9.1 | `checkpoint-first-sym` relies on reading a symmetric edge from its larger endpoint's file (`fixtures/moi/INDEX.md` §6 M-6) | `checkpoint-first-sym` |
| C-5 | `fixtures/canonical/` | Part 1 stored the named query `stale_blockers` as a bare query; [F14 §7.2.2] and [LQ/lexical §10.2] store the whole `define_stmt`. Corrected there (G-6 of its INDEX): `checkpoint-first`, `checkpoint-first-sym` and `checkpoint-next` have new ids | `checkpoint-*` |
| C-6 | [F14 §12.6] row R4 | The `PathClaim` state of `r4-merge` follows [RULES/link-merge-rules] PC-001, PC-002 (each claiming node's observation key holds {base, ours, theirs} of its own composite); the case asserts the carriers, not the merge, and carries no merge rows | `r4-merge` |

