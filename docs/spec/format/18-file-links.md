# 18 — File links

| | |
|---|---|
| Title | File links: the R4 invariants I-F1…I-F14 (R-12), the binding-row extension and binding uniqueness (R-15), the frozen state, detail and header strings (R-16), the `relink` provenance vocabulary (R-17), and an index of where every other R4 reservation is specified |
| Chapter | [F18], `docs/spec/format/18-file-links.md` |
| Status | draft, pass 1 pending |
| Work package | WP-14 (R-SPEC-F), part c: `18-file-links.md` ([PLAN §3.2] item 1) |
| Sources | [40 §2.11] R-12, R-15, R-16, R-17 (authoritative) and rows R-1…R-11, R-13, R-14, R-18 (for the index); [40 §2.10] (I-F1…I-F14); [40 §1.2] (terms); [40 §2.2] (file-node fields, the `relink` row, the closed vocabulary table, scores, the `--confirm` mapping); [40 §2.3] (derivations, predecessor, dead uids, `#N` reuse, foreign uids); [40 §2.4] (roots, path rules, case, twins, "spelling differs on disk", `unrepresentable path`, `missing (not representable on this OS)`, `unmapped root`, `path_moves` classes, `[glob matches nothing]`); [40 §2.6] (binding rows, `FILEOBS` state incl. `ambiguous (path reused; original at q)`); [40 §2.7] (anchor fields, `captured`, `pred`, `aN`, de-duplication); [40 §2.8] (`at` edges, the discriminator); [40 §2.9] (file, anchor and agent-visible states, details, the closed `unverified` set, `none`, `unresolved`, header strings, severity order); [40 §3.1] (verbs, exit codes), [40 §3.2] (`planned`), [40 §3.4] (writer-tree-only file verbs, the recovery table and its detail `content changed after the move`), [40 §3.5], [40 §3.6] (`file relink --after`, provenance), [40 §3.7] (`links fix` actions, `--confirm`, `--split`, `--restore`, `[accepted guess]`), [40 §3.8] (illustrative examples); [40 §4.1] P1–P10; [40 §4.2] (settle points, CAS, quiescence); [40 §4.3] (the cascade, step 5 places, "What settle writes"); [40 §4.4] (classification, `replaced`, path reuse); [40 §4.5] (anchor cascade, watch semantics); [40 §4.6] (special patterns, Recycle Bin, rename-over, swap); [40 §5.1] (tree identity, eligibility, the reader note), [40 §5.2] (gate rows G1–G4), [40 §5.3] (writer tree, freshness, conflict values by observation, promotion), [40 §5.4] (lane lifecycle), [40 §5.5] (merge rules, re-key), [40 §5.6], [40 §5.7] (`text-unavailable`, import validation, never exported); [40 §6.1]–[40 §6.5] (JSON example, pack markers, MCP presets, `link_state`); [40 §9.2] decisions 1, 2, 6, 13, 16; [40] Review log, "A1 re-review (M0 WP-80a)"; [80 §2.10] P3, P4, P5, P9, P12; [80 §2.11.1] (the trash row); [80 §2.11.4] rules 2, 5, 9; [80 §3.1] X-F7, X-F8, X-F12; [AR §5a.1] (client head, `HEADS`), [AR §5a.4] (bindings, R-15), [AR §4.3] (commit header `git` group), [AR §4.6] ("Not hashed"), [AR §5e.2]–[AR §5e.8], [AR §6.5] (`ClientHead` durable), [AR §7.1] (output contract, header rules, the reader note ≤ 80 B, the `links check` example), [AR §8.3] TOKENS row "Link marker; CLI result header", [AR §13] (`files.main-tree`, `files.main-ref`, `files.policy.auto`, `files.pending-escalate`, `files.deletion-inference`, `files.confirm-roles`); [50 §2.6] (`link_state`, `f.state`, `a.state`, `none`, `unresolved`, W10), [50 §4.1] (`links_guesses`); [60 §2.5] (R4 rows R-12, R-15, R-16, R-17; audit row "Resolver constants (R-14)"; Cross-platform row); [90 §10.1] row "Output contract"; reviews `docs/spec/reviews/a1-A.md` A-M1, A-M3, A-m1, A-m4, `a1-S.md` S-01, S-03, S-05, S-07, S-08, S-13, S-14, S-19, `a1-P.md` A1P-14; [PLAN §3.2] WP-14, [PLAN §3.3] (the gap "R-12, R-15, R-16 and R-17, which no chapter held") |
| Depends on | [F01] (notation, symbols and the `git-branch` class, the `algo` registry and the fixed 32-byte id slot, hexadecimal text, order, reserved bytes); [F02] (`trash/`, the git hint); cites [F04], [F05], [F06], [F07], [F08], [F09], [F11], [F12], [F13], [F14], [F19], [F20], [LQ/envelope], [LQ/std], [LQ/errors], [OS/path], [OS/project], [CFG], [RULES/link-merge-rules] |

## 1. Scope and conventions

### 1.1 What this chapter fixes

[40 §2.11] reserves eighteen items for R4 in format v1. [PLAN §3.3] found that four of them had no chapter: R-12, R-15, R-16
and R-17. This chapter is their home:

- **R-12** (§2): the full statement of every invariant I-F1…I-F14, with the terms made exact. [F13 §3.9] cross-lists each one
  with its enforcement point, model function and gates.
- **R-15** (§3): the binding-row extension (`BindingExt`, 40 bytes), the designation of trees, the checks that keep I-F12,
  and the writer-tree predicate that reads the extension.
- **R-16** (§4): the closed set of link-state strings, detail strings and header strings, with their exact bytes, their
  composition, and the `u8` codes that runtime rows use for them.
- **R-17** (§5): the closed `relink` grammar, its evidence tokens and scores, who writes which value, and the `--confirm`
  mapping.
- §6 indexes where every other R-row is specified.

This chapter defines no file and no log record. `BindingExt` is embedded by the `HEADS` binding row ([F11]) and by the
durable record that writes a binding ([F05]); the strings of §4 are rendered by [F19] and [LQ/envelope]; the `relink`
value is a text value of the observation composite, encoded by [F06], hashed by [F07] and exported by [F14].

### 1.2 Terms

| Term | Meaning in this chapter |
|---|---|
| **view** | the materialised state of one ref at one of its commits ([AR §5a.3]). "On every view" means at every commit of every ref, not only at tips |
| **live** | a node that is not engine-deleted (no tombstone) on the view. A node with status `removed` is live but not `present` |
| **file node** | a node of kind `artifact` ([40 §2.2], R-2), uid derivation `file-key` ([F08], R-3) |
| **root node** | the per-root node of kind `area` with uid derivation `root-key` and fields `root`, `path_moves` ([40 §2.4], R-5) |
| **key of a file node** | the pair (root name, path bytes): the name of the node's `root` field and the bytes of its `path` value, compared bytewise ([F01 §6.6]) |
| **`at` edge** | the adjacency (src, `at`, dst) from any node to a file node ([40 §2.8]). It is stored only as **edge keys** (src, `at`, dst, disc), one per anchor ([F06], [F07] item 10) |
| **anchor** | the edge-property record of one `at` edge key: its selectors, `captured`, `pred`, `resolver` and the store-local `aN` ([40 §2.7], R-4) |
| **tree** | one working copy of a root: the exact git top-level of a worktree (the directory holding the `.git` entry, [F02 §3.4]), or, without git, a bound directory ([40 §1.2], [40 §5.1]). A nested worktree is its own tree |
| **tree identity** | two canonical roots ([OS/path §2.3]) name one tree iff their root `OsFileId`s are the same object, else iff their canonical texts are equal ([OS/path §4.4]: by id first, by spelling second) |
| **binding row** | a `HEADS` row whose key is a directory key `blake3_16(canonical directory)` ([AR §5a.1], [OS/path §4.5]) and whose target is a ref (§3.1) |
| **designated tree** of branch B | the tree that the designation relation D (§3.4) pairs with B |
| **on the line** | the predicate of §3.6 over a tree and its binding |
| **writer tree** of B | B's designated tree while it is on the line (§3.6) ([40 §5.3]) |
| **reader tree** | any other eligible tree ([40 §5.1]): it resolves and displays, and its settles write only `PENDING` rows |
| **fresh** | writer tree T is fresh for file node F iff F's `observed_git` is empty, or `observed_git` is an ancestor of or equal to T's HEAD H (gate row G2), or F's `path` is in τ(H) (G1), or E6 over the integration window found a chain that starts at F's `path` (G4) ([40 §5.3], [F20 §5.11]) |
| **settle** | a command at a settle point of [40 §4.2] (`links sync`, the `SessionStart` settle, the file verbs and `links fix` for their own links, `complete`'s separate settle commit, the merge ritual, the git `post-commit` block) |
| **automatic re-bind** | a settle's `SetField` of a file node's observation composite whose new `relink` value has `how` ∈ {`lazy`, `git`, `hook`, `policy`, `merge-observation`} (§5) |
| **explicit door** | a recorded command that states intent: `file mv`, `file rm`, `file relink --after`, `links fix` and its actions, `Undelete`, and the history verbs `revert`, `undo`, `cherry-pick`, `op restore` |

### 1.3 Notation for strings

- Every string of §4 and §5 is given as its exact bytes between back-quotes. Every byte a template contributes is ASCII
  ([90 §10.1] "ASCII only in every rendered string"; [LQ/envelope §1.3] "template bytes"). Value bytes (paths, ref names,
  quoted text) keep their UTF-8.
- A template is fixed text with **slots** in angle brackets. The slot types:

| Slot | Bytes |
|---|---|
| `<c8>` | `c` followed by the first 8 lower-case hexadecimal digits of a moirai commit id ([F01 §6.4]; the text form of [LQ/envelope], review A-m7) |
| `<g7>` | the first 7 lower-case hexadecimal digits of a git object id (the envelope's `files` field form) |
| `<n>` | a decimal count ([F01 §6.5]) |
| `#<n>` | a node number: `#` followed by its decimal |
| `<score>` | a score in the two-decimal form of §5.3 (`0.00`–`1.00`) |
| `<path>` | the bytes of a stored root-relative path (I-F8), unchanged |
| `<age>` | an elapsed wall time as [LQ/envelope §3.2]'s age without ` ago`: the largest of the units `d`, `h`, `m`, `s` whose floor is ≥ 1, written `<n>` followed by the unit letter (`3m`, `16d`); `0s` below one second. The start is the `hlc` the detail names, converted as [F01 §5.7] |
| `<ev>` | an evidence token of §5.2 |
| `<text>` | untrusted text quoted, escaped and cut by [LQ/envelope §5.16] |
| `<quote>` | untrusted text escaped by [LQ/envelope §5.16] and cut by §4.7 rule 5 |
| `<ref>` | a git branch name in the short form of §3.2 |
| `<label>` | a tree's display label ([AR §7.1], [F19]) |

- `[x]` marks an optional part of a template. Nothing else in a template is optional.

## 2. R-12: the invariants I-F1…I-F14

[40 §2.10] is authoritative for these statements and [AR §5e.8] summarises them. Each subsection gives the statement,
normative, and then the precision this specification adds; where the precision closes a gap, the open points say so.
[F13 §3.9] lists each invariant's enforcement point, check class, model function and gates, and cites this section for the
wording (§2.15).

### 2.1 I-F1 — one live file node per (root, exact path)

**Statement.** On every view, at most one live file node with status `present` or `planned` exists per (root, exact path).
The rule is enforced at write time. After a merge, a duplicate is a `PathClaim` conflict value, never a silent pair.
Case-insensitive and normalization-insensitive collisions are a resolve-time state, because case and normalization
sensitivity are properties of a tree, not of versioned data.

**Precision.**
- The key is the file node's key (§1.2): exact bytes, never folded. Two keys that differ only under `fold_v1` are distinct
  keys; their collision is the twin rule of [F20 §3.5], never an I-F1 violation.
- Nodes with status `removed` are outside the rule: a removed node may share its key with a live `present` node, and the
  predecessor rule of [40 §2.3] depends on it.
- Two live `present` or `planned` nodes may share a key only while both carry an unresolved `PathClaim` conflict value on
  that key ([F12]). Resolving the conflict (`links fix --same-as`, or a settle that unifies the two by an E6 rename in a writer
  tree fresh for both, [40 §5.5]) leaves at most one.
- Every write that would make a second live `present` or `planned` node hold an existing key on the view without such a
  conflict value is refused before any byte lands ([F19] gives the code). A capture that finds the key already held reuses
  that node ([40 §3.3]).
- `PATHIDX` ([F09], R-8) indexes exactly the keys of this rule, ordered by (root, `fold_v1(path)`, path).

### 2.2 I-F2 — uids equal their derivations

**Statement.** A file node's uid equals the derivation of [40 §2.3] over its stored `root`, `origin_path` and `origin_pred`.
A root node's uid equals the derivation over its `root`. An anchor's uid equals the derivation of [40 §2.7] over its source
node's uid and its stored `captured` and `pred`. A `Create` of a uid the store already knows, on any branch, reuses that
uid's `#N`.

**Precision.**
- The derivation functions and their byte inputs are [F08]'s (R-3), built with `lp()` ([F01 §6.3]) and BLAKE3-128
  ([F01 §7.1]). Every input is stored and immutable: `origin_path` and `origin_pred` are identity-class fields, and
  `captured` and `pred` never change (a repin changes selectors only). Every uid is therefore recomputable by
  `doctor --verify` and by image import.
- A node or anchor imported with a uid that does not match its derivation is accepted as **foreign**, flagged in
  `image doctor`, and treated as random from then on ([40 §2.3], [40 §5.7]). For such a node I-F2 requires nothing more:
  no chapter encodes a foreign mark, because the mismatch is a property of the stored inputs that every reader and importer
  recomputes ([F08 §11.5], [F14 §9.3]; pass 1, A1-44).
- `#N` reuse goes through the store-wide `UIDX` probe under the writer byte ([F11], [AR §4.5] step 8). The anchor analogue
  is [40 §2.7]'s: a merge, sync or import that lands an anchor uid the store already knows reuses its `aN` (A1P-12).

### 2.3 I-F3 — anchors and `at` edges

**Statement.** Every `at` edge carries at least one anchor. Every anchor belongs to exactly one `at` edge. Anchor uids are
unique per (src, dst).

**Precision.**
- An `at` edge exists on a view iff at least one edge key (src, `at`, dst, disc) is present there. An `at` edge key always has
  a discriminator: a key of kind `at` without one is invalid ([F06], [F07]).
- The discriminator of a key equals the uid of the anchor record the key carries. An anchor record is carried by exactly one
  key.
- `unlink ID --at aN` removes one key; removing the last key of (src, dst) removes the adjacency ([40 §3.2]).
- Capture never creates a second anchor on (src, dst) whose current selectors equal an existing anchor's: it reuses that
  anchor ([40 §2.7] "Capture de-duplication"). Two anchors of one (src, dst) with equal `captured` differ in `pred`.

### 2.4 I-F4 — nothing machine-local in versioned, hashed or exported data

**Statement.** OS file ids, volume serials and volume keys, mtimes, creation times, stat caches, resolution states,
proposals, and the rows of `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `PREFIXEV`, `GITFACTS`, `TREES`, `DIRMAP`,
`ANCHORRES`, `JOURNALCUR` and the binding rows with their `BindingExt` never appear in versioned, hashed or exported data.

**Precision.**
- *Versioned* data is every op of every commit ([F06]); *hashed* data is every canonical item and every digest over them
  ([F07]); *exported* data is every object of the image, its trailers and side refs ([F14]).
- The runtime rows are those of [40 §2.6], which [40 §2.6] declares "never versioned, never merged, never exported and never
  hashed"; the binding rows are among them ([40 §2.6] row "binding rows"). The LQ tree-derived built-ins compute from them at
  read time only ([50 §2.6]).
- I-F4 does not cover the machine-local paths that the design versions on purpose: root `abs` values ([80 §2.10] P12) and the
  commit header's `git.worktree` provenance ([AR §4.6] item 5). Those are versioned by design and are not R4 runtime data.

### 2.5 I-F5 — reads append nothing

**Statement.** Read verbs — `show`, `pack`, `brief`, `get`, `find`, `q`, `links check`, `links mentions`, `file where` and
every MCP read — append nothing to the log. `check` is a write verb, and so is every settle point of [40 §4.2].

**Precision.**
- *Append nothing*: during the verb the process appends no log record of any kind, durable or lazy, writes no `HEAD` slot
  and creates no store file.
- The LQ built-ins `link_state()`, `f.state`, `a.state` and `links()` are part of whatever read evaluates them.
- A read may use `GITFACTS`, `FILEOBS` and `ANCHORRES` rows and keeps the facts it computes in process memory for the
  command only ([40 §2.6]); the MCP server's resolution LRU is process memory ([40 §4.8]).
- The one lazy `feed` 2 pack cursor of C8 ([F05 §9.11]; [AR §7.4]) is not part of the read. It is appended after
  delivery by the layer that delivers a pack, the `SubagentStart` hook or the MCP server ([AR §5d.1]), as owner question
  OQ-F-3 decided on 2026-09-28 (option (b)). The `pack` verb and every MCP read verb still append nothing, and I-F5's
  counting test counts the verb, not the delivery.

### 2.6 I-F6 — when an automatic re-bind is written

**Statement.** An automatic re-bind is written only when all of these hold:
1. its evidence is exact, or `files.policy.auto = strong` and the evidence is a unique strong candidate;
2. the settle runs in the writer tree of the branch it writes (§3.6);
3. that tree is fresh for the node;
4. the quiescence re-check passed ([F20 §5.17]);
5. on `main`, the observation is committed in the writer tree's HEAD: the new path q is in τ(H), so `observed_blob` is set.

**Precision.**
- "Exact" and "strong" are [F20 §1.5]'s evidence classes; a unique strong candidate is [F20 §5.5] selection step 3 with one
  distinct `strong` target, never a `copy` candidate. A re-bind under condition 1's second branch records `policy/…`
  (§5.4).
- A resolution of a composite conflict value by observation (`merge-observation/…`) additionally requires the tree to be
  fresh for every side's value and the chosen path's evidence to be exact ([40 §5.3] "Conflict values").
- A `PENDING` promotion is an automatic re-bind: all five conditions apply, with [40 §5.3] "Promotion"'s own rule.
- Companion rule, enforced at the same point: every settle write carries the file node's `rev_seq` as read when its
  resolution started and is dropped if the node changed meanwhile ([40 §4.2]).
- A `planned → present` binding is not a re-bind; it is written only by a writer tree, under [F20 §5.18].

### 2.7 I-F7 — `removed` is never inferred from absence

**Statement.** A file node's status becomes `removed` only through `file rm`, `links fix --drop`, `links fix --same-as`,
`links fix --split`, or, with `files.deletion-inference = main-tree-commits`, a settle in the writer tree of `main` that sees
the file's deletion committed in that tree's HEAD. Merges, syncs, imports and the history verbs carry such a change from the
commit that made it; they never originate one.

**Precision.**
- Absence — a missing path, a failed or denied stat, a location in the Recycle Bin or the trash, a git deletion under the
  default `files.deletion-inference = explicit` (rendered `missing (deleted in git)`, §4.6) — never writes `removed`.
- `links fix --split` writes `removed{reason: split}` ([40 §3.7]); [40 §2.10]'s list omits it (open point 22).

### 2.8 I-F8 — path rules

**Statement.** Stored paths are root-relative, `/`-separated, with no empty, `.` or `..` segment and no leading `/`; they
are valid UTF-8 stored as exact bytes, with no Unicode normalisation except [80 §2.10] P3 (NFC for untracked names on a
normalization-insensitive volume). The one exception is root `abs`, whose paths are machine-local absolute paths in the form
of [80 §2.10] P12, checked for existence and `oid` only. The root of every path value of a file node — its `path`, its
`origin_path` and each of its `aliases` — is the node's `root` field, and the root of the `from` and `to` of every entry of a
root node's `path_moves` is that root node's `root` field.

**Precision.**
- A stored path of a root other than `abs` is a non-empty `rel-path` of [OS/path §2.1]; of root `abs`, an `abs-path` of
  [OS/path §2.2].
- `pathmove.from` and `pathmove.to` are directory prefixes: a non-empty `rel-path` followed by exactly one `/`
  ([40 §2.4]). The `abs` root never has `path_moves` entries, because `abs` paths are never re-bound ([80 §2.10] P12).
- The path value carries its root as a store-local symbol ([40] R-1, [F01 §8.2] class `root`); canonical item 10 and the image
  carry it by name ([F07], [F14]; review S-19). The root clause of the statement makes that name redundant but checkable: a
  value whose root differs from its node's `root` field is refused at write time and at import ([F19]; `ImageParse` at
  import).

### 2.9 I-F9 — no bare line numbers

**Statement.** No live span anchor has a bare line number as its only selector. Every `quote`, `range`, `symbol` and
`heading` anchor carries a quote, and every `lines` anchor carries a window.

**Precision.**
- A span anchor is an anchor of any kind other than `file`.
- *Carries a quote*: its `quote.exact` is non-empty and its digest `quote_h` is in the selector block ([F07], R-10). After a
  hash-only import only the digest remains; the anchor is then `text-unavailable` (§4.6) and still satisfies I-F9.
- *Carries a window*: its window value ([F20 §2.7.3]) has `n_before + n_after ≥ 1`. A `lines` capture whose window would be
  empty — a file in which no non-trivial line lies outside the span — is refused (open point 23).

### 2.10 I-F10 — resolution is a pure function

**Statement.** `resolve(versioned link, tree snapshot with its git objects, the tree's runtime rows, resolver version,
budgets and files.* keys)` is a pure function. Thresholds and pattern lists are constants of the resolver version ([F20]), and
a version bump is a visible event. The caches `ANCHORRES`, `GITFACTS`, `FPRINT` and the MCP server's resolution LRU never
change an answer, and `unverified` is the only output that depends on the budgets and keys.

**Precision.**
- The inputs are exactly [F20 §1.3]'s list (a)–(e) (review S-13): the tree's runtime rows are `FILEOBS`, `PENDING`,
  `FSINTENT`, `TREES` and `DIRMAP`.
- *Visible*: every `--json` link object carries the resolver version under which it was computed ([F19] names the member),
  and the first result of a process whose resolver version differs from the version recorded in the runtime rows it reads
  carries a notice ([LQ/errors] numbers it; open point 25). Rows of another version count as absent ([F20 §1.3]).

### 2.11 I-F11 — hands off project files

**Statement.** moirai opens project files only with full sharing — `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`
on Windows; read-only, no-follow and without access-time updates on Linux and macOS — never maps or locks them, closes each
handle before touching the next file, and never opens the content of a cloud-only entry on an automatic path.

**Precision.**
- A project file is any file under a root of any kind. An automatic path is a read, a settle or a hook; an explicit verb may
  open cloud-only content only with `--allow-hydrate` ([40 §4.6]).
- The per-OS calls are [OS/project]'s ([80 §2.12] row "Project-file reads").
- Related rule ([40 §9.2] decision 3, DR9): moirai never writes into a project file, never changes its attributes, and
  creates no object id and no alternate data stream; the only changes it makes to a project tree are the renames and deletes
  of `file mv`, `file rm` and `file revert`.

### 2.12 I-F12 — binding uniqueness

**Statement.** Each moirai branch has at most one designated tree, and each tree is designated for at most one branch. Trees
are identified by their exact git top-level (without git, by the bound directory), looked up by the root's `OsFileId` first
and by canonical spelling second, and a binding of a directory never covers a nested worktree inside it.

**Precision.** §3.4 defines the designation relation D and the checks at the binding verbs; §3.6 defines the writer tree.

### 2.13 I-F13 — no content-only re-bind

**Statement.** An automatic re-bind never rests on equal content alone: an equal-`oid` candidate needs corroboration by
creation time (only where the per-OS rule allows it), by a git rename inside one commit, or by captured intent, and a
candidate that coexisted with the original is never a target.

**Precision.** The copy rule of [F20 §5.9] is the definition. "Equal content" is `oid` equality under one algorithm; a pair of
different algorithms is "content unknown" and never equal ([F20 §2.3], review S-18).

### 2.14 I-F14 — no resurrection

**Statement.** A derived uid that is `removed` or engine-deleted on a view never becomes live again on that view through
registration or merge. Only explicit, recorded doors bring it back: `links fix --restore`, `Undelete`, and a `revert`,
`undo`, `cherry-pick` or `op restore` of the commit that removed or deleted it, which re-apply history through the ordinary
rules.

**Precision.**
- *Live again* means status `present` or `planned` without a tombstone.
- Registration derives a new uid (the dead-uid rule, [F08], R-3); a merge re-keys the node created on one side since the LCA
  to `uid′ = uid(root, origin_path, U)` and re-points every edge that side added ([40 §5.5], [RULES/link-merge-rules]).

### 2.15 Cross-listing in [F13]

[F13 §3.9] states each I-F row with its enforcement point, model function and gates. Its statements were written before this
chapter and differ from §2.1–§2.14 in four places: I-F2 omits the anchor's `pred` input, I-F7 omits `links fix --split`, I-F8
omits the root clause of review S-19, and I-F14 omits the history-verb doors of review S-14. §2.1–§2.14 are the statements
of record; [F13 §3.9] cites them for each statement, lists the four differences and keeps only its own columns (open
point 21; pass 1, A1-44).

## 3. R-15: tree bindings

### 3.1 Binding rows

- A **binding row** maps a directory to a moirai ref: a `HEADS` row whose key is a directory key ([AR §5a.1]). `moirai
  worktree bind`, `lane open` and `init` without git write them ([AR §2.14], [AR §5c]). Rows keyed by a `--client` name or a
  `session:<harness>:<id>` key are client heads, not bindings, and designate nothing.
- R-15 extends every binding row with the 40-byte `BindingExt` of §3.2: the `designated` flag, the **expected git ref** and the
  **base commit** of the tree ([40] R-15, [40 §2.6], [AR §5a.4]).
- A binding is durable ([AR §6.5]; [40 §2.6] "durable, like every [AR] binding"), machine-local and runtime: never versioned,
  hashed or exported (I-F4).
- A binding row's key resolves the **branch** of a command by the longest bound prefix ([AR §5a.4]); only the designation of
  §3.3 makes a tree a writer of file-link re-binds.

### 3.2 `BindingExt`

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 1 | `u8` | `bflags` | bit table below |
| 1 | 1 | `u8` | `base_algo` | object format of `base`, a value of the `algo` registry ([F01 §7.5]): 0 `none` (no base), 1 `sha1`, 2 `sha256` |
| 2 | 2 | `[2]u8` | `_reserved` | zero |
| 4 | 4 | `u32` | `expected_ref` | symbol id of class `git-branch` ([F01 §8.2]): the expected git ref in the short form below; 0 (the empty string) = none |
| 8 | 32 | `[32]u8` | `base` | the base commit id in the fixed 32-byte id slot of [F01 §7.5] (`digest_lo` 20 B, `digest_hi` 12 B); all zero when `base_algo` = 0 |
| total | 40 | | | |

| bit | name | meaning |
|---|---|---|
| 0 | `designated` | 1: the key's directory is the designated tree of the row's ref (§3.3). 0: the row only resolves branches |

Bits 1–7 are reserved-zero ([F01 §10]).

**Rules.**
1. **Short form of a git ref.** `expected_ref` names a branch as the bytes after `refs/heads/` of its full ref name, as git
   spells it (`u/l5np`, not `refs/heads/u/l5np`). A value given with the `refs/heads/` prefix is stored without it. A branch
   name that is not valid UTF-8 cannot be recorded, and the verb refuses it (exit 2, [F19]). The short form is the **one
   spelling** of a git branch in every stored and hashed form: the commit header's `git.branch` ([F06 §4.4.6]) and canonical
   item 5 ([F07 §3.6]), which share the class, the `HEADS` row and the `ClientHead` record (§3.7), and the comparison of
   §3.6 (pass 1, S1-10; open point 4). P1-2 and A1-8 preferred the full name; the short form is kept because item 5
   already hashes it and a second spelling would intern two symbols for one branch.
2. **No git line without designation.** When `designated` = 0, `base_algo`, `expected_ref` and `base` are zero.
3. **Validity.** A `BindingExt` is invalid when a reserved bit or byte is non-zero, when `base_algo` is not 0, 1 or 2, when
   `base_algo` = 0 and `base` is not all zero, when `base_algo` = 1 and `digest_hi` is not zero, or when rule 2 is broken. An
   invalid extension makes its row non-designated for every purpose (fail closed); `doctor --fsck` reports it. The validity of
   the enclosing record is [F05]'s.
4. **Byte order.** `expected_ref` is little-endian ([F01 §4.1]); `base` is a byte string.

*(Informative)* The binding row of [40 §2.6] is ≈ 60 B: the 16-byte key, the 4-byte `ref_id` and this 40-byte extension;
[F11 §5]'s `HEADS` row, which also holds the directory's `root_id`, the detached-head fields and the key text, is 161 B
with this extension at offsets 105–144. A designated binding of `lane/l5np` expecting `u/l5np` (symbol id 7) at the SHA-1
base `7c1e0a4d…` begins `01 01 00 00 07 00 00 00 7c 1e 0a 4d …`, with 12 zero bytes at offsets 28–39 (row offsets
133–144); the same 40 bytes sit at the same place in the `HEADS` row image its `ClientHead` record carries
([F05 §9.3]). This is the byte
fixture of `COVERAGE.md` row R-15 (pass 1, P1-2; R-FIX builds the files).

### 3.3 Designation

A binding row may carry `designated` = 1 only if all of these hold when it is written:
1. its key's directory is a tree (§1.2): the exact git top-level of a worktree, or, when the walk of [F02 §3.1] step 3 finds no
   `.git` entry at or above it, the directory itself. A binding of any other directory — a subdirectory of a tree, or a
   directory that holds worktrees — resolves branches only ([40 §5.1], [41 m13]);
2. its target is a ref, not a detached commit;
3. the checks of §3.5 passed.

Only the binding verbs of §3.5 (enforcement point EP-BD of [F13 §2]) write `designated` = 1.

### 3.4 The designation relation D

- **Rows.** D_heads is the set of pairs (B, T) for which a valid binding row with `designated` = 1 has ref B and a key whose
  directory is tree T.
- **`main` by configuration.** Let T_m be the tree named by the effective `files.main-tree` ([AR §13], [CFG]), canonicalised
  by [OS/path §4]. The pair (`main`, T_m) belongs to D iff D_heads holds no pair with branch `main` and no pair with tree
  T_m. Its expected ref is the effective `files.main-ref` in the short form of §3.2 rule 1 (none when unset or empty), and it
  has no base.
- D = D_heads ∪ {(`main`, T_m)} when that pair belongs, else D_heads. Trees in D are compared by tree identity (§1.2).
- **I-F12 over D.** No two pairs of D share a branch, and no two share a tree. The checks of §3.5 keep D_heads injective
  both ways, and the configuration pair is included only when it keeps D injective, so D is a partial one-to-one relation by
  construction.
- **Fail closed.** If D_heads is found not injective at read time (a store restored from a backup made under another
  binding, or a defect), every tree of a pair that shares its branch or its tree with another pair is a reader tree for every
  branch, and `doctor lanes` reports the pairs. No re-bind is written in such a tree until the bindings are repaired.
- **Nested worktrees.** Designation is by tree identity only, so a binding of a directory never designates a nested worktree
  inside it; the nested tree is a reader of whatever branch the longest bound prefix resolves ([40 §5.1]).

### 3.5 What the binding verbs write

| Verb | `designated` | `expected_ref` | `base` | Refusals |
|---|---|---|---|---|
| `lane open NAME --worktree DIR [--git-branch B] [--base SHA]` | 1 (DIR must be a tree, §3.3, else exit 2) | B; without `--git-branch`, the branch DIR's HEAD names at open; none when DIR's HEAD is detached or DIR has no git | SHA resolved to the full id of a commit in DIR's repository (a unique prefix; else exit 2); without `--base`, DIR's HEAD commit at open; none without git | the checks below |
| `worktree bind DIR REF [--replace]` on a tree | 1 | the branch DIR's HEAD names at bind; none when detached or without git | DIR's HEAD commit at bind; none without git | the checks below |
| `worktree bind DIR REF` on a directory that is not a tree | 0 | 0 | none | — (the result says `binding only, not a designated tree`; [F19] text) |
| `worktree unbind DIR` | the row is removed | — | — | — |
| `init` outside any repository ([40 §5.1], [AR §5e.7]): a binding of its working directory to `main` | 1 | none | none | — |
| any other write of a directory key's row (for example `checkout REF` resolving to that key, [AR §5a.4]) that changes the row's ref | 0 | 0 | none | — (a designation it clears is named in one line of the result) |

**The checks** of `lane open` and of `worktree bind` on a tree T for branch B, in order ([40 §5.3], I-F12):
1. If D pairs B with a tree T0 ≠ T: refused, exit 5, naming T0, unless `--replace` is given. With `--replace`, T0 loses the
   designation. When T0's pair comes from a binding row, that row is rewritten with `designated` = 0 and a zero extension in
   the same durable group as T's row (it keeps resolving branches). When it is the configuration pair of `main`, T's new
   designated row for `main` removes it from D by §3.4, and `doctor lanes` warns that `files.main-tree` no longer names
   `main`'s designated tree.
2. If D pairs T with a branch B1 ≠ B: refused, exit 5, naming B1, unless `--replace` is given. With `--replace`, T's row is
   rewritten for B, which ends the pair (B1, T). When T = T_m and B1 = `main`, the configuration pair drops out of D by §3.4
   and `doctor lanes` warns that `files.main-tree` names a tree designated for another branch.
3. Otherwise the row is written as the table says.

The agent-facing text of these refusals names only the orchestrator ([40 §3.4], review A-m4); the bind command is printed for
the orchestrator and owner roles. [F19] owns the texts.

**Never changed by anything else.** No hook (the git `post-checkout` block included), read, settle, sync, merge, import,
`config set` or `doctor` run changes `expected_ref` or `base`. [AR §5c]'s "`hook git-post-checkout` (refreshes the binding's
provenance)" refreshes provenance shown to the user, never these two fields: following a `git switch` with them would make a
switched lane tree a writer for another branch's paths, the defect [41 M2] removed (open point 6). `lane.git_branch` and
`lane.base_sha` are versioned copies; the binding is authoritative for §3.6, and `doctor lanes` reports a difference.

### 3.6 On the line: the writer-tree predicate

Let T be the designated tree of B with binding W (from D), and let T's HEAD be read textually from `<gitdir>/HEAD` as in
[F02 §3.4]. T is **on the line**, and so B's writer tree, iff one of these holds ([40 §5.3], [40 §5.8]):
1. **No git.** T has no `.git` entry, and W has no expected ref and no base.
2. **Symbolic HEAD.** T's HEAD is `ref: refs/heads/<name>`, and `<name>` equals `expected_ref`'s string bytewise.
3. **Detached HEAD.** T's HEAD is a commit h; W has a base b and an expected ref e; the ref `refs/heads/e` exists in T's
   repository with tip t; h is b or a descendant of b; and h is an ancestor of, equal to, or a descendant of t. Ancestry is read
   through the in-process git object reader ([60] M4); an answer that cannot be computed within the command's budget makes
   T a reader for that command.

In every other case T is a reader tree, among them: a git tree whose binding has no expected ref (bound before the repository
existed, or while its HEAD was detached) and no matching base clause; a symbolic HEAD naming another branch; a symbolic HEAD
outside `refs/heads/`; a tree whose HEAD cannot be read. `doctor lanes` names the fix (`worktree bind DIR REF --replace`) for
the orchestrator and owner.

### 3.7 Where `BindingExt` is carried

This section owns the bytes of `BindingExt`; the other chapters embed it **verbatim**, 40 bytes at a fixed place, and hold
no field of their own for the designation, the expected ref or the base (pass 1, S1-10, A1-8, P1-2):

- **[F11] `HEADS`.** Every binding row carries exactly one `BindingExt` (at row offset 105, [F11 §5]), in place of any
  separate flags, base-commit or git-ref field. A row that is not a binding carries a zero one, which is valid and
  non-designated. The lookup of a binding by root id first ([OS/path §4.4]) uses the bound directory's root `OsFileId`,
  which the row stores as `root_id` ([F11 §5]; open point 5).
- **[F05] `ClientHead`.** The durable record that creates, changes or removes a binding carries the row's full new value
  as the image of the `HEADS` row it folds into ([F05 §9.3], [F11 §2.9]: [F11] owns the row, [F05] frames it), so
  `BindingExt` travels as the same 40 bytes and replay rebuilds `HEADS` exactly. A `--replace` writes both affected rows
  in one durable group.
- **[F12]/[AR §5a.4].** The client-key order is unchanged: a binding still resolves a command's branch by its longest bound
  prefix, designated or not.

## 4. R-16: the frozen strings

### 4.1 Rules for every string of this section

1. Each string is frozen at the bytes given here ([40] R-16, [AR §7.1] output contract). A later change is a format-version
   change ([F01 §9.1]).
2. Every fixed byte is ASCII; separators are ` | ` and arrows `->` ([AR §7.1], [90 §10.1]). Value bytes in slots keep their
   UTF-8 (§1.3).
3. The strings are byte-identical on every OS, except the two place strings of §4.9, which the golden harness replaces by
   `<OS-DETAIL>` ([OS/shell], [80 §4.2] T7).
4. The JSON forms carry the state strings of §4.2–§4.4 unchanged and the detail tokens of §4.6; [F19] and [API] name the
   members. The example of [40 §6.1] (`"detail":"similar"`) is illustrative: the token is `similarity`.
5. Text details are written for agents and people. Only the JSON form is a machine interface; no parser splits a text
   detail.

### 4.2 File-level states

The state of a file node in a tree, computed by the file cascade ([40 §4.3], [F20 §5]), except `deleted` and `planned`,
which follow from versioned status ([40 §2.9]).

| code | string | meaning |
|---|---|---|
| 1 | `ok` | the path is present in the tree |
| 2 | `moved-auto` | found elsewhere by exact evidence |
| 3 | `moved-needs-confirm` | one proposal: a strong, weak or identical-copy candidate |
| 4 | `ambiguous` | two or more candidates, or a collision or conflict that only a decision settles |
| 5 | `deleted` | the node has status `removed`, or the node is engine-deleted |
| 6 | `replaced` | the path is present, but the file there was re-created with unrelated content ([40 §4.4]) |
| 8 | `missing` | absent, with no candidate |
| 9 | `absent-in-tree` | this tree cannot contain the observation ([40 §5.2]) |
| 10 | `pending` | this tree still has an alias; the move happened on a line it has not received |
| 11 | `planned` | linked with `--planned`; not bound yet |
| 12 | `unverified` | not determinable now; one reason of §4.6 |

The status value `removed` (R-2) and the state string `deleted` are distinct: the first is versioned data, the second a
rendering.

### 4.3 Anchor-level states

The state of one anchor, computed by the anchor cascade ([40 §4.5], [F20 §6]) when its file state is `ok` or `moved-auto`.

| code | string | meaning |
|---|---|---|
| 1 | `fresh` | the span hash matches at the hint (for `header` watch, the header matches) |
| 2 | `moved` | identical text, unique, found elsewhere in the file |
| 3 | `edited` | a fuzzy match, a changed span under `span` watch, or only the scope survived |
| 4 | `ambiguous` | two or more candidates within the margin |
| 5 | `orphaned` | nothing found |
| 6 | `unverified` | the cascade could not decide: its content or line-hash array was unavailable ([F20 §1.5], [F20 §6.5]) |
| 7 | `unresolved` | LQ only, never stored: the cascade did not run (§4.4) |

Code 6 closes a gap: [AR §5e.3] and [F20 §6.5] yield `unverified (size)` for an anchor while [40 §2.9]'s anchor table has no
such row (open point 13).

### 4.4 Link states: what an agent sees

The **link state** of an anchor is its file state refined by its anchor state ([40 §2.9]):

| file state | anchor state | link state |
|---|---|---|
| `ok` or `moved-auto` | `fresh` or `moved`, or the anchor is a `file` anchor with `watch = header`, or the anchor is `pinned` | the file state |
| `ok` or `moved-auto` | `edited`, `ambiguous` or `orphaned` | `stale-anchor` (code 7), with the anchor state as its principal detail (§4.6) |
| `ok` or `moved-auto` | `unverified` | `unverified`, with the anchor's reason |
| any other | not computed | the file state |

- The anchor cascade does not run on `replaced` content ([40 §4.5]) nor for a `pinned` anchor (a historical citation, never
  re-resolved, [40 §2.7]).
- **LQ totals** ([50 §2.6], review S-05). `link_state(n)` of a node that is neither an `AT` edge variable nor an artifact and
  has no `AT` edge is `none` (state code 13, never stored). `a.state` is the anchor state of §4.3, or `unresolved` when the
  cascade did not run: the file state is not `ok` or `moved-auto`, the content is `replaced`, or the anchor is `pinned`
  (open point 13). W10 marks `<>` and `NOT IN` over the node form ([50 §5.2], [LQ/errors]).
- **No tree.** A tree-derived LQ built-in with no eligible tree is E302 with the `--tree` hint, never a value; `unverified (no
  tree)` is a rendering of packs, briefs and the `links check` verb only ([LQ/std §2.8] item 4, review S-07).

### 4.5 Severity order

Where one state summarises several — `link_state(n)` of a node over its anchors, the pack header, `brief` — the most severe
wins ([40 §2.9], [50 §2.6]):

| rank | state |
|---|---|
| 1 | `missing` |
| 2 | `replaced` |
| 3 | `deleted` |
| 4 | `ambiguous` |
| 5 | `moved-needs-confirm` |
| 6 | `stale-anchor` |
| 7 | `unverified` |
| 8 | `pending` |
| 9 | `planned` |
| 10 | `absent-in-tree` |
| 11 | `moved-auto` |
| 12 | `ok` |

`none` takes part in no summary: it is the value only when there is nothing to summarise.

### 4.6 The detail registry

Every detail has a `u8` code, an ASCII token (the JSON form and the key of the code), the states it qualifies, a **label**
(fixed bytes, used in the qualified form of §4.7; "—" means the detail is never principal) and a **text** template (used in
link lines, markers and `file where`). Values not listed are invalid ([F01 §5.4]). In the table, `\|` inside a code span is
the single byte `|` (Markdown escaping), so text 25 is `merge conflict: <path> | <path>` and text 57 is
`git: moirai links sync | moirai check`.

| code | token | states | label | text |
|---|---|---|---|---|
| 1 | `changed` | `ok` | — | `changed since <c8>` (the commit that last set the node's observation composite on the view) |
| 2 | `spelling` | `ok` | `spelling differs on disk` | `spelling differs on disk (<path>)` (the on-disk spelling, [F20 §3.6]) |
| 3 | `normalization` | `ok` | `normalization differs on disk` | `normalization differs on disk` |
| 4 | `body-changed` | `ok`, `moved-auto` | — | `body changed since capture` (a `header`-watched anchor whose body changed, [40 §4.5]) |
| 5 | `evidence` | `moved-auto` | — | `<ev>` (the exact evidence's token, §5.2) |
| 6 | `recorded` | `moved-auto` | `recorded` | `recorded <c8>` (only in the output of the settle that wrote the re-bind; the commit it wrote) |
| 7 | `not-recorded` | `moved-auto` | `not yet recorded` | `not yet recorded (moirai links sync)` |
| 8 | `reader-tree` | `moved-auto` | `reader tree: not recorded` | `reader tree: not recorded` |
| 9 | `uncommitted-main` | `moved-auto` | `uncommitted in the main tree: not recorded` | `uncommitted in the main tree: not recorded` |
| 10 | `identical-copy` | `moved-needs-confirm` | `identical copy` | `identical copy` |
| 11 | `file-id-edited` | `moved-needs-confirm` | `moved and edited in place` | `moved and edited in place` |
| 12 | `prefix-strong` | `moved-needs-confirm` | `inferred directory move` | `inferred directory move` |
| 13 | `git-pair` | `moved-needs-confirm` | `git rename` | `git rename <score>` |
| 14 | `edited+moved` | `moved-needs-confirm` | `edited+moved` | `edited+moved <score>` |
| 15 | `similarity` | `moved-needs-confirm` | `similar` | `similar <score>[, runner-up <score>]` |
| 16 | `weak` | `moved-needs-confirm` | `weak` | `weak <score>` |
| 17 | `split` | `moved-needs-confirm` | `split` | `split` |
| 18 | `merged` | `moved-needs-confirm` | `merged` | `merged <score>` |
| 19 | `argv` | `moved-needs-confirm` | `seen in a shell command` | `seen in a shell command` |
| 20 | `moved-differently` | `moved-needs-confirm` | `moved differently on this line` | `moved differently on this line` |
| 21 | `dir-replaced` | `moved-needs-confirm` | `directory moved, file replaced` | `directory moved, file replaced` |
| 22 | `candidates` | `ambiguous` | `candidates` | `<n> candidates` |
| 23 | `rename-over` | `ambiguous` | `rename-over` | `rename-over` |
| 24 | `swap` | `ambiguous` | `swap` | `swap` |
| 25 | `merge-conflict` | `ambiguous` | `merge conflict` | `merge conflict: <path> \| <path>` (the two sides' paths, [40 §5.3]) |
| 26 | `case-collision` | `ambiguous` | `case collision` | `case collision` |
| 27 | `normalization-collision` | `ambiguous` | `normalization collision` | `normalization collision` |
| 28 | `path-claim` | `ambiguous` | `PathClaim` | `PathClaim` |
| 29 | `path-reused` | `ambiguous` | `path reused` | `path reused; original at <path>` |
| 30 | `removed` | `deleted` | `removed` | `removed in <c8>` (the commit that set `removed`) |
| 31 | `node-deleted` | `deleted` | `node deleted` | `node deleted in <c8>` (the file node is engine-deleted, [40 §2.8]) |
| 32 | `reason` | `deleted` | — | `reason: <text>` |
| 33 | `replaced-by` | `deleted` | — | `replaced by #<n>` |
| 34 | `unrelated` | `replaced` | `unrelated content` | `re-created with unrelated content (containment <score>/<score>)` (old-in-new, then new-in-old) |
| 35 | `git-readded` | `replaced` | — | `deleted in git <g7>, re-added in <g7>` |
| 36 | `since` | `missing` | — | `since <age>` (from `FILEOBS.missing_since`) |
| 37 | `no-candidate` | `missing` | `no candidate` | `no candidate` |
| 38 | `outside-root` | `missing` | `moved outside the root` | `moved outside the root` |
| 39 | `ignored` | `missing` | `moved into ignored output` | `moved into ignored output` |
| 40 | `trash` | `missing` | per OS, §4.9 | per OS, §4.9 |
| 41 | `never-candidate` | `missing` | `moved to a temporary or backup name` | `moved to a temporary or backup name` |
| 42 | `cloud-target` | `missing` | `moved to a cloud-only file` | `moved to a cloud-only file` |
| 43 | `deleted-in-git` | `missing` | `deleted in git` | `deleted in git <g7>` |
| 44 | `not-representable` | `missing` | `not representable on this OS` | `not representable on this OS` (decided by `representable_here` ([OS/path §8.1]) before any OS call on the path, [F20]'s cascade testing it first: a Windows segment with `:`, or ending in `.` or a space, never reaches the OS; `--allow-nonportable` never overrides it; pass 1, P1-15) |
| 45 | `unrepresentable` | `missing` | `unrepresentable path` | `unrepresentable path` |
| 46 | `nothing-written` | `missing` | — | `nothing written` |
| 47 | `behind` | `absent-in-tree` | `behind` | `behind` |
| 48 | `diverged` | `absent-in-tree` | `diverged` | `diverged` |
| 49 | `observed-at` | `absent-in-tree` | — | `observed at git <g7>` (the node's `observed_git`) |
| 50 | `not-here-yet` | `pending` | — | `not in this tree yet` |
| 51 | `pending-age` | `pending` | — | `<age>` (from the `hlc` of the commit that last set the observation composite; printed only when it is at least `files.pending-escalate`) |
| 52 | `plan-predates` | `planned` | `file present, tree predates the plan` | `file present, tree predates the plan` ([F20 §5.18] refused the binding) |
| 53 | `budget` | `unverified` | `budget` | `budget` |
| 54 | `cloud-only` | `unverified` | `cloud-only` | `cloud-only` |
| 55 | `commit-not-here` | `unverified` | `commit not in this repository` | `commit not in this repository` |
| 56 | `no-tree` | `unverified` | `no tree` | `no tree` |
| 57 | `git` | `unverified` | `git` | `git: moirai links sync \| moirai check` ([AR §5e.3]) |
| 58 | `size` | `unverified` | `size` | `size` |
| 59 | `unreadable` | `unverified` | `unreadable` | `unreadable` |
| 60 | `unmapped-root` | `unverified` | `unmapped root` | `unmapped root` |
| 61 | `oid-algo` | `unverified` | `oid algorithm differs` | `oid algorithm differs` |
| 62 | `edited` | `stale-anchor` | `edited` | `edited[ <score>]` (the fuzzy score, when the fuzzy step decided) |
| 63 | `edited-scope` | `stale-anchor` | `edited` | `edited (scope only)` |
| 64 | `anchor-ambiguous` | `stale-anchor` | `ambiguous` | `ambiguous` |
| 65 | `orphaned` | `stale-anchor` | `orphaned` | `orphaned` |
| 66 | `was` | `stale-anchor` | — | `was: "<quote>"` (the captured quote; for a `range`, its start quote) |
| 67 | `text-unavailable` | `ok`, `moved-auto`, `stale-anchor`, `unverified` of an anchor | — | `text-unavailable` (the anchor was imported without its text, [40 §5.7]) |
| 68 | `accepted-guess` | every state except `deleted` and `planned` | — | `accepted guess <c8>` (the commit that set the current `relink` value) |
| 69 | `confirm` | with 68 | — | `confirm: moirai links fix <n> --confirm` |
| 70 | `glob-empty` | a glob field, not a link | `glob matches nothing` | `glob matches nothing` ([40 §2.4]) |

**The unverified reasons** (codes 53–61) are the closed set of [40 §2.9] — `budget`, `cloud-only`, `commit not in this
repository`, `no tree`, `git`, `size` — plus three this chapter adds: `unreadable` (a denial on the stat of p or on a content
read, [80 §2.11.4] rule 9, [F20 §4.8]), `unmapped root` (a named root with no `roots.<name>` mapping on this machine,
[40 §2.4], [F02 §7.3]) and `oid algorithm differs` (a `file` anchor with `span` watch whose `blob` has another algorithm than
the current content, [F20 §6.5]). [F20]'s internal reason `unstable` renders `budget` ([F20 §1.5]). The additions are
frozen-string additions for the owner's sign-off, which is owner question OQ-F-4 (`reviews/owner-questions.md`; open
point 12); they stand as written until it is answered.

**Intent-recovery strings.** `doctor` and `brief` render the outcome of an `FsIntent` recovery ([40 §3.4]) with these texts,
which are not link details:

| code | token | text |
|---|---|---|
| 71 | `changed-after-move` | `content changed after the move` |
| 72 | `intent-both` | `interrupted move: both paths present` |
| 73 | `intent-neither` | `interrupted move: neither path present` |

### 4.7 Composition

1. **Parts per state.** A link's detail is a sequence of parts, joined by ` | `, in this order (braces: exactly one of;
   brackets: optional; the numbers are the codes of §4.6):

| state | parts |
|---|---|
| `ok` | [1] [{2, 3}] [4] [67] [68, 69] |
| `moved-auto` | 5, {6, 7, 8, 9}, [4] [67] [68, 69] |
| `moved-needs-confirm` | {10–21} [68, 69] |
| `ambiguous` | {22–29} [68, 69] |
| `deleted` | {30, 31} [32] [33] |
| `replaced` | 34 [35] [68, 69] |
| `stale-anchor` | {62, 63, 64, 65} [{66, 67}] [68, 69] |
| `missing` | [36] {37–45} 46 [68, 69] |
| `absent-in-tree` | {47, 48} [49] [68, 69] |
| `pending` | 50 [51] [68, 69] |
| `planned` | [52] |
| `unverified` | {53–61} [67] [68, 69] |

   Parts 68 and 69 appear together iff the file node's `relink` is a guess (§5.6) and its status is not `removed`.
2. **Principal part.** The part drawn from the first braced set of the row, and for `ok` the part 2 or 3 when present. `ok`
   without 2 or 3, `deleted`, `replaced`, `pending` and `planned` without 52 have no principal part.
3. **Qualified form.** Wherever a state and its principal detail appear together as one token — headers, counts, markers,
   `FILEOBS` renderings, notices, prose — the only spelling is `<state> (<label>)`, one space before the parenthesis, or
   `<state>` alone when there is no principal part: `ambiguous (normalization collision)`,
   `missing (not representable on this OS)`, `ok (spelling differs on disk)`, `absent-in-tree (behind)`,
   `stale-anchor (edited)`, `unverified (git)`, `ambiguous (path reused)`. The spellings `absent-in-tree(behind)` and `stale-anchor(edited)` of [40 §3.8] and [AR §7.1]'s
   example are illustrative and not used.
4. **Link lines.** [LQ/envelope §5.13] lays out a link line; its `<detail>` field is the joined parts of rule 1.
5. **The quote in part 66.** The captured quote is escaped as [LQ/envelope §5.16] does; when the escaped value exceeds 24
   bytes it is cut at the last scalar-value boundary at or before byte 21 that does not split an escape, and `...` is
   appended inside the quotes. Part 66 is therefore at most 31 bytes (`was: "` + 24 + `"`). An anchor without its text shows
   part 67 instead (review A-m1; open point 17).

*(Informative)* Link lines with these rules, laid out as [LQ/envelope §5.13] says (handle, two spaces, the state padded to
20 bytes, two spaces, the path, two spaces, the detail):

```
#812  moved-auto            crates/engine/src/lock.rs -> crates/engine/src/sync/lock.rs  file-id | not yet recorded (moirai links sync)
#815  moved-needs-confirm   docs/plan/storage.md -> docs/plan/storage-v2.md  similar 0.81, runner-up 0.22 | verify #815
#820  missing               tests/quarantine_me.rs  since 3m | no candidate | nothing written
#826  replaced              docs/perf/findings.md  re-created with unrelated content (containment 0.04/0.07) | verify #826
a31  stale-anchor          crates/engine/src/log.rs::Log/append  edited 0.86 | was: "pub fn append(&mut se..." | verify a31
```

The anchor `a31` is of kind `symbol`. While [F20 §6.1]'s interim scanner rule holds, a store holds such an anchor only by
import: no capture makes a `symbol` or `heading` anchor ([F08 §10.3.1]), as [F14 §17.2] says of its own example (pass 1,
closure round 1, editorial residue).

### 4.8 Header strings

1. **No tree.** When the tree resolved for a file-bearing result is not eligible ([40 §5.1]), the header's `files` field is
   exactly `files: no tree bound` ([LQ/envelope §3.1]). The parenthesis `(moirai worktree bind DIR BRANCH)` of [40 §5.1] is not
   part of the header: binding is an orchestrator ritual (review A-m4), and `doctor lanes` names the command for the
   orchestrator and owner roles (open point 11).
2. **The reader note.** A file-bearing result read from a reader tree prints, on line 2 ([AR §7.1], [LQ/envelope §2.4]):

   `reading only: tree on <here>, branch expects <there>`

   with one spelling and these slot values:

| slot | value | when |
|---|---|---|
| `<here>` | `<ref>`: the branch T's HEAD names (short form, §3.2 rule 1) | T's HEAD is symbolic |
| | `detached <g7>` | T's HEAD is detached at a commit |
| | `no git` | T has no readable git HEAD |
| `<there>` | `<ref>`: the binding's expected ref | T is the designated tree of the result's branch but not on the line (§3.6) |
| | `tree <label>` | another tree is the designated tree of the result's branch |
| | `a bound tree` | the result's branch has no designated tree |
| | `a git line` | T is the designated tree, has git, and its binding has no expected ref |

   Each slot value is at most 20 bytes: a longer value is cut from the left, keeping its last bytes, to `...` followed by the
   longest suffix of at most 17 bytes that starts at a scalar-value boundary; in `tree <label>` the label alone is cut so, to
   at most 15 bytes. The fixed bytes are 39, so the note is at most 79 bytes, within the 80-byte limit of [AR §8.3].
3. The `files @ <label> (…)` header field and the pack header segment are [LQ/envelope §3.1]'s and [F19]'s; they use the state
   strings and qualified forms of this section.

### 4.9 Per-OS strings

The only strings that differ by OS are the two texts of detail 40 (`trash`), which a golden file carries as `<OS-DETAIL>`
([OS/shell]; [80 §2.11.1] row "Trash"; [F20 §4.6]):

| OS | label and text of detail 40 |
|---|---|
| Windows | `in the Recycle Bin` |
| Linux | `in the trash` |
| macOS | `in the trash` |

### 4.10 Codes in runtime rows

Runtime rows that store a state, a detail or a proposal class ([F11]: `FILEOBS.state` with its detail, the proposals'
evidence, `ANCHORRES.state`) use the `u8` codes of §4.2 (file states 1–6 and 8–12; the link-only codes 7 and 13 are never
stored), §4.3 (anchor states 1–5; 6 `unverified` and 7 `unresolved` are never stored: an undecided anchor leaves no
`ANCHORRES` row, since its reason is transient), §4.6 (details) and §5.2 (evidence tokens). Code 0 is invalid in every
such field; a row that has no detail has no detail field or marks it absent as [F11] specifies. Codes are never reused for
another meaning ([F01 §5.4]).

**One owner per code** (pass 1, S1-9, A1-7, P1-2). This section and §5.2 own the codes; [F11]'s rows and [F05]'s records
cite them verbatim and define no numbering of their own (`replaced` is 6 and `missing` 8 in every chapter). Which of
the codes a row records is the row's owner's: [F11 §12.5] records only the file states a settle decides (1–4, 6, 8
and 12) and computes 5, 9, 10 and 11 when a link is rendered.
A **proposal** stored in a runtime row ([F11 §12.5] `FILEOBS` proposals and the [F05] records that fold into them) is
the tuple below. A `PENDING` row ([F11 §12.6]) stores the same `class` and an `evidence` code, which may also be an
exact token (codes 1–12), since hooks record exact evidence there ([F20 §5.6]):

| part | encoding | meaning |
|---|---|---|
| `class` | `u8` | the evidence class of [F20 §1.5]: 1 `exact`, 2 `strong`, 3 `copy`, 4 `weak` (strongest first; `none` is never stored) |
| `evidence` | `u8` | a proposal-class token of §5.2, codes 13–26 |
| `path` | [F08 §5.2] `path` | the candidate path with its root |
| `score` | [F11]'s encoding | the exact rational score of [F20 §1.2]; §5.3 renders it with two decimals |

[F11] owns the row's bytes and [F05] frames them; this chapter owns the two codes. The `class` numbering follows [F20 §1.5]'s
order, which names the classes without numbers.

## 5. R-17: the `relink` provenance vocabulary

### 5.1 Grammar

`relink` is a text value of the observation composite ([40 §2.2], R-2). It is hashed inside canonical item 10 ([F07]), so its
bytes are frozen: a value is valid iff it matches `relink` below ([RFC 5234] with [RFC 7405]'s `%s`); every other byte string
is invalid, the empty string included.

```abnf
relink       = explicit / lazy / git / hook / journal / judged / merge-obs / merge-comp
explicit     = %s"explicit/" ( %s"intent-recovered" / %s"intent" )
lazy         = %s"lazy/" lazy-ev
git          = %s"git/" git-ev
hook         = %s"hook/" hook-ev
journal      = %s"journal/" ( %s"usn" / %s"fsevents" )          ; reserved; never written in format v1
judged       = judge "/" ( unscored / scored "/" score / %s"manual" / %s"replacement" )
merge-obs    = %s"merge-observation/" ( lazy-ev / git-ev / hook-ev )
merge-comp   = %s"merge-compose/prefix"
lazy-ev      = %s"file-id" / %s"dir-id" / %s"oid+ctime" / %s"prefix" / %s"pending"
git-ev       = %s"r100" / %s"case"
hook-ev      = %s"file-id" / %s"move"
judge        = %s"owner" / %s"agent" / %s"policy" / %s"confirmed"
unscored     = %s"identical-copy" / %s"file-id-edited" / %s"prefix-strong" / %s"split" / %s"argv"
             / %s"tie" / %s"rename-over" / %s"swap" / %s"path-reused"
scored       = %s"git-pair" / %s"edited+moved" / %s"similarity" / %s"weak" / %s"merged"
score        = "0." 2DIGIT / "1.00"
```

- A value is ASCII and at most 27 bytes (`merge-observation/oid+ctime`, `confirmed/edited+moved/0.81`).
- A score is present exactly when the evidence token is scored.
- The value is stored as a `text` or `sym` value ([F08 §5.1]; [F01 §8.2] class `text`), hashed as its bytes ([F07]) and written in the image
  as `field relink: <value>` ([F14], [40 §5.7]).

### 5.2 Evidence tokens

| code | token | scored | after `how` | meaning |
|---|---|---|---|---|
| 1 | `intent` | no | `explicit` | `file mv` recorded its own move (E1) |
| 2 | `intent-recovered` | no | `explicit` | the roll-forward of intent recovery ([40 §3.4] step 5) |
| 3 | `file-id` | no | `lazy`, `hook`, `merge-observation` | E3 (`lazy`), or a file-id chain an evidence hook captured (E1, `hook`) |
| 4 | `dir-id` | no | `lazy`, `merge-observation` | E3d |
| 5 | `oid+ctime` | no | `lazy`, `merge-observation` | the copy rule's creation-time line ([F20 §5.9] line 2) |
| 6 | `prefix` | no | `lazy`, `merge-observation`, `merge-compose` | E5 through a recorded `explicit`, `confirmed` or `committed` entry; merge composition |
| 7 | `pending` | no | `lazy`, `merge-observation` | a `PENDING` row promoted on exact evidence ([40 §5.3]) |
| 8 | `r100` | no | `git`, `merge-observation` | E6: an exact rename inside one commit |
| 9 | `case` | no | `git`, `merge-observation` | τ(H)'s spelling in a writer tree ([F20 §3.6]) |
| 10 | `move` | no | `hook`, `merge-observation` | a Codex `apply_patch` `*** Move to:` line (E1) |
| 11 | `usn` | no | `journal` | reserved (E2 is not built) |
| 12 | `fsevents` | no | `journal` | reserved (E2 is not built) |
| 13 | `identical-copy` | no | a judge | the copy rule's line 4 ([F20 §5.9]) |
| 14 | `file-id-edited` | no | a judge | E3: moved and edited in place |
| 15 | `prefix-strong` | no | a judge | E5 with changed content, an `observed` entry or sibling inference; also E3d's `directory moved, file replaced` (§5.4) |
| 16 | `git-pair` | yes | a judge | an E6 per-commit pair, score = git's similarity index ÷ 100; also an E6 chain from an alias only (§5.4) |
| 17 | `edited+moved` | yes | a judge | E8, score = the smaller of the two containments |
| 18 | `similarity` | yes | a judge | the similarity search's strong result, score = the stage-2 pair score |
| 19 | `weak` | yes | a judge | a weak result, score = its pair score |
| 20 | `split` | no | a judge | a split proposal |
| 21 | `merged` | yes | a judge | a merge into a host file, score = old-in-new containment |
| 22 | `argv` | no | a judge | E1: a hook's argument-parsed destination |
| 23 | `tie` | no | a judge | one of two or more exact candidates |
| 24 | `rename-over` | no | a judge | the rename-over reading |
| 25 | `swap` | no | a judge | the swap reading |
| 26 | `path-reused` | no | a judge | `ambiguous (path reused)` answered |
| 27 | `manual` | no | a judge | `links fix --to`, `file relink --after` |
| 28 | `replacement` | no | a judge | `links fix --accept-replacement` |

"A judge" means `owner`, `agent`, `policy` or `confirmed`. Tokens 13–26 are the **proposal classes**: each proposal a
resolution keeps ([F11] `FILEOBS` proposals) has exactly one, which becomes the evidence token when the proposal is accepted.

### 5.3 Scores

A score is the evidence's own exact rational score r, 0 ≤ r ≤ 1 ([F20 §1.2]), in two decimals: let k be the integer nearest
to 100 × r, ties to the even integer (half-even); the text is `0.` followed by k as two digits (with a leading `0` below 10)
when k < 100, and `1.00` when k = 100. A git similarity index is an integer percentage, so its score needs no rounding. The
score of a judged value is the proposal's score re-evaluated when the command runs ([40 §3.7] `--accept`).

### 5.4 Who writes which value

| Value | Written by |
|---|---|
| `explicit/intent` | `file mv`'s commit ([40 §3.4] step 4) |
| `explicit/intent-recovered` | the roll-forward of intent recovery, after the re-barrier ([40 §3.4] step 5) |
| `lazy/<lazy-ev>`, `git/<git-ev>`, `hook/<hook-ev>` | a settle's exact re-bind in a writer tree (I-F6), by the source that decided ([F20 §5.5]) |
| `policy/<class>[/<score>]` | a settle's automatic strong re-bind under `files.policy.auto = strong` ([40 §9.2] decision 1); the class is one of `file-id-edited`, `prefix-strong`, `git-pair`, `edited+moved`, `similarity`, `argv` |
| `owner/…`, `agent/…` | `links fix --accept --expect PATH` (the proposal's class and score), `links fix --to PATH` and `file relink --after` (`manual`), `links fix --accept-replacement` (`replacement`); `owner` when the actor's role is the owner, `agent` for every other role ([40 §3.6], [40 §3.7]) |
| `confirmed/…` | `links fix --confirm` (§5.5) |
| `merge-observation/<ev>` | a settle that resolves a composite conflict value by observation ([40 §5.3], [40 §5.5]) |
| `merge-compose/prefix` | merge composition through a `path_moves` entry ([40 §5.5], [RULES/link-merge-rules] CP-006) |
| `journal/…` | nothing in format v1 |

- **Two proposals without a class of their own** take one of the list, so that every proposal can be accepted without a new
  token: `moved-needs-confirm (moved differently on this line)` (an E6 chain that starts only at an alias, [F20 §5.11.3]) has
  class `git-pair` with the chain's lowest pair score (`1.00` for a chain of exact renames); `moved-needs-confirm (directory
  moved, file replaced)` ([F20 §5.7]) has class `prefix-strong` (open point 15). Neither is ever applied automatically, under
  either policy ([F20 §5.7]); their classes matter only for `--accept`.
- **Values valid but never written.** The grammar admits every judge with every judged token, as [40 §2.2]'s table does; a
  value such as `policy/manual` is valid on import but no command writes it.
- **Absent `relink`.** The registering `Create` (`link --at`, `file add`) and the `planned → present` binding set the path
  without a re-bind, and leave `relink` absent. `links fix --restore` restores the value before the node's last re-bind,
  which may be absent. An absent value is not the empty string (open point 16).

### 5.5 `--confirm`

`links fix ID --confirm` rewrites only the `how` of the node's current value ([40 §2.2]):

| current `how` | result |
|---|---|
| `agent` | `confirmed/` + the rest unchanged (`agent/similarity/0.81` → `confirmed/similarity/0.81`, `agent/manual` → `confirmed/manual`, `agent/replacement` → `confirmed/replacement`) |
| `policy` | `confirmed/` + the rest unchanged |
| `owner`, `confirmed`, any other, or absent | refused, exit 6: nothing to confirm ([F19] gives the text) |

The confirming actor must differ from the actor of the commit that set the current value, and its role must be in
`files.confirm-roles` (default `orchestrator,owner`; [40 §9.2] decision 16); otherwise the command is refused (exit 6).

### 5.6 Guesses

A value is a **guess** iff its `how` is `agent` or `policy`. `links_guesses` ([LQ/std §4.22], [50 §4.1]) lists exactly the file
nodes whose `relink` is a guess, and every link to such a node that is not `deleted` carries parts 68 and 69 (§4.7) until it
is confirmed ([40 §3.7], [40 §6.2]). `owner/*` and `confirmed/*` values are decisions, never guesses.

### 5.7 Validation

- **Write time.** A value outside the grammar is a defect: the writer refuses the commit (exit 1, [F19]).
- **Import.** A `relink` field whose value is outside the grammar is `ImageParse` ([40 §2.2], [F14]).
- **`doctor --verify`** checks every stored value against the grammar.

## 6. Where every other R-row is specified

| R-row | Reservation ([40 §2.11], authoritative) | Specified in |
|---|---|---|
| R-1 | value types `path`, `oid`, `pathmove` | [F08 §5] (value encodings; pass 1, P1-1); [F06 §5.5] (`pathmove.hlc` in commits); [F07] (canonical encoding, the root by name, review S-19); [F14] (text forms); the primitives: [F01 §6.2] (`vstr`), [F01 §7.5] (`algo`), [F01 §8.2] (the `root` class, `u16`) |
| R-2 | the `artifact` field set, statuses `planned`/`removed`, merge classes `observation` and `identity`, `area` fields `root` and `path_moves` | [F08]; merge semantics [RULES/link-merge-rules], [F12] |
| R-3 | `uid_derivation`; the length-prefixed derivations; predecessor order; the dead-uid rule; the merge re-key with edge re-pointing | [F08] (column, derivations, predecessor, dead uids); [F12] and [RULES/link-merge-rules] (re-key); [F01 §6.3] (`lp()`) |
| R-4 | edge kind `at`; the 128-bit discriminator; the anchor record with `captured` and `pred`; `SetEdgeProps`; the unhashed `aN` | [F06] (ops with `disc` and `aN`); [F08] (the `at` edge kind, the property block §10.2 and the anchor record §10.3); [F07] (the item-10 edge key) |
| R-5 | the root node | [F08] |
| R-6 | `HEAD.next_anchor` | [F04]; its recovery from the log [F05], [F16] |
| R-7 | record kinds `FsIntent`, `FsIntentDone`, `FsIntentAborted`, `FileObs`, `Pending`, `FPrint`, `JournalCursor`, `DirMap`, `TreeReg`, `PrefixEv`, `GitFacts`, `AnchorRes` | [F05] |
| R-8 | sections `PATHIDX`, `ALIASIDX`, `ANCHORS`, `ANCHOR_UID`, `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `TREES`, `PREFIXEV`, `GITRENAMES`, `ANCHORRES`, `GLOBIDX` | [F09] |
| R-9 | the fingerprint blob class in `blobs.<n>` | [F10]; the file [F02 §5.1]; the fingerprint value's bytes [F20 §2.6.4] |
| R-10 | the anchor selector block with `quote_h`, `prefix_h`, `suffix_h`, `end_h` | [F07]; the window value [F20 §2.7.3] |
| R-11 | the `.moi` anchor lines, artifact fields, `pathmove` blocks, the `text-unavailable` sub-state | [F14]; the string `text-unavailable` §4.6 |
| R-12 | I-F1…I-F14 | this chapter §2; [F13 §3.9] |
| R-13 | `roots.<name>`, `files.*`, `image.dest.<name>.anchor-text` | [CFG]; the user-scope file [F02 §7] |
| R-14 | the resolver-version constant table | [F20] |
| R-15 | the binding-row extension; I-F12 | this chapter §3; the row [F11], the record [F05] |
| R-16 | the frozen state, detail and header strings | this chapter §4; rendering [F19], [LQ/envelope §5.13] |
| R-17 | the `relink` vocabulary | this chapter §5; its encoding [F08 §5], hashing [F07], image [F14] |
| R-18 | the `FILEOBS` row layout | [F11]; `OsFileId`, `FsTime`, `FileAttrs` [OS/project]; the meaning of the compared fields [F20 §5.2–§5.4] |

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [40] R-12; [60 §2.5] R4 row R-12 | complete: the statements of I-F1…I-F14 with their precision; enforcement points, model functions and gates are [F13 §3.9]'s | §2 |
| [40] R-15; [60 §2.5] R4 row R-15 | complete: `BindingExt` (40 B), designation, the relation D and I-F12's checks, the writer-tree predicate; the `HEADS` row that embeds it is [F11]'s, the record [F05]'s | §3 |
| [40] R-16; [60 §2.5] R4 row R-16 | complete: file, anchor and link states with codes; `none`, `unresolved`; the severity order; the closed detail registry with labels, texts and codes (incl. `replaced`, the closed `unverified` set, `spelling differs on disk`, `normalization differs on disk`, `ambiguous (normalization collision)`, `unrepresentable path`, `missing (not representable on this OS)`, `ambiguous (path reused)`, `text-unavailable`); composition and the qualified form; `files: no tree bound` and the reader note; the per-OS strings. Layout of lines and headers is [LQ/envelope]'s and [F19]'s | §4 |
| [40] R-17; [60 §2.5] R4 row R-17 | complete: grammar, evidence tokens with codes, scores, writers, `--confirm`, guesses (`agent/*` and `policy/*` distinct from `owner/*` and `confirmed/*`), validation | §5 |
| [40] R-11 | the string `text-unavailable` only; the `.moi` grammar is [F14]'s | §4.6 |
| [60 §2.5] audit row "Resolver constants (R-14)" | the string `ambiguous (path reused)` only; the rule is [F20 §5.4.2]'s | §4.6 |
| [80] X-F7 | the strings that P4 and P5 render (`unrepresentable path`, `missing (not representable on this OS)`); the rules are [OS/path]'s | §4.6 |
| [80] X-F8 | the strings of [80 §2.11.4] rule 2 (spelling, normalization, collisions) and the trash places of rule 5; the layouts are [F11]'s, the rules [F20]'s | §4.6, §4.9 |
| [80] X-F12 | the R-16 strings are ASCII templates, byte-identical on every OS except `<OS-DETAIL>`; T1–T10 are [OS/shell]'s | §4.1, §4.9 |
| [60 §2.5] Cross-platform row | the R4 strings of X-F7 and X-F8 above | §4.6, §4.9 |
| [90 §10.1] "Output contract" | the ASCII rule and `...` truncation for the R-16 strings; byte units, both-ends and `--ids` rules are [F19]'s | §4.1, §4.7, §4.8 |

## Holes

None. No value of this chapter is decided by an M0 measurement or benchmark: the strings, codes, grammar and binding layout
are fixed here, and the thresholds they mention are [F20]'s holes.

## Open points for the review

1. **[PLAN §3.3] gap closed: R-12, R-15, R-16 and R-17 had no chapter.** This chapter holds all four (§2–§5) and indexes the
   other fourteen rows (§6).
2. **Conflict: the home of R-1's value encodings.** [F01]'s Coverage row for R-1 and [F20]'s "Depends on" place the value
   layouts (and [F20] the anchor record) in [F08]; [PLAN §3.2] WP-12 puts "the closed value set including R-1" in [F06], and
   this chapter's index follows [PLAN]. Proposal: [F06] owns the value encodings and the anchor record as edge props, [F08]
   the schema, kinds and derivations; [F01] and [F20] correct their citations at pass 1. Both chapters are R-SPEC-F's.
   **Pass 1 (P1-1, S1-1, S1-3): decided the other way.** [F08 §5] owns every value's bytes and [F08 §10.3] the anchor
   record; [F06] cites them. §6's index is corrected; [F01] and [F20]'s citations were already right.
3. **R-15 layout** (§3.2). [40 §2.6] gives the fields but no bytes. Resolution: 40 bytes — a flag byte, the `algo` byte, 2
   reserved bytes, the expected ref as a `git-branch` symbol (`u32`, [F01 §8.2]) and the base in [F01 §7.5]'s 32-byte slot —
   which gives [40 §2.6]'s ≈ 60 B per binding with the key and `ref_id`. A symbol keeps the row fixed-size; the ref name is
   store-local runtime data, so a symbol is allowed (bindings are never exported).
4. **Short form of the expected ref** (§3.2 rule 1): the bytes after `refs/heads/`. [F06] (WP-12) is asked to store the commit
   header's `git.branch` in the same form, since both use the `git-branch` class; otherwise the two would intern different
   spellings of one branch.
5. **For WP-13: lookup of a binding by root id** (§3.7). [OS/path §4.4] looks bindings up by the root `OsFileId` first, but
   [40 §2.6]'s binding row has no id. [F11] either adds the 57-byte root `OsFileId` to the binding row or requires the binding
   verbs to write the tree's `TREES` registration durably; this chapter needs one of the two for I-F12's "two spellings, one
   tree". **Closed** (pass 1, round 1): [F11 §5] stores the 57-byte `root_id` in every directory row, and its rules refuse a
   second binding of the same object.
6. **Nothing but a binding verb changes `expected_ref` and `base`** (§3.5). [AR §5c]'s `post-checkout` "binding refresh" is
   read as refreshing displayed provenance only; a hook that followed `git switch` would recreate [41 M2]. The review should
   confirm the reading and [AR §5c] should say so at WP-81a.
7. **`checkout` on a designated directory** (§3.5). [AR §5a.4]'s `checkout` writes a `ClientHead` for the resolved key, which
   can be a binding's directory key. Resolution: a write that changes a binding row's ref clears its designation and names the
   cleared designation in its result; only the binding verbs designate. Alternative: refuse `checkout` on a designated key
   unless `--replace`; rejected because `checkout` must stay usable in a lane tree.
8. **The designation relation with `files.main-tree`** (§3.4). [40 §5.1] resolves `main`'s tree from a binding first and from
   `files.main-tree` second; [40 §5.3] lets `files.main-tree` designate `main`. Resolution: a designated binding row for
   `main` wins; the configuration pair joins D only when it keeps D one-to-one; `doctor lanes` warns otherwise. For WP-18:
   [40 §5.1] says an unset `files.main-tree` means "the main worktree", [AR §13] gives the default "the directory where `init`
   ran"; [CFG] settles the default, which §3.4 only reads.
9. **`--replace` in both directions** (§3.5). [40 §5.3] names `--replace` only for "a second tree for B". This chapter also
   requires it to move a tree designated for one branch to another (check 2), so that no lane's tree loses its designation
   silently (X5).
10. **Bindings without a git line** (§3.6). A designated git tree whose binding has no expected ref (bound before `git init`,
    or while detached) is a reader until re-bound: without an expected ref, a `git switch` could not be detected. `main`'s
    configuration pair has no base, so a detached `main` tree is a reader. Both are conservative readings of [40 §5.3].
11. **`files: no tree bound` without the bind hint** (§4.8). [40 §5.1] prints `(moirai worktree bind DIR BRANCH)` in the
    header. Following review A-m4 (binding is an orchestrator ritual), the frozen header string omits it and `doctor lanes`
    names the command for the orchestrator and owner.
12. **Three additions to the closed `unverified` set** (§4.6): `unreadable` ([F20] open point 22, [80 §2.11.4] rule 9),
    `unmapped root` ([40 §2.4] and [F02 §7.3] name this rendering, which [40 §2.9]'s closed set lacks) and `oid algorithm
    differs` ([F20 §6.5]). Each is a frozen-string addition, like `none` and `unresolved` at the A1 re-review, and needs the
    owner's sign-off. Alternative if the owner declines: `unmapped root` renders `no tree`, and the other two render `budget`,
    which [F20] open point 22 shows to be misleading. **Spec sync 2a:** the three additions were not among the owner
    questions decided on 2026-09-28, so they are raised as owner question OQ-F-4 (`reviews/owner-questions.md`); this
    point stays open until it is answered.
13. **Anchor state `unverified` and a total `unresolved`** (§4.3, §4.4). [AR §5e.3] and [F20 §6.5] produce `unverified (size)`
    for anchors; [40 §2.9]'s anchor table and [50 §2.6]'s `a.state` list lack it. This chapter adds anchor code 6 and extends
    `unresolved` to `replaced` content and `pinned` anchors, where the cascade also does not run, so that `a.state` is total.
    [LQ/std] (WP-19) takes both.
14. **The place details are closed** (§4.6, codes 37–45; [F20] open point 22): `no candidate`, `moved outside the root`,
    `moved into ignored output`, the per-OS trash places, `moved to a temporary or backup name` (a never-candidate location,
    [F20 §4.7.3]), `moved to a cloud-only file`, `deleted in git <g7>`, `not representable on this OS` and `unrepresentable
    path` (an E6 chain that ends at a name P4 refuses). [40]'s wordings "moved to <place>", "in Recycle Bin" and "moved
    outside root" are unified into these.
15. **Classes for two class-less proposals** (§5.4): `moved differently on this line` → `git-pair` with the chain's lowest
    score; `directory moved, file replaced` → `prefix-strong`. This keeps [40 §2.2]'s closed vocabulary unchanged
    (review A-M1) while every proposal stays acceptable by `--accept`.
16. **Absent `relink`** (§5.4). [40 §2.2] does not say what a newly registered node carries. Resolution: absent until the first
    re-bind; the empty string is invalid. [F08] marks `relink` optional; [F14] omits the line when absent.
17. **Review A-m1: marker lengths** (§4.7 rule 5). The `was:` quote is cut to 24 bytes with `...`, so the stale-anchor detail
    is ≤ 31 bytes beyond its sub-reason. The markers themselves are [F19]'s ([RULES/pack-classes] RN-008). **Pass 1 (A1-44,
    A1-29): closed.** [F19 §4.6] owns the marker shapes, every one ≤ 50 B; this chapter's earlier proposal is superseded.
18. **Ages** (§1.3). [AR §7.1]'s example `since 3 min` becomes `since 3m`, the unit form [LQ/envelope §3.2] already freezes
    for the dirty row's age, so one age form exists.
19. **One qualified spelling** (§4.7 rule 3): `<state> (<label>)` with a space. [40 §3.8]'s and [AR §7.1]'s
    `absent-in-tree(behind)` and `stale-anchor(edited)` are treated as illustrative, following review A-m3.
20. **The reader note's slot values** (§4.8). [AR §7.1] and review A-M3 require one spelling. The one template covers every
    reader case through its slot values (`detached <g7>`, `no git`, `tree <label>`, `a bound tree`, `a git line`); without
    them a second, non-designated tree on the expected git branch would print "tree on u/l5np, branch expects u/l5np".
21. **For WP-16: [F13 §3.9]'s statements** (§2.15) predate reviews S-01/S-03 (I-F2's `pred`), S-14 (I-F14's history doors),
    S-19 (I-F8's root clause) and this chapter's I-F7 addition. Proposal: [F13] cites §2 for the wording. **Closed** (pass 1,
    round 1, A1-44): [F13 §3.9] names §2.1–§2.14 as the statements of record and lets this chapter win.
22. **I-F7 and `links fix --split`** (§2.7). [40 §3.7] makes the split's old node `removed{reason: split}`, but [40 §2.10]'s
    I-F7 list omits it. It is an explicit door, so adding it keeps the invariant's intent; [40 §2.10] should list it at
    WP-81a.
23. **I-F9 and empty windows** (§2.9). [F20 §6.1] makes a span of trivial lines a `lines` anchor, whose window can be empty
    in a file with no other non-trivial line; such an anchor would be a bare line number. Resolution: that capture is refused
    ([F19] gives the code; exit 2 proposed, like other capture refusals).
24. **I-F5's list** (§2.5) adds `links mentions` ([40 §3.7]: "never") and `file where` (a read by [40 §4.1] P1), and states
    that the LQ link built-ins are part of the read that evaluates them.
25. **I-F10's "visible" version bump** (§2.10). [F20 §1.3] delegates "every rendered link result carries the version" to this
    chapter. Resolution: the JSON link object carries it; text lines do not repeat it, and a notice marks the first result
    after a resolver-version change ([LQ/errors] numbers the notice; WP-19).
26. **I-F8's root clause extended** (§2.8). Review S-19 asks for "an artifact's `path` root must equal its `root` field". This
    chapter applies the same clause to `origin_path`, every alias and the `path_moves` entries, which carry roots the same
    way, and states that `abs` root nodes have no `path_moves`.
27. **Review A1P-14 (the dead-uid check's cost; owner WP-14, chapter 18).** Since review S-01, [40 §2.3]'s dead-uid rule reads
    the registering view only, not every branch head, so the check costs one `UIDX` probe and one view lookup per
    re-derivation step, with no per-ref probes; the finding's cost concern is gone. [AR §5e.2] still describes the superseded
    predecessor order "(generation, commit id)"; [40 §2.3] wins, and [AR §5e.2] is corrected at WP-81a. The derivation itself
    is [F08]'s.
28. **For [OS/project] open point 16 (WP-14): a missing destination parent of `file mv`.** [40 §3.4] does not create
    directories. Resolution: `file mv` refuses a destination whose parent directory does not exist, in its read-only plan step
    (exit 3, naming the directory; [F19] owns the text), so `ProjectFs` needs no directory creation.
29. **Codes for runtime rows** (§4.10, §5.2). [40] fixes strings but no numbers; the `u8` codes here let [F11] (WP-13) store
    `FILEOBS` states, details and proposal classes and `ANCHORRES` states without a second numbering. WP-13 adopts them or
    records why not. **Closed** (pass 1, round 1, S1-9, A1-7): [F11 §12.5], §12.6 and §12.13 cite §4.2, §4.3, §4.6,
    §4.10 and §5.2 and number nothing of their own; a `PENDING` row may hold an exact evidence token (§4.10).
30. **An engine-deleted file node renders `deleted`** (§4.2, detail 31 `node deleted in <c8>`). [40 §2.9] defines `deleted`
    as status `removed`, and [40 §2.8] only says that the `at` edge becomes a tombstone reference and the source `suspect`
    when the file node itself is deleted. Resolution: the link keeps a state, `deleted`, with its own principal detail, so
    the two cases stay distinguishable.
31. **Refusal codes this chapter proposes for [F19]**: I-F12 violations exit 5 ([40 §5.3]); an invalid `--base` or a non-UTF-8
    branch name exit 2; `--confirm` with nothing to confirm, or by the acceptor or a role outside `files.confirm-roles`,
    exit 6 ([40 §3.1]); an I-F1 violation at write time exit 6 (precondition); an invalid `relink` at write time exit 1.
32. **Pass 1, round 1** (S1-9, A1-7, S1-10, A1-8, P1-2, A1-44). The other chapters now cite this chapter's bytes and codes:
    [F11 §5] embeds `BindingExt` at row offset 105 and stores the directory's `root_id` (open point 5 closed), [F05 §9.3]
    carries the `HEADS` row image, [F11 §12.5], §12.6 and §12.13 store the §4.10 codes (open point 29 closed; §4.10 now
    says which codes a `FILEOBS` row records and that a `PENDING` row may hold an exact token, R-SPEC-R's alignment, kept),
    and [F13 §3.9] takes §2 as the statements of record (open point 21 closed). §3.2's example is the byte fixture of
    `COVERAGE.md` row R-15. The reviewers' preference for the full git-ref spelling stays declined: besides §3.2 rule 1's
    reason, the design writes the expected ref in the short form: [40 §5.3] item 2 equates it with `lane.git_branch`, and
    [40 §5.4] item 1 binds a tree "with expected ref `u/l5np`".
