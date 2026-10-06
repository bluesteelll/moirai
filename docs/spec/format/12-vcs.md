# 12 — Version control

| | |
|---|---|
| Title | Version control: ref names and the ref-name rule (X-F9), the ref namespaces `main`, `lane/*`, `plan/*`, `merge/*`, `import/*`, `orphans/*` and `tags/*`, the revision syntax and its resolution, commit ancestry and the single base-selection rule, the recursive-virtual-base addendum to [AR §5a.7], the conflict-class enumeration and conflicts as data, the conflict-key text form, the typed three-way merge contract at the byte level it needs (value equality, order keys, the line diff and diff3, the re-key, op emission, violation keys), the records every version-control operation writes, and staging refs |
| Chapter | [F12], `docs/spec/format/12-vcs.md` |
| Status | draft, pass 1 pending |
| Work package | WP-12c, the version-control part of WP-12 ([PLAN §3.2] item 1), author role R-SPEC-F |
| Sources | [AR §5a.1] (object model: ref names, kinds, reserved segments, `gen`); [AR §5a.2] (`ref_old` and the CAS, `RefUpdate` only for non-commit moves, `ref_seq` never reused, reflog); [AR §5a.3] (fork, `sync` residue, the fold of a side since the LCA, CM9); [AR §5a.4] (client resolution, `checkout`, detached heads); [AR §5a.5] (`undo`, `op restore`, `revert`, `cherry-pick`, `NotFound`/`DATA`, `--mainline 1`); [AR §5a.6] (`log`, `diff A..B`, `diff A...B`, as-of, the revision forms); [AR §5a.7] steps 0–8 and the typed-rule table; [AR §5a.8] (the conflict and violation taxonomy, conflict rendering); [AR §5a.9] (tags, branch delete); [AR §2.7] T7; [AR §3.4] I12, I25′, I26′, I27′, I31′, I33′, I34′, I37′, I41′; [AR §4.3] (commit header); [AR §4.6] items 2, 3 and 10, "Not hashed", "Net changeset = state diff"; [AR §5b.2] rule 3 (`conflict` lines) and rule 8; [AR §5b.4] (ref mapping to git; staging refs never exported); [AR §5b.6] steps 3–4 (foreign merges, import staging); [AR §7.1] (the VCS verbs; exit codes); [60 §3.1] item 1 ("the recursive-virtual-base addendum … is part of this specification") and its exit criterion "the recursive-virtual-base cases (both sides equal → clean)"; [60 §3.4] ("The recursive virtual base"; GT6 I31′ properties); [60 §3.13] GT6; [60 §4.2] (the model's LCA and virtual base by definition); [60 §4.4] item 5; [80 §2.10] P5, P6, P11 (b); [80 §3.1] X-F9; [80 §3.2] (refs row); [50 §2.2] rule 6 (as amended after the A1 re-review, S-11); [50 §2.3] (`revspec`, `rev_arg`, `ref_name`); [50 §2.4] (revision meaning); [50 §3.9] items 4, 6, 7 and 9; [50 §3.10] item 6 (`RESOLVE`); [50 §4.4] "Versioning and merge" (S-10); [40 §2.11] R-2, R-3; [40 §5.5] (link merge rules, the edge-complete re-key); [40 §8.3.2] P8; [90 §4.1] (the Branch row that resolves `HEAD`); [72 M5] (re-keys in a sync's residue); `docs/spec/reviews/a1-S.md` S-01, S-10, S-11, S-15; `docs/spec/reviews/a1-dispositions.md` §2 FB-3 and §5 (the obligations of chapter 12); [RULES/merge-table] (every table; open points 5, 9, 13, 14, 18, 19, 20); [RULES/link-merge-rules] PC-002, RK-001–RK-010; [F05 §9.2], [F05 §9.10]; [F06 §3], [F06 §4], [F06 §6], [F06 §7.7]–[F06 §7.9]; [F08 §8.2], [F08 §8.5], [F08 §11.2]; [F11 §3], [F11 §10]; [F13 §4], [F13 §5]; [F19 §10], [F19 §12]; [LQ/lexical §4], [LQ/lexical §7]; [LQ/errors] E301, E305; [LQ/envelope §5.10]; [PLAN §3.2] WP-12, [PLAN §3.3] |
| Depends on | [F01], [F05], [F06], [F08]; cites [F02], [F04], [F07], [F09], [F10], [F11], [F13], [F14], [F16], [F17], [F18], [F19], [F20], [LQ/lexical], [LQ/errors], [LQ/envelope], [LQ/std], [LQ/canonical-ast], [API], [CFG], [RULES/merge-table], [RULES/link-merge-rules] |

## 1. Scope

### 1.1 What this chapter owns

- The ref-name grammar of the store, the namespaces, the names the store may hold, and the rules on user-chosen names,
  including [80] X-F9's ref-name rule (§2).
- The text grammar of revisions and ranges, and how a revision resolves to a ref, a commit or the empty revision (§3).
- The commit DAG as the merge sees it, the LCA set, its order, and the single base-selection rule of I31′ (§4).
- The recursive-virtual-base addendum to [AR §5a.7] that [60 §3.4] requires: construction, the virtual merge, keys whose
  base holds a conflict value, termination, determinism and the cases the model must pass (§5).
- The value-conflict classes (codes 1–63 of the one class code space of [F19 §12.1]), where each class sits, the provisional
  value of a conflicted key, flat sides, resolution, and the text form of conflict and violation keys (§6).
- The typed three-way merge contract at the byte level: inputs, key identity, value equality, the order keys the rules use,
  the line diff and diff3 of the text rule, the re-key of R-3, the directional rules, how results become ops, and the key of
  every violation (§7).
- The records each version-control operation writes and the values of their version-control fields (§8).
- Staging refs: creation, content, `resolve`, `merge --continue` and `merge --abort` (§9).

This chapter defines no fixed-size structure, so it has no offset table. Its byte-level content is one enumeration (§6.1),
the grammars of §2.1, §2.3, §3.1 and §6.6, the values that version-control operations write into fields other chapters lay
out (§8, §9), one proposed field (`prov`, §6.3) and the functions whose results enter commit ids (§5, §7).

### 1.2 What it does not own

The bytes of `RefUpdate`, `RefTable` and `ClientHead` records and the ref-kind codes ([F05 §9.2], [F05 §9.3], [F05 §9.10]);
the commit record, `cstate`, the `Conflict`, `Violation` and `Resolve` ops ([F06]); canonical encodings and `commit_id`
([F07]); the schema, its merge-class column and the uid derivations ([F08]); the `REFS` and `CONFLICTS` rows ([F11]); the
invariant list, I26′ and the validator order ([F13]); the image, its refs and `conflict` lines ([F14]); the write protocol
and recovery ([F16]); the reflog window ([F17 §11.2]); exit codes, error texts and the structural and hint class codes 64–191
([F19]); LQ lexing inside query text ([LQ/lexical §7]) and LQ diagnostics ([LQ/errors]); the semantics of every typed merge
row ([RULES/merge-table], [RULES/link-merge-rules]). Where a rule here needs a change in one of those, the change is an open
point addressed to its owner.

### 1.3 Terms

| Term | Meaning |
|---|---|
| **commit** | a `Commit` record ([F06]); its **id** is its 32-byte `commit_id` ([F07]); `id16` is its first 16 bytes |
| **ε** (the empty revision) | the state before any commit: the core schema of version 1 ([F08 §9]), no node, no edge, no schema item, no conflict value. It has no id and no ancestors |
| **state(c)** | the state at commit c ([AR §4.6]: the fold of net changesets along its history); state(ε) is the empty state |
| **ref** | a row of the ref table ([F11 §3]) with a name, a never-reused `ref_id` and a kind; **live** when its `deleted` flag is clear |
| **tip(R)** | the commit ref R points at, or ε when R has no commit ([F11] open point 35) |
| **branch** | a live ref of kind `work` or `plan` |
| **staging ref** | a ref of kind `merge` (§9); an `import/*` ref holding a staged import is treated alike (§9.6) |
| **dst, src** | the ref a merge lands on and the ref it merges from; **o** (ours) is a key's value in state(tip dst), **t** (theirs) in state(tip src), **b** in the base state ([RULES/merge-table §2]) |
| **key** | a key of canonical item 10 ([AR §4.6]); in store-local form a `ckey` ([F06 §6.1]) |
| **value** | a key's value: a plain value (a `kval`, [F06 §6.2]) or a **conflict value** `{class, base, ours, theirs}` (a `cstate` with `cs` = 1). `absent` is a value |
| **user part** | the part of a ref name a person or agent chooses: `<u>` in `lane/<u>`, `plan/<u>`, `tags/<u>` |

## 2. Ref names

### 2.1 Grammar of a ref name

The ref-name grammar of the store is LQ's `ref_name` ([50 §2.2] rule 6 as amended after the A1 re-review, S-11; [50 §2.3]).
It is the grammar of every ref name the store holds, every ref-name argument, every revision base ([LQ/lexical §7.2]) and
every `ref`-typed configuration value ([CFG]). ([RFC 5234] ABNF with [RFC 7405]'s `%s`.)

```abnf
ref-name   = ref-seg *( "/" ref-seg )
ref-seg    = ref-word *( "." ref-word )        ; no "..", no leading or trailing "."
ref-word   = ref-first *ref-rest
ref-first  = LOWER / DIGIT / "_"
ref-rest   = LOWER / DIGIT / "_" / "-"
LOWER      = %x61-7A                            ; a-z
```

- A **segment** is the text between two `/` (or the name's ends). A ref name has no empty segment, no leading or trailing
  `/`, no upper-case letter, no byte outside `a-z 0-9 _ - . /`, and never contains `..`, `@`, `{`, `~`, `^`, `:`, `?`, `*`,
  `[`, `\`, a space or a control byte. Every ref name therefore passes git's ref-name rules on dots, `@{` and forbidden
  characters (git-check-ref-format, which [80 §2.10] P11 cites) and is a sequence of names every supported OS can hold.
- A ref name is ASCII, so NFC normalisation leaves it unchanged, and `fold_v1` ([F20]) is the identity on it: two ref names
  are equal under `fold_v1` exactly when their bytes are equal ([80] X-F9, P6; [LQ/lexical] L-10).

### 2.2 Namespaces and kinds

The first segment of a ref name fixes its namespace, and the namespace fixes the ref's kind ([AR §5a.1]: the `ref` row
and "Branch kinds and write masks"). The kind codes are [F05 §9.10]'s `rkind` (and [F11 §3.2]'s `kind`); this table maps names to them.

| Namespace | Name form | Kind (`rkind`) | Created by | Commits land by | Merge dst | Merge src | Exported to git ([AR §5b.4], [F14]) |
|---|---|---|---|---|---|---|---|
| `main` | exactly `main` | 1 `work` | `init` only ([F16]) | every write, merge, sync, revert, cherry-pick, import | yes | yes | yes |
| lanes | `lane/<u>` | 1 `work` | `branch`, `checkout --branch-new`, `lane open` ([API]) | as `main` | yes | yes | yes |
| plans | `plan/<u>` | 2 `plan` | `branch --kind plan` | writes under the I33′ mask; merges (V12 of [F13 §5]) | yes | yes | yes, when selected |
| tags | `tags/<u>` | 5 `tag` | `tag` | never: a tag never moves after its creation | no | yes | yes |
| staging | `merge/<dst>/from/<src>` | 3 `merge` | the merge machinery (§9.2) | staged commits and `Resolve` commits only (§9) | no | no | never |
| imports | `import/<ref>` | 4 `import` | `image import` ([F14], [AR §5b.6] step 4) | imported commits and `Resolve` commits only | no | yes | never |
| orphans | `orphans/<ref>` | 6 `orphans` | only the park of [F16] P-70: the first appender whose scan meets a commit whose ref CAS failed parks it here (I27′) | none: parking moves the ref without a commit of its own (§8.2) | no | yes, when `<ref>` is a branch | never |

- A ref's kind is fixed for its life and equals the kind of its namespace (V for the format oracle: a `REFS` row or
  `RefEntry` whose `kind` differs from its name's namespace is invalid).
- Kind 6 `orphans` ([F11] open point 7) is confirmed: none of the five kinds of [AR §5a.1] fits a ref that only the park
  writes, and [AR §5a.1] names `orphans/<n>` refs.

### 2.3 The names the store holds

Every ref name in the store is a `store-ref`:

```abnf
store-ref     = branch-name / tag-name / staging-name / import-name / orphans-name
branch-name   = %s"main" / %s"lane/" user-part / %s"plan/" user-part
tag-name      = %s"tags/" user-part
staging-name  = %s"merge/" branch-name %s"/from/" source-name
source-name   = branch-name / tag-name / import-name / %s"orphans/" branch-name / commit-name
commit-name   = %s"c" 64LHEX                     ; the full id of a reverted or cherry-picked commit
import-name   = %s"import/" ( branch-name / tag-name )
orphans-name  = %s"orphans/" ( branch-name / staging-name / import-name )
user-part     = ref-name                         ; rules RN-2 to RN-6 of §2.4
LHEX          = DIGIT / %x61-66                  ; 0-9 a-f
```

- A `staging-name` parses in one way: a user part never has a segment `from` (RN-2), and a `branch-name` never contains
  one, so the first segment `from` after `merge/` separates dst from src.
- A `staging-name`'s dst is always a branch; a revert or cherry-pick stages with the reverted or picked commit as its
  source, spelled `c` + 64 lower-case hex digits ([AR §5a.5]: `merge/<R>/from/<commit>`).
- The longest possible name is 284 bytes (`orphans/` + a staging name of `merge/`, a 128-byte branch name, `/from/` and a
  136-byte orphans source), so every ref name fits the `ref` symbol class ([F01 §8.2]) and a `REFS` heap slice ([F11 §3.1]).

### 2.4 Rules on user-chosen names

A verb that creates a branch or a tag checks the complete new name, prefix included, against these rules in this order and
refuses the first one that fails, writing nothing (exit 2; codes in open point 16). RN-2 to RN-6 apply to the user part
only: the fixed words of §2.3 (`main`, `lane`, `plan`, `tags`, `merge`, `from`, `import`, `orphans`) and machine-built
`commit-name` segments are exempt.

| # | Rule | Source |
|---|---|---|
| RN-1 | The name is a `branch-name` (for `branch`, `checkout --branch-new`, `lane open`) or a `tag-name` (for `tag`) of §2.3, and every part of it matches `ref-name` (§2.1). `main` itself is never created by a verb | [AR §5a.1], [50 §2.2] rule 6 |
| RN-2 | No segment of the user part equals `main`, `lane`, `plan`, `merge`, `import`, `orphans`, `tags` or `from` | [AR §5a.1] "reserved as lane and branch names" |
| RN-3 | No segment of the user part matches, as a whole, `c` followed by 7 to 64 digits or letters `a`–`f`, or `s` followed by one or more digits | [50 §2.2] rule 6 (S-11) |
| RN-4 | No segment of the user part, taken up to its first `.` (the whole segment when it has none), equals a Windows device name: `con`, `prn`, `aux`, `nul`, `com0` to `com9`, `lpt0` to `lpt9` | [80] P11 (b), P5 |
| RN-5 | No segment of the user part ends in `.lock` | [80] P11 (b); git |
| RN-6 | The complete name is at most 128 bytes | this chapter (open point 13) |
| RN-7 | No live ref has the same name. This is [80] X-F9's fold-equality refusal, which by §2.1 is byte equality | [80] X-F9 |
| RN-8 | No live ref of kind `work`, `plan` or `tag` has a name that is a proper prefix of the new name ending at a segment boundary, and the new name is no such prefix of a live one (`lane/a` excludes `lane/a/b` and the reverse) | git's loose-ref layout (§2.7) |

*(Informative)* Accepted: `lane/l5np`, `lane/team_2/phys-ecs`, `plan/q3.v2`, `tags/v1.2`, `tags/release/1.0`. Refused:
`lane/L5np` (RN-1), `feature-x` (RN-1: no namespace), `lane/merge` (RN-2), `lane/x/from/y` (RN-2), `lane/cafebabe`
(RN-3), `tags/s4400` (RN-3), `lane/nul.txt` (RN-4), `tags/v1.lock` (RN-5), `lane/a..b` and `lane/.x` (RN-1).

Names the machinery builds (staging, import and orphans names) are built only from names that passed these rules, so they
need no check beyond uniqueness: the machinery never creates a second live ref of one name, and the case where it would is a
refusal of the operation (a second merge of one staged pair, I41′, §9.1).

### 2.5 Ref-name input

- **IN-1 (NFC).** Every ref-name argument and `ref`-typed configuration value is NFC-normalised before any rule is checked
  ([80] X-F9). Any byte outside ASCII that remains then fails RN-1. NFC turns non-ASCII input only into non-ASCII or into
  ASCII that RN-1 refuses: the only non-ASCII characters whose normalisation is ASCII are the canonical singletons
  U+212A → `K`, U+037E → `;` and U+1FEF → `` ` `` (checked against the UCD 17.0.0 `UnicodeData.txt` of `fixtures/ucd/`),
  and a composition always yields a non-ASCII character. So IN-1 needs no composition table at M0: an implementation
  refuses any non-ASCII byte under RN-1 and its error text ([F19 §10.2] `bad_ref_name`) quotes the input as given
  (completed by IN-3, never normalised), which accepts and refuses exactly what NFC followed by RN-1 does.
- **IN-2 (no folding).** Upper case is never folded: `lane/L5np` fails RN-1 (hint: ref names are lower case).
- **IN-3 (namespace completion).** Only the creating verbs complete a name:
  - `branch NAME [--kind work|plan]` and `checkout --branch-new NAME`: a NAME that begins with `lane/` or `plan/` is taken
    as written, and a given `--kind` must agree with it (`work` with `lane/`, `plan` with `plan/`), else IN-3 fails
    (`bad_ref_name` with the reason `--kind <k> does not match <prefix>` and the JSON `rule` `IN-3`, [F19 §10.2], §10.3);
    any other NAME becomes `lane/NAME`, or `plan/NAME` under `--kind plan`, and is then checked.
  - `tag NAME [COMMIT]`: NAME becomes `tags/NAME` unless it begins with `tags/`.

  No other argument is completed: a revision `l5np` names a ref `l5np`, which cannot exist (E301, §3.9).
- **IN-4 (`ref` configuration values).** A `ref`-typed key ([CFG], for example `default-branch`) takes a `branch-name` that
  satisfies RN-1 to RN-6; the ref need not exist when the key is set.

### 2.6 Life of a name

- A ref's id is allocated at creation and never reused ([F11 §3.7]; `main` is `ref_id` 0). No ref is ever renamed: a new
  name is a new ref.
- At most one live ref holds a name (RN-7, and the machinery's uniqueness). A deleted ref keeps its row and name until its
  reflog window expires ([F11 §3.8], [F17 §11.2]); meanwhile a new ref may take the name with a new id.
- `main` is never deleted. A tag's only moves are its creation and its deletion. A staging ref is deleted by
  `merge --continue` when it lands, and by `merge --abort` (§9).
- `branch -d`/`-D` names a branch; it refuses `main`, staging refs (use `merge --abort`), import refs and orphans refs
  (both are removed by `gc` after their commits are merged or expired, [F16]).

### 2.7 Names in the git image

Branch and tag names map to git refs by [AR §5b.4]'s rule (`refs/heads/<name>` and `refs/tags/<u>` in a separate image
repository, `refs/moirai/heads/…` and `refs/moirai/tags/…` in a project repository); [F14] owns the mapping. §2.1 with RN-4,
RN-5 and RN-8 makes every exported ref valid under git's ref-format rules, creatable as a loose ref on Windows, Linux and
macOS, and free of file/directory clashes between live refs. Staging, import and orphans refs are never exported
([AR §5b.4]). A destination that already holds two refs equal under `fold_v1` (written by another tool) is reported by
`doctor image` ([80] P11 (b), [F14]).

## 3. Revisions

### 3.1 Text grammar

The text of a revision is the same wherever the store reads one: a CLI argument (`--at REV`, `merge SRC`, `checkout REF`,
`diff A..B`, `log REF`, `cherry-pick COMMIT`, `revert COMMIT`, `tag NAME COMMIT`, `--from`, `--if-tip`, `--expect`,
`--base`), an MCP `use` parameter, and a revision position of LQ text ([LQ/lexical §4.2], §7). [LQ/lexical §7] lexes the
same language inside query text and adds what only LQ has (`$param`, lists, whitespace around a range operator);
a difference between the two texts is a review finding.

```abnf
revision   = rev-spec / rev-range
rev-range  = rev-spec range-op rev-spec          ; ranges only where a verb or relation takes one
range-op   = "..." / ".."                        ; longest match
rev-spec   = rev-base *rev-suffix
rev-base   = %s"HEAD" / ref-name                 ; a ref-name is classified by §3.2
rev-suffix = "~" *DIGIT                          ; first-parent ancestor; no digits means 1
           / "^" *DIGIT                          ; n-th parent; no digits means 1
           / "@" 1*DIGIT                         ; reflog position
           / "@{" 1*DIGIT "}"                    ; the same, git's spelling
           / "@" datetime                        ; the ref at a wall time
datetime   = 4DIGIT "-" 2DIGIT "-" 2DIGIT [ %s"T" 2DIGIT ":" 2DIGIT [ ":" 2DIGIT ] ] [ %s"Z" ]
```

- After `@`, four digits followed by `-` start a `datetime`; otherwise digits are a reflog position.
- `HEAD` is the four bytes `48 45 41 44` and is not followed by a letter, digit, `_`, `-`, `.` or `/`.
- Counts after `~`, `^`, `@` and `@{` are decimal, 0 to 4,294,967,295; leading zeros are accepted ([LQ/lexical] L-4).
- Datetime fields have the ranges of [LQ/lexical §7.5]; the time zone is UTC, `Z` changes nothing, a missing time is
  00:00:00 and missing seconds are 00. Its value T is milliseconds since 1970-01-01T00:00:00Z.
- A revision argument is the whole argument: a trailing byte that the grammar does not consume makes it malformed (E003 in
  LQ text, [LQ/lexical §7.6]; `usage`, exit 2, in argv).

### 3.2 Classification

A `ref-name` base that is exactly one `ref-word` (no `/`, no `.`) is classified in this order ([LQ/lexical §7.2]):

1. `c` followed by 7 to 64 lower-case hex digits: a **commit literal** (an id or id prefix);
2. `s` followed by one or more digits: a **sequence literal**; a value above 2^64 − 1 is malformed;
3. anything else: a **ref name**.

Every other base is `HEAD` or a ref name. Because of RN-3 and the namespaces of §2.3, no ref name has the shape of rule 1
or 2: the only one-word ref name is `main`.

### 3.3 Snapshot

A command or query resolves every revision it reads once, against one snapshot of the store: the refs, moves and commits
covered by the `committed_lsn` it read at its start ([50 §2.4]; [F04]). Every part of a composite query uses that snapshot
([50 §3.9] item 2).

### 3.4 Bases

A base resolves to a **ref**, a **commit** or **ε**:

| Base | Resolves to |
|---|---|
| `HEAD` | the caller's branch in the order of record of [AR §5a.4] ([90 §4.1]'s Branch row): a ref, or the detached commit of a `ClientHead` whose target is a commit ([F05 §9.3]). A resolved name that no live ref holds is E301 |
| ref name | the live ref of that name. Only when the base is followed immediately by a reflog suffix (`@n`, `@{n}`, `@T`) and no live ref has the name, the deleted ref of that name with the greatest `ref_id` whose row the store still holds. Otherwise E301 |
| commit literal | the one commit the store holds whose id, in lower-case hex, begins with the literal's digits. The candidates are every commit record in the valid log and in `hist` files ([F05], [F10]), orphans and staging commits included. None: E301; several: E301 listing them in ascending `seq` |
| sequence literal `s<N>` | N = 0: ε. Otherwise the commit whose `seq` is N ([F06 §4.4.3]); none (N above 2^32 − 1, above `commit_seq`, or collected by `gc`): E301 |

A ref resolved without a suffix stands for its tip where a commit is needed: a ref with no commit yields ε.

### 3.5 Suffixes; moves and the reflog

**Moves.** The moves of a ref R, in log order, are:
- every `Commit` record with R's `ref_id` whose implied ref move was adopted ([F06 §4.4.2]; a commit parked on
  `orphans/<R>` is not a move of R): `old` = the commit named by `ref_old` (zero when absent), `new` = the commit, time =
  its `append_hlc` ([F06 §4.4.5]);
- every `RefUpdate` record with R's `ref_id` ([F05 §9.2]): `old`, `new` and `hlc` from the record.

The **held** moves are those whose records the store still holds. `gc` drops a move only when it is older than
`gc.reflog-expire` at that `gc` run ([F17 §11.2]), so every move younger than the window is held. `undo N` moves R to the
value `R@N` names ([AR §5a.5]), so the reflog suffix and `undo` count the same moves.

Suffixes apply left to right to the value built so far:

| Suffix | Applies to | Result |
|---|---|---|
| `@n`, `@{n}` | a ref, and only as the first suffix after a `ref-name` or `HEAD` base that resolved to a ref | n = 0: tip(R). n ≥ 1: with m₁ … m_k R's held moves newest first, the `old` value of m_n; E301 when n > k. A zero value is E301 |
| `@T` | as `@n` | the `new` value of the newest held move whose time, taken as `hlc >> 16` (milliseconds, [F01 §5.7]), is at most T. When no held move is that old and T ≥ now − `gc.reflog-expire` (now read at the snapshot): the `old` value of the oldest held move, or tip(R) when R has no held move — no dropped move can lie after T. Otherwise E301. A zero value is E301 |
| `~n` | a commit, or a ref (replaced by its tip) | n = 0: the commit itself. n ≥ 1: its first parent's `~(n − 1)`. ε, or a root commit before n steps: E301 |
| `^n` | as `~n` | n = 0: the commit itself. n ≥ 1: the n-th of its **actual** parents, in record order ([F06 §4.3] order 4: first = dst or lane tip, second = src tip or `sync_base`); n above the number of parents: E301 |

A reflog suffix after any other suffix, after a commit literal, sequence literal or detached `HEAD`, or a second reflog
suffix, is E301 (open point 20). Parent navigation always follows the actual parents this store holds, never a demoted
commit's stated id ([F06 §4.4.1]).

### 3.6 Ranges

Both ends of a range resolve to commits or ε (a ref end stands for its tip).

| Range | `log` ([50 §2.4]) | `diff` ([AR §5a.6], [50 §2.4]) |
|---|---|---|
| `a..b` | the commits in anc*(b) \ anc*(a) (§4.1), newest first by the order of [AR §5a.6] | when a is ε or a ∈ anc*(b): state(b) against state(a). Otherwise state(b) against the base Base(a, b) of §4.3, with notice N05 ([LQ/errors]) |
| `a...b` | the commits in (anc*(a) ∪ anc*(b)) \ A(Base(a, b)) | both sides against Base(a, b): a's changes are side `ours`, b's side `theirs`, keys both changed `both` — the preview of merging b into a |

Base(a, b) is the base a merge of b into a uses (§4.3), recursive virtual base included, so a preview and a merge always
agree. Base(a, b) = Base(b, a). A(·) is the ancestor set of §5.2.

### 3.7 Views and writes

- A revision that is exactly the name of a branch, or `HEAD` resolving to a branch, selects that branch's **tip view**:
  runtime and tree-derived relations exist there ([50 §3.9] item 4), and writes land there (`work` fully, `plan` under
  I33′'s mask).
- Every other revision selects a **commit view** of the resolved commit (or of ε): a past view, flagged `as-of`, where
  runtime relations are E302 and writes are E305 ([50 §3.9] items 4, 6, 7). This includes tags, import refs, orphans refs,
  suffixed revisions and literals, even when they resolve to a branch's current tip.
- A staging ref's view is a commit view of its tip flagged `staged (read-only)` ([LQ/envelope]); it accepts only `RESOLVE`
  statements, which land on it (§9.3). An import ref that holds a staged import accepts `RESOLVE` likewise (§9.6; open
  point 21).

### 3.8 Display forms

| Form | Where | Meaning |
|---|---|---|
| `c` + 8 lower-case hex digits | text output ([AR §7.1], [LQ/envelope]) | a commit, abbreviated; it is a valid commit literal |
| `c` + 64 lower-case hex digits | `--json v1`, `.moi` values, staging names | a commit, exactly |
| `rev <seq>` | headers | the view's sequence number; `s<seq>` is its revision spelling |
| a ref name | headers (`branch: <ref>`), everywhere | as stored |

### 3.9 Errors

Resolution failures are E301 `unknown_revision` (exit 3) and read-only targets E305 (exit 6) ([LQ/errors]); a malformed
revision is E003 in LQ text and `usage` (exit 2) in argv ([F19 §10.2]). The E301 cases this chapter adds (a misplaced
reflog suffix, a reflog position beyond the held moves) need texts in [LQ/errors] (open point 20).

## 4. Ancestry and the base of a merge

### 4.1 The commit DAG

- The DAG's vertices are the commits the store holds; each commit's edges go to its **actual** parents ([F06 §4.3] order
  4). Every parent's record precedes its child's in the log ([F06 §4.4.1], C), so the DAG is acyclic and "ancestor of" is a
  strict partial order.
- anc*(c) is the set of c and all its ancestors; anc*(ε) = ∅.
- gen(c) = 1 + max gen of c's actual parents, and 1 for a root ([AR §5a.1], [F06 §4.3] order 7). gen(x) ≥ gen(y) implies
  that x is not a proper ancestor of y, and a proper ancestor has a strictly smaller gen.
- The **order key** of a commit is the pair (gen(c), id(c)): gen as an integer, then the 32-byte id bytewise ([F01 §6.6]).
  Commit ids are distinct, so the order is total.

### 4.2 Common ancestors and LCAs

For commits x and y: CA(x, y) = anc*(x) ∩ anc*(y), and **LCA(x, y)** = the maximal elements of CA(x, y): every c ∈ CA(x, y)
such that no other d ∈ CA(x, y) has c ∈ anc*(d). The LCAs are pairwise incomparable (none is an ancestor of another). If
x ∈ anc*(y), LCA(x, y) = {x}. If x or y is ε, LCA(x, y) = ∅.

The definition names a set; how an engine finds it (the gen-pruned bidirectional walk of [AR §5a.7] step 1) is free, and
the model computes it from full ancestor sets ([60 §4.2]).

### 4.3 The base-selection rule (I31′)

A merge of src into dst, a `sync` (a merge of `main` into the lane), a range's base (§3.6) and a foreign-merge import
([AR §5b.6] step 3) take their base state by exactly one rule. With x = tip(dst) and y = tip(src) (or the two parents of the
imported merge):

1. `merge --base C`: state(C), for any commit C the store holds. No ancestry check is made ([AR §5a.7] step 1).
2. Otherwise L = LCA(x, y), sorted ascending by the order key of §4.1:
   - |L| = 0: state(ε) (unrelated histories; [RULES/merge-table] VB-003);
   - |L| = 1: state(L₁) (the daily case, I25′);
   - |L| ≥ 2: the recursive virtual base VBase(L) of §5.

A revert or cherry-pick is not a merge of two tips: its base is fixed by [RULES/merge-table] DM-003 to DM-005 (the origin's
first parent, or the origin itself) and this rule does not apply to it.

## 5. The recursive virtual base (the addendum to [AR §5a.7])

[AR §5a.7] step 1 and [60 §3.4]: under criss-cross — two LCAs L₁, L₂ of x and y that disagree on a key k, x having resolved k
to L₁'s value and y to L₂'s — choosing one LCA as the base takes the other side's value without a conflict. The rule: the
LCAs are merged pairwise in generation order, ties by the lowest commit id, by the same typed rules, and the result is the
base; a key whose virtual-base value is a conflict value is clean when both sides hold the same value and conflicts when
both sides resolved it and differ. This section states the rule exactly.

### 5.1 Construction

For LCAs L = (L₁, …, L_k), k ≥ 2, sorted ascending by (gen, id):

```
V₁ := state(L₁)                     A₁ := anc*(L₁)
for i = 2 … k:
    Mᵢ := maximal elements of (A_{i−1} ∩ anc*(Lᵢ)), sorted ascending by (gen, id)
    Bᵢ := state(ε)        if Mᵢ is empty
          state(m)        if Mᵢ = {m}
          VBase(Mᵢ)       if |Mᵢ| ≥ 2
    Vᵢ := VM(dst = V_{i−1}, src = state(Lᵢ), base = Bᵢ)       (§5.3)
    Aᵢ := A_{i−1} ∪ anc*(Lᵢ)
VBase(L) := V_k
```

- "Generation order" is ascending: the oldest LCA is V₁, and each later LCA is merged into the accumulated state as src.
  The accumulated state is always the virtual merge's dst ([RULES/merge-table] VB-005, VB-006, VB-008).
- For i = 2, Mᵢ = LCA(L₁, L₂): the inner base of the first virtual merge is the ordinary base of L₁ and L₂.
- A virtual state is never stored: it has no commit record, no id, no `seq`, no ref, no markers and no absorbed vector; it
  is never exported and never hashed. The merge that uses it records only its own result (§7.8).

### 5.2 Ancestor sets of virtual states and the inner bases

A virtual state Vᵢ has the ancestor set **A(Vᵢ) = Aᵢ** = anc*(L₁) ∪ … ∪ anc*(Lᵢ). It is a set of real commits: a virtual
state is never an ancestor of a real commit. For a real commit c, A(c) = anc*(c); for ε, A(ε) = ∅; for a base state B,
A(B) is the ancestor set of the commit or virtual state it is the state of.

The base of the i-th virtual merge is the base the rule of §4.3 gives for the pair (V_{i−1}, Lᵢ) over these sets: Mᵢ is
the set of maximal elements of A(V_{i−1}) ∩ A(Lᵢ). Since Lᵢ and every Lⱼ (j < i) are incomparable, Mᵢ contains only proper
ancestors of Lᵢ.

### 5.3 The virtual merge

VM(dst, src, base) is the typed three-way merge of §7 and the rule tables with these differences, each stated as the
[RULES/merge-table] VB row it realises or proposes (open point 8):

| # | Rule inside a virtual merge | Row |
|---|---|---|
| VM-1 | dst is never `main`: the owner-authority rows that need "dst is the ref `main`" (MR-011, MR-016) never apply | VB-007 |
| VM-2 | No automatic policy applies: `merge.policy.<kind>`, `--policy` and `--strict` of the outer command are ignored. A `DeleteVsModify` takes the kind's existence policy from the schema of the virtual merge's dst state ([F08 §8.5.1]) | VB-019 (open point 8) |
| VM-3 | Validators (V01 to V13 of [F13 §5]) do not run; a virtual merge never stages and records no violation and no hint | VB-009 |
| VM-4 | A row whose disposition is `structural` yields its `result` value (`stage-take-o`: o; `stage-diff3`: the diff3 result) and records nothing | VB-009 |
| VM-5 | Conflict values are kept as values: a key the virtual merge leaves in conflict holds that conflict value in the result | VB-010 |
| VM-6 | The re-key and composition rules of [RULES/link-merge-rules] run as in a real merge | VB-012 |
| VM-7 | Hierarchy moves (RS-007) are the Kleppmann steps of §7.4's row for a merge, with the inner base as B: the replay starts from the replay start R of the pair, taken over A(V_{i−1}), A(Lᵢ) and A(Bᵢ) (§5.2), and a side's steps are the real commits of A(side) \ A(R) that have step keys, a virtual side's included; every rule of that row applies (the recursive second-parent step keys of a two-parent commit, the (0, 0) steps, dst's moves first where two steps share a key, a move that keeps its node's parent never undone, the undo one move at a time among the moves that changed their node's parent, least uid on a cycle first). A virtual merge is never one-sided ([AR §11] OQ-A-11; [RULES/merge-table] open point 35 (iv)) | VB-011 |
| VM-8 | §5.4 applies to every key whose inner base holds a conflict value | VB-013, VB-014 |

### 5.4 Keys whose base holds a conflict value

In every merge — real or virtual, with a real or virtual base — a key k whose base value b is a conflict value
B = {cls_B, b′, o_B, t_B} is decided by these rules, in this order, before any row of the key's own class:

| # | Case | Result |
|---|---|---|
| RVB-1 | o ≈ t | clean; k takes o (a conflict value included: it stays unresolved and emits no new `Conflict` op) |
| RVB-2 | o ≈ b and t ≉ b | clean; k takes t (dst left the base's conflict as it was; src's value lands, as MR-004 does at a real LCA) |
| RVB-3 | t ≈ b and o ≉ b | clean; k takes o |
| RVB-4 | otherwise (o ≉ t, o ≉ b, t ≉ b) | conflict value {class, base b′, ours flat(o), theirs flat(t)}, where flat is §6.4's and class is the `conflict` cell of the first row of k's merge class whose disposition is `value` and whose case holds for (b′, flat(o), flat(t)), or cls_B when no such row holds |

x ≈ y is §7.3's equality, widened for two conflict values only: they are also ≈ when their classes and `base` sides
are equal and x's `ours` and `theirs` equal y's `theirs` and `ours` (on an existence key, x's provisional side is then
the opposite of y's). The orientation of a conflict value comes from the merge that made it: the virtual base orders its
sides by the (gen, id) order of the LCAs, while a side that left its own merge's conflict unresolved holds it in its
own merge's orientation, so without the exchange VBC-3 would be clean for one side and an RVB-4 conflict for the other
(spec sync 2b). A conflict value's sides are never conflict values, so RVB-4 never nests.

- **Both sides equal → clean** (RVB-1): two sides that resolved a criss-cross identically never conflict ([60 §3.4]).
- **Resolved differently → conflict** (RVB-4): two sides that each changed k away from the virtual base and hold
  different values always conflict, whatever the rows of k's class would do on a plain base; a single chosen LCA could
  have taken one of them silently.
- **One side untouched** (RVB-2, RVB-3): a side that still holds the virtual base's conflict value has not touched k since
  the base (I25′). This reads [60 §3.4]'s "conflicts whenever they differ" as "whenever both changed it and differ"
  (open point 2).
- When RVB-4's class is `DeleteVsModify` on an existence key, the node's other keys take the provisional state of
  [RULES/merge-table] RS-008 if exactly one of flat(o), flat(t) is live, and o's values otherwise; the conflict value's
  provisional side (§6.3) is set accordingly.

[RULES/merge-table] states these rows in its evaluation order MR-001 (RVB-1), MR-003 (RVB-3), MR-004 (RVB-2) and MR-002
(RVB-4, with RS-010 and VB-018; review pass 1 S1-15).

**A plain base with a conflict-valued side** (MR-005; pass 1, P1-21; open point 3 adopted). When b is plain, o ≠ b,
t ≠ b, o ≠ t, and o or t (or both) holds a conflict value, the result is the conflict value {class, base b, ours flat(o),
theirs flat(t)}, where class is the `conflict` cell of the first row of k's merge class whose disposition is `value` and
whose case holds for (b, flat(o), flat(t)), or, when no such row holds, the class of the conflicted side (dst's when both
are conflicted). On an existence key the provisional side follows the last bullet above. The conflicted side's own
conflict value stays readable as the `Conflict` op's `old` (§6.4). No merge input therefore leaves the model in
`SpecGap`: [RULES/merge-table] MR-005 states this rule with RS-015 (pass 1, round 1). **Decided 2026-09-28** (owner
question OQ-M-1, option (a); `reviews/owner-questions.md`): the owner accepted the changed rows (MR-005 and RS-015
among them); the re-signature in `rules/SIGNED.md` follows under V3.

### 5.5 Termination, cycles and criss-cross

- **Termination.** Let μ(L) = the greatest gen among L. Every element of an inner set Mᵢ is a proper ancestor of Lᵢ, so
  μ(Mᵢ) < gen(Lᵢ) ≤ μ(L). Every recursive call strictly lowers μ, which is a positive integer, so the recursion ends, with
  depth at most μ(L). The DAG's acyclicity (§4.1) is what makes μ well-founded.
- **No state is revisited.** VBase(L) is a pure function of the sorted list L, and Vᵢ of the prefix (L₁ … Lᵢ); an engine may
  memoise both by that list.
- **Hierarchy cycles.** Inside a virtual merge a Kleppmann step that leaves a cycle has the moves that changed their
  node's parent undone as §7.4's row says (RS-007) and nothing is recorded (VM-3, VM-4); a virtual state's `parent`
  relation is therefore a forest, whose depth is not checked.
- **Other cycles and violations.** A virtual state may break I2, I4's depth, I5′, I6, I7, I-F1 or schema conformance,
  because validators do not run there (VM-3). Only the final candidate of the real merge is validated (I37′), so nothing a
  virtual state holds can reach a ref by itself.
- **Criss-cross.** |LCA| ≥ 2 arises only through merges across lanes and branch-of-branch merges (CM9). A merge into `main`
  runs its sync first ([AR §5a.7] step 0), after which LCA(src, main) = {tip(main)}, so the daily path never builds a
  virtual base ([RULES/merge-table] VB-017).
- **Unrelated histories.** An empty LCA set, at any level, gives the empty state as base (§4.3).

### 5.5a Work and memory

A virtual base is built inside the merge that needs it and is charged to that merge (pass 1, P1-29): every virtual
merge's keys, entries and text merges count against the command's `wmem` and work budget ([F17 §4.4] W1–W2, [CFG §10.4]),
exactly as the real merge's do. A merge whose virtual bases, held in memory, would exceed `wmem` is a bulk-class producer:
it streams per-key results and spills sorted runs as [F07 §10.6] requires, or is refused with the budget error when it may
not ([F17 §4.4] W1). Memoised states (§5.5) are released when the merge ends. Criss-cross histories are rare on the daily
path (§5.5), so the rule bounds a worst case and costs nothing on the usual one.

### 5.6 Determinism

VBase(L), and so every merge result, depends only on: the commit DAG over actual parents; commit ids and gens; the states
at the commits involved; the canonical `hlc` of the commits that moved hierarchy keys (§7.4); and the rule tables. It does
not depend on `#N`, symbol ids, store-local schema ids, lsns, `seq`, `append_hlc`, configuration, the outer command's
flags (other than `--base`, which bypasses it), the process, or the order in which an implementation enumerates LCAs.
Two stores that hold the same commits compute the same virtual base and emit merge commits with the same canonical form
(I28′, I30′).

*(Informative)* An engine that computes base values per key by folds ([AR §5a.7] step 3) must produce exactly the values of
this definition for every key its rules read; counters and hierarchy keys need the base even when o = t.

### 5.7 Required cases

The model's suite (E5, [PLAN §7]) and GT6's I31′ properties ([60 §3.13]) cover at least:

| # | Case | Expected |
|---|---|---|
| VBC-1 | two LCAs, both sides resolved k identically | clean, k = the common value (the exit criterion "both sides equal → clean") |
| VBC-2 | two LCAs, the sides resolved k differently | conflict, base = the inner base's value, class from the key's rows |
| VBC-3 | two LCAs, one side holds the virtual base's conflict value unchanged (in either orientation, §5.4's ≈), the other resolved | clean, the resolving side's value, for both lanes |
| VBC-4 | two LCAs agreeing on k, neither side touched k | clean, no conflict |
| VBC-5 | LCAs enumerated in every permutation | the same base and result |
| VBC-6 | three LCAs | V₃ = VM(VM(L₁, L₂), L₃) with the inner bases of §5.2 |
| VBC-7 | an inner set Mᵢ with two elements (nested criss-cross) | the recursion of §5.1 |
| VBC-8 | unrelated roots | empty base |
| VBC-9 | a counter incremented on both LCAs and on both sides | the sum over the virtual base |
| VBC-10 | a hierarchy move that would close a cycle inside a virtual merge | undone silently in the base (RS-007); the real merge validates |
| VBC-11 | a `DeleteVsModify` in the virtual base, then both sides resolved differently | RVB-4 with class `DeleteVsModify` and RS-008's provisional state |
| VBC-12 | two stores importing the same criss-cross history | byte-identical merge commits |

### 5.8 Example (informative)

R (gen 1) sets k = z. L₁ (lane/a, gen 2) sets k = x; L₂ (lane/b, gen 2) sets k = y; id(L₁) < id(L₂). A merges L₂ into
lane/a: `FieldEdit` {z, x, y}; A's author resolves k = x. B merges L₁ into lane/b: `FieldEdit` {z, y, x}; B's author
resolves k = y. Now `merge lane/b --into lane/a` (x = A, y = B): LCA = {L₁, L₂}. V₁ = state(L₁); M₂ = {R};
V₂ = VM(L₁, L₂, R) holds k = {FieldEdit, z, x, y}. The real merge sees o = x, t = y, b = that conflict value: RVB-4 gives
{FieldEdit, z, x, y}. With L₁ alone as the base (k = x), o would equal the base and y would land silently. Had B resolved
k = x, RVB-1 would give k = x, clean; had A left k unresolved as {FieldEdit, z, x, y}, RVB-2 would give y, clean; had B
left k unresolved as {FieldEdit, z, y, x} (its own merge's orientation), t ≈ b and RVB-3 would give x, clean.

## 6. Conflicts as data

### 6.1 The conflict-class enumeration

A class is one `u8` wherever a structure stores one: `cstate.class` ([F06 §6.2]), `Conflict.class` and `Violation.class`
([F06 §7.7]) and `CONFLICTS.class` ([F11 §10]). The code space is [F19 §12.1]'s; this chapter owns the value-conflict codes
1–63:

| value | name | meaning ([AR §5a.8]) |
|---|---|---|
| 1 | `FieldEdit` | a key both sides changed to different values where no typed rule merges them: scalars, edge properties, the observation composite, anchor selectors, schema items and named queries |
| 2 | `StatusFork` | two status moves the kind's lattice cannot join: a side state against a forward move, incomparable states, a reopen against a completion |
| 3 | `TextHunk` | a text or body whose diff3 has a conflicting chunk (§7.5) |
| 4 | `DeleteVsModify` | a node deleted on one side and modified on the other; a named query dropped on one side and changed on the other |
| 5 | `SupersedeFork` | a second active superseder of one target (I6) |
| 6 | `OwnerFieldEdited` | an owner-authority field changed on both sides of a merge whose dst is not `main` |
| 7 | `PathClaim` | two live file nodes with different uids at one exact path (I-F1) |

- 0 and 8–63 are invalid in format v1 (V). Codes 64–127 (structural violations) and 128–191 (hints) are [F19 §12.2] and
  [F19 §12.3]'s; 192–255 are invalid.
- Texts, JSON, `.moi` `class=` values ([AR §5b.2] rule 3) and `CALL conflicts()` rows name a class by the name above, the
  frozen spelling. Whether canonical item 10 carries the name or the code is [F07]'s; both are frozen and never reused
  ([AR §2.12]).
- **`DATA` has no code.** [AR §5a.8] lists `DATA`, but I34′, [AR §2.7] and [AR §5a.5] record a revert's or cherry-pick's
  before-image mismatch "as a `FieldEdit` conflict value", and [RULES/merge-table] DM-013 emits the key class's own `both`
  class. A `DATA` case is therefore stored with the class its key's rules give, and the commit's kind (`revert`,
  `cherry-pick`) tells it apart (open point 5).

### 6.2 Where each class sits

A conflict value sits on exactly one key and replaces that key's value. The key classes are [F06 §6.1]'s.

| Class | Key (`ckey` class) | Sides (`kval` of the key class) | Emitted by |
|---|---|---|---|
| `FieldEdit` | `field` (4) of merge class `scalar` or `authority`; `edge` (7), `at` edges with their anchor props included; `observation` (5); `schema` (9) of a `query` item | the key's b, o, t | [RULES/merge-table] MR-009, MR-020, MR-050, MR-051, MR-062, DM-013; [RULES/link-merge-rules] LM-006, LM-021, LM-022 |
| `StatusFork` | `status` (2) | b, o, t | MR-025 |
| `TextHunk` | `field` (4) of merge class `text`; `body` (8) | the three whole texts (`body`: the three body hashes) | MR-032, MR-038; §7.5's length rule and its dropped-body rule |
| `DeleteVsModify` | `existence` (1) of the node; `schema` (9) of a `query` item | b, o, t; a `live` side carries its node image (`snap` = 1, [F06 §6.2]); on an existence key the `prov` byte (§6.3) | MR-042, MR-060, LM-010 |
| `SupersedeFork` | `edge` (7) of kind `supersedes` from the **src side's** superseder S to the target T | base `absent`, ours `absent`, theirs = S's edge value | V06 ([F13 §5]); key form fixed here ([RULES/merge-table] open point 9) |
| `OwnerFieldEdited` | `field` (4) of merge class `owner` or `authority` | b, o, t | MR-014, MR-019 |
| `PathClaim` | `observation` (5) of **each** claiming file node | that node's composite b, o, t | V08 ([F13 §5]); [RULES/link-merge-rules] PC-002 |

The numbers in parentheses are [F06 §6.1]'s `ckey` class values, which equal [F07 §6.1]'s class codes (pass 1, A1-46).

A key holds at most one conflict value. When several rules would put one on a key, the one that runs first in [F13 §5]'s
order wins and the later rule sees the key already conflicted (the conflicted-key rows).

### 6.3 Representation and the provisional value

- **In a commit.** The commit that introduces a conflict value carries one `Conflict` op on its key: `old` = the key's
  before-image `cstate` (dst's value, possibly an earlier conflict value), `class`, `base`, `ours`, `theirs` ([F06 §7.7]).
  Any later commit that replaces it carries a `Resolve` op (NF-9 of [F06 §7.8]).
- **In a view.** The key's value is the conflict value. The view's `CONFLICTS` section holds one row per conflicted key:
  `n` = the key's owner `#N` (**0 for a `schema` key**, which has no owner), the class, the `id16` of the introducing commit,
  the key and the three sides ([F11 §10]; open point 18).
- **`conflicted`.** A node is `conflicted` on a view exactly when it owns a key ([F06 §2.3]) that holds a conflict value
  there ([F08 §3.2] flag 5, [RULES/merge-table] RE-008). A conflicted node leaves `ready` ([AR §5a.8]).
- **Provisional value.** Every reader, derived predicate, index and validator that needs a plain value of a conflicted key
  uses its **provisional value** P:
  - for a key of class `existence`: the side the conflict value's **provisional side** names — `ours` or `theirs` — which
    the merge sets from the existence policy in force ([RULES/merge-table] RS-008: `delete-wins` → the deleting side,
    `resurrect` → the modifying side, `none` → ours; §5.4 for RVB-4);
  - for every other key: `ours` when `ours` is not `absent`, else `theirs`.

  The node's stored columns, indexes and bitsets hold P; the `CONFLICTS` row holds the conflict. So a `DeleteVsModify`
  node under `delete-wins` is deleted for I26′ and every predicate while the conflict stands, a conflicted edge is present
  when either side has it, and a `SupersedeFork` leaves both superseders present, which I6 tolerates while the value stands
  (as I-F1 tolerates a `PathClaim`).
- **The provisional side is a stored byte.** P of an existence key depends on the policy of the merge that emitted it,
  which `--policy` and `merge.policy.<kind>` can override (AP-004, AP-005), so it cannot be derived from the sides. This
  chapter requires one byte, `prov` (`u8`: 0 `ours`, 1 `theirs`, 2–255 invalid), in every `cstate` with `cs` = 1 whose key
  class is `existence`, placed after `theirs`; it is part of the state, so [F07] hashes it with the conflict value, and the
  image carries it as the node file's form (a tombstone file when P is `deleted`, [AR §5b.2] rule 8). [F06 §6.2] stores it
  after `theirs` in the `cstate` and in the `Conflict` op ([F06 §7.7]), [F07 §7.3] hashes it by name, and [F11 §10]
  keeps it in the `CONFLICTS` row (pass 1, S1-5, A1-9; open point 4).

### 6.4 Flat sides

A conflict value's sides are plain values ([F06 §6.2], C). Where a rule needs a side value from a side that itself holds a
conflict value — RVB-4 (§5.4) — it uses **flat(v)**: v when v is plain, and v's provisional value P (§6.3) when v is a
conflict value. The side's full conflict value stays readable as the `old` before-image of the new `Conflict` op.

### 6.5 Resolution

A `Resolve` op ([F06 §7.7]) replaces a key's conflict value, on a staging ref a violating key's value, or on a work or
plan branch a flagged edge, by a plain value (`new`):

| `choice` | `new` |
|---|---|
| 0 `ours` | the conflict value's `ours` side |
| 1 `theirs` | its `theirs` side |
| 2 `base` | its `base` side |
| 3 `value` | a value the caller supplies, checked against the key's type ([F08 §8.6]) |
| 4 `repoint` | violation edges and flagged edges only: the edge key becomes `absent` and an `AddEdge` of the same kind, with the target in place of the replaced endpoint, is in the same commit ([F06 §7.7]) |
| 5 `drop` | flagged edges only: the edge key becomes `absent` |

- A `live` existence side restores the node's value keys from its node image (`snap` = 1, [F06 §6.2]). Its **hierarchy key
  and out-edges** are restored from that side's state at the conflict's introducing commit M (the commit whose `Conflict`
  op set the value; `CONFLICTS.commit`, [F11 §10]): `state(first parent of M)` for `ours`, `state(second parent of M)` for
  `theirs`. The `Resolve` commit carries, as ordinary ops of the same commit, a `Move` that sets the node's (parent, order)
  to that state's and an `AddEdge` or `SetEdgeProps` for each out-edge of the node in that state that the view lacks or
  holds with other props (pass 1, S1-22). The write path's checks apply to them as to any write ([F08 §8.6]); a restore
  that would break an invariant (a parent that is no longer live, a structural edge to a deleted target) refuses the
  `resolve` with that check's code, and the caller clears the obstacle first. The node image keeps only value keys
  ([F06 §6.3], [F07 §7.4]), so no hashed byte changes; the model computes the same side state from history.
- **`SupersedeFork`.** `ours` and `base` remove S's edge (`absent`). `theirs` keeps S's edge and the same commit removes, by
  `RemoveEdge`, every other active `supersedes` edge to T, so at most one remains (I6). `value` is refused (usage, exit 2)
  (open point 7).
- **On a violation's key** (staging refs only): `resolve` on G accepts any key of G's staged violations (the `Violation`
  ops of G's newest staged commit), and the `Resolve` sets the key's value; the violation is not "resolved" by itself
  but re-checked when `merge --continue` re-validates (§9.4). This confirms [F19] open point 17's third bullet. On a
  violation's key that holds no conflict value, the sides are the staged operation's (spec sync 2b): `ours` is the key's
  value in state(D₀), where D₀ is the first parent of G's newest staged commit s (§9.4); `theirs` its value in state(P₂),
  or in the revert's or cherry-pick's source state (the state [RULES/merge-table]'s DM row reads as src); `base` its value
  in the base s was computed over (s's recorded `--base`, [F06 §4.4.16], else §4.3's base for (D₀, P₂); for kinds 3 and
  4, the DM row's base). `value` is checked against the key's type. `repoint` of a violation edge replaces the endpoint
  that is not live on the view, and the destination when both are live.
- **A flagged edge** (a retained `blocks` or `gates` out-edge of a tombstone carrying `flagged`, [F08 §10.2];
  [RULES/delete-policy-matrix] FL-005, FL-006) on a work or plan branch is a resolvable key (spec sync 2b): `repoint`
  replaces it by an edge of the same kind from the target to the same destination, without the flag (the `Resolve` sets
  the flagged key `absent` and an `AddEdge` in the same commit adds the new edge; the write path checks I5′ and the
  other edge rules on it, [F08 §8.6]); `drop` removes it. `ours`, `theirs`, `base` and `value` are refused on a flagged
  edge (usage, exit 2), and `drop` is refused on every other key (usage, exit 2).
- **A merge that carries a resolution.** When a merge's result for a key is plain while o holds a conflict value (MR-004,
  RVB-2), the merge commit carries a `Resolve` op with `choice` 3 and `new` = the result (§7.8).
- **A write to a conflicted key** is always a `Resolve` (`choice` 3 for a plain `SET`, [F06 §7.7]); `RESOLVE` statements
  ([50 §3.10] item 6) write on a branch where a conflict value stands, or on a staging ref.
- `resolve --all --policy P` resolves every conflict value of the view with the side P names ([RULES/merge-table]
  `auto-policy`) and leaves violations alone.

### 6.6 The key text form

Every text that names a conflict or violation key — `resolve KEY`, `RESOLVE '<key>'`, the `key` column of
`CALL conflicts()` and `violations()` ([LQ/std]), the `<skey>` of [F19 §10] and [LQ/envelope §5.10] — uses this form:

```abnf
skey          = node-key / edge-key / schema-key / no-key
node-key      = node "." node-part
node-part     = %s"existence" / %s"status" / %s"body" / %s"parent" / %s"observation" / field-name
edge-key      = %s"edge:" node ":" edge-kind ":" node [ ":" anchor ]
schema-key    = %s"schema:kind:" kind-name
              / %s"schema:field:" kind-or-all "." field-name
              / %s"schema:enum:" kind-or-all "." field-name "." enum-name
              / %s"schema:edge:" edge-kind
              / %s"schema:policy:" policy-name       ; [F08 §8.5.6] (spec sync 2b)
              / %s"query:" query-name
no-key        = "-"
node          = [ "#" ] NZDIGIT *9DIGIT           ; #N, 1 .. 4294967295
anchor        = %s"a" NZDIGIT *9DIGIT             ; the anchor handle aN
kind-or-all   = kind-name / "*"
policy-name   = pname                             ; [F14 §7.1]: a [CFG §10.13] row instance, [CFG §3.3]'s canonical key name
query-name    = plain-name / "`" *( qchar / "``" ) "`"
plain-name    = ( ALPHA / "_" ) *( ALPHA / DIGIT / "_" )
qchar         = <any UTF-8 scalar value except "`">
NZDIGIT       = %x31-39
```

- `field-name`, `kind-name`, `edge-kind` (the stored, lower-case edge-kind name) and `enum-name` follow [F08 §8.2];
  `pname` (with its `pseg`) is [F14 §7.1]'s, the one grammar of a policy row's name (spec sync 2b). `#N.<field>` names
  a field or counter key; on input `#N.order` is accepted for the hierarchy key (printed `#N.parent`),
  `#N.resolution` for the status key (printed `#N.status`), and `path`, `oid`, `bytes`, `observed_git`, `observed_blob` and
  `relink` for an artifact's observation key (printed `#N.observation`), because each is part of one merge key
  ([F06 §6.1]).
- **Output** always writes the `#`, the stored names, a `query:` name bare when it matches `plain-name` and back-quoted
  otherwise (a back-quote doubled), and `-` for a violation without a key. **Input** also accepts a node without `#`, so an
  argv key needs no quoting for `#` ([80 §4.2] T1): `resolve 91.body`, `resolve edge:203:blocks:40`.
- The anchor handle `aN` identifies the discriminator of an `at` edge key; it is store-local, like `#N`.
- The words `existence` and `observation` must not be field names ([F08 §8.2]; open point 17).
- An input key that parses but names no conflicted key, no violating key (on a staging ref) and no flagged edge (on a work
  or plan branch, §6.5) of the view is `not_found` (`conflict key`, exit 3); one that does not parse is `usage` (exit 2)
  ([F19 §10.2]).

*(Informative)* `#91.body`, `#12.priority`, `#40.existence`, `edge:#203:blocks:#40`, `edge:#51:at:#812:a17`,
`schema:field:task.estimate`, `query:stale_blockers`, ``query:`Foo bar` ``.

### 6.7 Rendering and export

Conflict and violation lines render as [LQ/envelope §5.10] says; packs show the base text and the resolve hint, never
`<<<<<<<` markers, which appear only in `show --full --markers` ([AR §5a.8]). The image writes `conflict` lines and omits a
conflicted key's ordinary line ([AR §5b.2] rule 3, N12); `Violation` ops, staging refs and `Resolve` choices are never
exported ([AR §5b.4]). The `.moi` key forms and the placement of a provisionally deleted node's `conflict existence` line are
[F14]'s ([RULES/merge-table] open point 5 (d)).

## 7. The typed three-way merge contract

### 7.1 Inputs

A merge is a function of: the base state B (§4.3, or a revert's or cherry-pick's DM row); the states O = state(tip dst) and
T = state(tip src), or for a virtual merge the two states of §5.1; the ancestor sets A(B), A(O), A(T) (§5.2); whether B is
state(tip dst), which makes the merge one-sided (§7.4); the state of the replay start R of §7.4's row "Kleppmann steps";
the (hlc, commit id), net changeset and state of every commit whose step keys that row reads (for a merge, a `sync` or a
virtual merge that is not one-sided, each side's commits since R; for a revert or a cherry-pick of C, C and the commits of
A(O) ordered after C; and, for each two-parent commit among them, recursively, every commit of A(p₂) \ A(p₁)), and the
states of the second parents of the two-parent commits among them ([RULES/merge-table] PR-016; spec sync 3; [AR §11]
OQ-A-11); whether dst is the ref `main` (false inside a virtual merge); the policy override and `--strict` (ignored inside a virtual merge); the
operation (merge, sync, revert, cherry-pick, virtual, foreign-merge import, import merge: [RULES/merge-table] DM rows); the
store's set of dropped bodies ([F06 §8.1]), which only the text rule reads (§7.5; spec sync 3); and the rule tables. It
reads neither the file system nor git ([AR §5a.7] step 6, PR-016).

### 7.2 Keys

- The keys are the canonical keys of item 10 ([AR §4.6]), identified by uid, names and, on `at` edges, the discriminator.
  Inside one store a `ckey` ([F06 §6.1]) names exactly one canonical key, so a merge may work on either form; results
  compare by the canonical key.
- The keys a merge decides are every key whose value is not the same in B, O and T, plus the keys the validators and the
  re-key read ([RULES/merge-table] PR-007, PR-008). A key equal in all three keeps its value.
- The six observation fields of an `artifact` are one key; `status` and `resolution` are one key; `parent` and `order` are
  one key ([F06 §6.1]).

### 7.3 Value equality

Two values are equal exactly when their canonical encodings are byte-equal: [F07 §7.3] states the rule, which is
[RULES/merge-table §2]'s, and this chapter cites it (pass 1, A1-19). On store-local `cstate`s ([F06 §6.2]) this is
equivalent to bytewise equality after these normalisations:

1. a `sym` value becomes a `text` value with the symbol's string ([F08 §5.1]);
2. a plain existence value never carries a node image (`snap` = 0, [F06 §6.2]), so plain `live` values compare by
   (`ex`, `kind`); inside a conflict value, a `live` side's node image is part of the side and compares with it, as
   [F07 §7.3] hashes it;
3. symbol ids, `#N`, store-local enumeration and schema ids compare as the strings, uids and names they stand for — within
   one store equal ids are equal names, so a bytewise comparison suffices;
4. a conflict value compares by class, the three sides and, on an existence key, the provisional side (`prov`).

`absent` equals only `absent`. No other normalisation applies: `f64` values are already canonical ([F08 §5.3]), sets are
sorted and unique, empty values are absent ([F08 §5.3]), and texts compare as exact bytes.

### 7.4 Order keys

| Use | Key | Bytes |
|---|---|---|
| LCA order (§4.2, §5.1) | (gen, commit id) | `gen` as an integer; the 32-byte id bytewise ([F01 §6.6]) |
| Hierarchy moves (RS-007, VM-7) | (hlc, commit id) | `hlc` is the commit's canonical item 3 ([F06 §4.3] order 16), compared as `u64`; never `append_hlc`, which is store-local |
| Kleppmann steps (RS-007) | **Step keys.** A commit's step keys are the hierarchy keys of its canonical net changeset against its first parent ([AR §4.6], the full state diff for a `sync`). A two-parent commit M (a merge or a `sync`) with parents p₁ and p₂ also has each hierarchy key whose value in state(M) differs from its value in state(p₂) and that is a step key of some commit of A(p₂) \ A(p₁), so M's step re-asserts what M kept against the merged branch's moves, and nothing else ([RULES/merge-table] open point 35 case (i), in its narrow form); "is a step key of" is read recursively, so a two-parent commit of A(p₂) \ A(p₁) counts with its own second-parent keys ([AR §11] OQ-A-11 11.3). **One-sided merge.** When the base of a merge or a `sync` is state(tip dst) (the single LCA is tip(dst), `--base` names tip(dst), or both are ε), every hierarchy key takes its value in T, with no replay, and no key is `kleppmann-skipped` (OQ-A-11 11.1 (A); I25′, since dst touched no key since B). Every merge of a branch into `main` is one-sided after its step 0 ([RULES/merge-table] PR-001, VB-017). A virtual merge, a revert and a cherry-pick are never one-sided. **Replay start.** For every other merge or `sync`, and for a virtual merge (VM-7), the replay start R is the greatest commit of A(B) ∩ A(O) ∩ A(T) that every other commit of A(O) ∪ A(T) descends from or is an ancestor of, and ε (the empty state) when no commit is (OQ-A-11 11.1 (B)). The commits that qualify are pairwise comparable, so they form a chain and R is unique; every commit of A(O) \ A(R) and of A(T) \ A(R) descends from R, and its own ancestors since R are steps too, so none is replayed on a state that lacks the history it was made on. R is B's commit when every commit of both sides since a single-LCA B descends from B; otherwise it precedes B and every commit of A(O) ∪ A(T) that does not descend from B. **Merge, sync, virtual merge.** Start from state(R)'s (parent, order) for every node. Each commit of A(side) \ A(R) that has step keys is one step keyed by its (hlc, commit id), setting each of its step keys to its value in that commit's state. For each side, a hierarchy key whose value on that side differs from its value in state(R) while no step of that side sets it is set to that side's value in a step keyed (0, 0), before every commit. **Revert and cherry-pick of C** (DM-003 to DM-005; case (ii)). Start from O's (parent, order) for every node; dst has no step and neither side has a (0, 0) step; src has one step, keyed by C's (hlc, commit id), which sets each hierarchy key whose value in T differs from B (the hierarchy keys of C's net changeset against its first parent) to its value in T, leaving out each key that is a step key of a commit of A(O) ordered after C by (hlc, commit id): that later move stands (MR-040). **Applying the steps.** Steps apply in ascending key order, commit ids compared bytewise, all moves of a step at once; where two steps share a key (a commit that is a step of both sides, or the two sides' (0, 0) steps), dst's moves apply first, then src's. Only a move that changes its node's parent can close a cycle, so only such a move is ever undone: a move that keeps its node's parent, because it sets its node's current value (its value just before the step, case (iii)) or changes only its order (OQ-A-11 11.2), is never undone and never makes its key `kleppmann-skipped`. After a step that leaves a node its own ancestor, the step's moves that changed their node's parent are undone one at a time, the least uid among those moves' nodes that lie on a cycle first, until none is (MR-039); every such cycle holds one of them, because the state before each step is a forest. A key whose last move was undone is `kleppmann-skipped` (CS-013), and a later move of it that applied decides its value (MR-040). Each key's value is its node's final (parent, order). Step keys follow the re-key (RK-005, RK-006); keys of a uid fixed by an existence policy (PR-007) take no part. **Guarantees.** (1) A one-sided merge lands every hierarchy key at its value in T. (2) A revert or a cherry-pick of a commit whose net changeset against its first parent has no hierarchy entry leaves every hierarchy key at its value in O. Outside these cases the steps can still stage a `HierarchyCycle` that the sides' histories do not call for. [RULES/merge-table] open point 35 records the cases the owner decided on 2026-10-06 ([AR §11] OQ-A-6 (a): (i) a two-parent commit inside a side, (ii) a revert or a cherry-pick, (iii) a no-op move in a cycle-closing step; OQ-A-11: (iv) `--base` and a virtual base, through the replay start, (v) the limit of the narrow form, (vi) a move that changes only its node's order), which this row states, and the one still open, E5 of (v): a two-sided merge or `sync` over a history in which a merge's resolution kept a side's cycle-closing move | as above |
| Emission order of conflicts and violations | [F13 §5] VO-2: validator order, then canonical key order ([AR §4.6] item 10) | [F07] |
| Set members and `pathmove` entries | canonical order ([F07]) | [F07] |

### 7.5 The text rule: lines, the line diff and diff3

The rows of merge classes `text` and `section-text` ([RULES/merge-table] MR-028 to MR-038, CS-011, CS-012) use the
functions of this section; the model and the engine implement them from this text ([RULES/merge-table] open point 13).
`moirai-diff`'s histogram line diff (WP-60) implements HD exactly (open point 11).

**Lines.** A text value x is a byte string; `absent` reads as the empty string. lines(x) splits x after every LF (`0A`):
each line is a maximal run of bytes ending with LF, except that a final run without LF is the last line. The empty string
has no lines, and concatenating lines(x) gives x. Two lines are equal when their bytes are equal, LF included.

**The line diff HD(P, Q)** returns a set of matched pairs (i, j), strictly increasing in both coordinates, with P[i] = Q[j].
It is HD(0, |P|, 0, |Q|) of:

```
HD(p0, p1, q0, q1):
  M := {}
  while p0 < p1 and q0 < q1 and P[p0] = Q[q0]:            # common prefix
      add (p0, q0); p0 += 1; q0 += 1
  while p0 < p1 and q0 < q1 and P[p1-1] = Q[q1-1]:        # common suffix
      add (p1-1, q1-1); p1 -= 1; q1 -= 1
  if p0 = p1 or q0 = q1: return M
  cnt(v) := the number of i in [p0, p1) with P[i] = v
  a region is (i, j, len), len ≥ 1, with P[i+k] = Q[j+k] for 0 ≤ k < len, inside [p0, p1) × [q0, q1),
      and maximal: not (i > p0 and j > q0 and P[i-1] = Q[j-1]),
                   not (i+len < p1 and j+len < q1 and P[i+len] = Q[j+len])
  rarity(i, j, len) := min over 0 ≤ k < len of cnt(P[i+k])
  candidates := the regions with rarity ≤ 64
  if there is no candidate: return M
  (i, j, len) := the candidate with the least (rarity, -len, j, i)
  return M ∪ HD(p0, i, q0, j) ∪ {(i+k, j+k) : 0 ≤ k < len} ∪ HD(i+len, p1, j+len, q1)
```

The constant 64 (`DIFF_MAX_RARITY`) is part of format v1: a merged text enters commit ids, so it is never a configuration
key or store parameter. Two maximal regions never share a start, so the choice is unique and HD is a function.

**Complexity** (pass 1, P1-29). With n the number of lines, the region search is O(64 · n) per call and the recursion
depth is at most n, so HD is O(64 · n²) in the worst case (a text of repeated rare lines); prefix and suffix stripping make
the usual edit O(n). Texts and bodies are at most 65,536 bytes ([F08 §5.3]), so n ≤ 65,536. WP-60 (`moirai-diff`)
measured HD and diff3 at that bound: on repeated-rare-line inputs a direct implementation took 1–12 s per HD and up to
23.9 s per diff3, so WP-60 built a faster equivalent of the same function, which takes about 16–22 ms per HD and about
32 ms per diff3 on those inputs, about 40 µs per HD and 78 µs per diff3 on the usual edits, with at most 1.6 MB of scratch
memory. The constant 64 stays and no work cap is needed; how an adversarial text key counts against the merge gate of
[AR §1] row 12 is open point 29.

**diff3(Ob, Ao, Bt)** takes the base, ours and theirs texts. Let O = lines(Ob), A = lines(Ao), B = lines(Bt), MA = HD(O, A),
MB = HD(O, B), and ma(i), mb(i) the partner of O's line i in MA, MB when it has one. From (p, q, r) = (0, 0, 0), repeat:

1. If p = |O|, q = |A| and r = |B|: stop.
2. s := the number of consecutive k ≥ 0 with p + k < |O|, ma(p + k) = q + k and mb(p + k) = r + k.
3. If s > 0: emit the **stable chunk** O[p, p+s); set p += s, q += s, r += s; repeat.
4. Otherwise let j be the least index with p ≤ j < |O| such that ma(j) and mb(j) both exist. If j exists, emit the
   **unstable chunk** (O[p, j), A[q, ma(j)), B[r, mb(j))) and set (p, q, r) := (j, ma(j), mb(j)); else emit
   (O[p, |O|), A[q, |A|), B[r, |B|)) and stop.

An unstable chunk (o, a, b) resolves to b when a = o, to a when b = o, to a when a = b, and is a **conflicting chunk**
otherwise (the comparisons are on line sequences). diff3 is **clean** when no chunk conflicts; its result R is the
concatenation, in order, of every stable chunk's lines and every unstable chunk's resolution.

**The rows' inputs and outputs.**
- `both-diff3-clean` (CS-011) holds when diff3(b, o, t) is clean.
- The removed-text guard (CS-012) uses lines(·) as multisets: it fails when, for S = o or S = t, the multiset lines(S) minus
  lines(R) is not contained in lines(b).
- **Empty result.** R = "" gives `absent`, for a text field and for a body alike: the empty text is never stored
  ([F08 §5.3]).
- **Length.** A result R longer than 65,536 bytes — the bound of a `text` value and of a body ([F08 §5.3], [F08 §7.2]) —
  makes the key a `TextHunk` conflict value instead (b, o, t), as MR-032 would.
- A body result is carried by the merge commit as a body entry ([F06 §8] BD-4), and the key's value is its BLAKE3-128 hash.
- **A dropped body** ([F06 §8.1]) has no readable lines. When b, o or t of a body key is a dropped body and the key's
  case needs diff3 (o and t both differ from b and from each other), diff3 is not run: the key takes a `TextHunk`
  conflict value (b, o, t), whose sides are the three hashes, as the length rule above gives (spec sync 3;
  [RULES/merge-table] CS-011, MR-032, MR-038). Every other case of a body key compares hashes only and is unchanged.

*(Informative)* b = `a\nb\nc\n`, o = `a\nB\nc\n`, t = `a\nb\nc\nd\n`: MA pairs lines 0 and 2, MB lines 0–2; the chunks are
stable `a`, unstable (`b`, `B`, `b`) → `B`, stable `c`, unstable (∅, ∅, `d`) → `d`; clean, R = `a\nB\nc\nd\n`.

### 7.6 The re-key (R-3)

[40 §5.5] as revised by the A1 re-review (S-01; [PLAN] disposition FB-3), [40 §2.11] R-3 and [RULES/link-merge-rules]
RK-001 to RK-011. The re-key is a transformation of one side's state that runs before any per-key rule, in real and
virtual merges alike ([RULES/merge-table] PR-007, VM-6):

1. **When.** For a uid U of a derived-uid kind (`file-key`, [F08 §8.4.7]) that is `absent` in B, that side S holds live
   and not `removed`, and whose final state on the other side is `removed` or deleted ([RULES/link-merge-rules] LC-003,
   LM-007).
2. **uid′** is the file uid of [F08 §11.2] over S's `root` and `origin_path` with predecessor U, re-derived while the
   result names any node of B, O or T — live at another path, `removed`, or a tombstone, as [F08 §11.2] step 4 tests a
   view (RK-003, RK-004).
3. **S′** is S's state with:
   - every key owned by U (value keys, existence, hierarchy and out-edges) moved to uid′, and `origin_pred` = the
     predecessor of step 2's last derivation, which is U unless step 2 re-derived, so that uid′ verifies over its stored
     inputs (I-F2; RK-005);
   - every key owned by U, in S′, set to its value in B (U keeps the other side's state, RK-007);
   - every edge key present in S and absent in B whose **destination** is U — every edge kind, `at` edges with their
     anchors, `produced`, `consumed`, `mentions`, `implements` and the rest — replaced by the same edge with destination
     uid′ (FB-3; [40 §5.5]'s "every edge incident to U that side S added");
   - every existence value `deleted(reason, replaced_by = U)` that S set since B, and every `ref`-typed field value equal to
     U that S set since B, re-pointed to uid′ (open point 25).
4. The per-key merge then runs on (B, S′, the other side). uid′ gets a new `#N` ([RULES/merge-table] RE-012); anchor uids
   do not change.
5. A `sync` whose merge re-keys records every key the transformation changed in its residue, because those keys differ
   from what `main`'s window alone yields ([AR §5a.3], [72 M5]).

[RULES/link-merge-rules] RK-003 to RK-007 and RK-011 state this procedure; RK-006, which re-pointed only anchors in its
first draft, was amended to this edge-complete scope in review pass 1 (S1-14; FB-3; [F08] open point 35; open point 25
here).

### 7.7 Directional rules

A merge's result is a function of (dst, src), not of the unordered pair ([AR §5a.7]). merge(A into B) and merge(B into A)
give equal states, with each conflict value's `ours` and `theirs` exchanged, except at the keys decided by these directional
rows: MR-011 and MR-016 (`main` wins), MR-046 and LM-014 (dst's deletion data), MR-061 (dst's text when both named-query
hashes are equal, [50 §4.4] as amended by S-10), LM-004 (dst's composite on the same path), RS-008 with policy `none`
(dst's state), and the provisional value (§6.3, ours first). This is the "commutativity" of [40 §8.3.2] P8 and GT6
([60 §3.13]) stated exactly (review S-15; open point 12).

### 7.8 From results to ops

The commit a merge emits holds the typed diff between O (dst's state) and the result state ([AR §4.6] "Net changeset =
state diff"; a `sync` stores its residue instead, [AR §5a.3]). For each key k whose result r differs from o:

| o | r | Op ([F06 §7]) |
|---|---|---|
| plain | plain | the key class's ordinary op with `old` = o, `new` = r: `Create`, `Delete`, `Undelete`, `CreateDeleted` (existence; the last for o = absent and r = `deleted`, pass 1, S1-6), `SetStatus`, `SetField`, `Incr` (delta r − o), `SetBody`, `Move`, `AddEdge`, `RemoveEdge`, `SetEdgeProps`, `Schema` |
| any | conflict value | `Conflict` with `old` = o, the class and the three sides (and `prov` on an existence key, §6.3) |
| conflict value | plain | `Resolve` with `choice` 3 `value`, `old` = o, `new` = r |

The net-form rules NF-1 to NF-10 ([F06 §7.8]) then apply: one op per key, image folding, delete folding. A node whose
existence key becomes a `DeleteVsModify` value carries its other keys' provisional changes as ordinary ops (RS-008). The
stored order is [F06 §7.9]'s; conflicts and violations are emitted in [F13 §5]'s order (VO-2).

### 7.9 Violation keys

A staged commit carries one `Violation` op per structural violation ([F06 §7.7]), with the key of this table, so that GT2
compares class and key exactly ([F13 §5] VO-2; open point 19):

| Class ([F19 §12.2]) | `key` |
|---|---|
| `HierarchyCycle` | V01: the hierarchy key of the node whose move was skipped; V05: the hierarchy key of the least uid on the cycle |
| `Cycle` | V03's canonical witness edge ([F13 §5]) |
| `DanglingEdge` | the dangling structural edge |
| `DepthExceeded` | the hierarchy key of the least uid deeper than the bound |
| `Cardinality` | the greatest offending edge key in canonical order |
| `SchemaConflict` | the schema item's key (MR-055, MR-056); for a conformance failure (V09), the first non-conforming key of the least uid |
| `QueryInvalid`, `QueryCycle` | [F19 §12.5] |
| `PlanMask` | the masked key |
| `RemovedTextNotInBase` | the body key |
| `IdCollision` | the existence key |
| `ImageParse` | none (`-`); the description names the image path |
| `NotFound` | the key of the src change that found no target (DM-012) |
| `TombstoneRemoved` | the existence key |

## 8. Records of the VCS operations

### 8.1 Merge-family commits

Each row is one command's records, all in one flushed group ([F05 §4]); `ref`/`ref_id`, `ref_old`, `prev_on_ref` and
`ref_seq` follow [F06 §4.4.2] for the landing ref; markers follow [F13 §4.2] MC-1 (none on staging refs).

| Operation | Records | `kind` | `parents` | Landing ref | `sync_base` | `absorbed` | `origin` | `stmt_origin` ([F06 §3.4]) |
|---|---|---|---|---|---|---|---|---|
| `merge SRC --into DST`, lands | `Commit` (+ `Marker`s) | 1 `merge` | (tip DST, tip SRC) | DST | `id16`(tip SRC) | DST's vector after (RE-005) | — | 4 |
| the same into `main` when SRC is a branch (`work` or `plan`) and tip(main) ∉ anc*(tip SRC) | three cases ([AR §5a.7] step 0, §9.6): the sync lands with no conflict value — the sync row on SRC, then the merge row (or the merge's staging group, §9.2), whose second parent is that sync commit; the sync stages — only the sync's staging group (§9.2); the sync lands but leaves conflict values on SRC — the sync row alone, in its own group, and the merge is refused `conflicted_src` ([RULES/merge-table] PR-003) with nothing written for `main` | | | | | | | |
| `sync` (merge `main` into L), lands | `Commit` (the residue) | 2 `sync` | (tip L, tip `main`) | L | `id16`(tip `main`) | L's vector after | — | 4 |
| `revert C [--onto R]`, lands | `Commit` | 3 `revert` | (tip R) | R | — | — | C | 0 (`revert`) |
| `cherry-pick C [--onto R]`, lands | `Commit` | 4 `cherry-pick` | (tip R) | R | — | — | C | 0 (`cherry-pick`) |
| any of the above, staged | §9.2 | as landing | as landing | the staging ref | as landing | §9.2 | as landing | as landing |
| `resolve` on a staging ref or a branch | `Commit` (`Resolve` ops; a `SupersedeFork` companion `RemoveEdge`) | 0 `ordinary` | (tip of the ref) | that ref | — | — | — | per [F06 §3.4] (a verb or `TX`) |
| `merge --continue`, lands | `Commit` on DST, then `RefUpdate` reason 2 and `RefTable` deleting the staging ref (§9.4) | the staged kind | §9.4 | DST | as the staged | DST's vector after | as the staged | 4 |
| `merge --continue`, re-stages | `Commit` on the staging ref (§9.4) | the staged kind | §9.4 | the staging ref | as the staged | as the staged | as the staged | 4 |

- No fast-forward: a merge always writes a merge commit with two parents, also when tip(DST) ∈ anc*(tip SRC). A merge
  whose tip(SRC) ∈ anc*(tip DST) writes nothing and succeeds (exit 0, "already merged").
- `SRC` is a ref of kind `work`, `plan`, `tag`, `import`, or an `orphans` ref of a branch; `DST` a branch; SRC ≠ DST.
  A merge of a revision that is not such a ref is refused (usage, exit 2; hint: `cherry-pick`) (open point 13). Step 0
  (the sync first) applies only when SRC is a branch: a tag, `import/*` or `orphans/*` SRC merges into `main` directly
  over §4.3's base, and nothing is ever written on it (spec sync 2b).
- The absorbed vectors follow [F11 §3.4] and [RULES/merge-table] RE-005. The pin a merge into `main` takes on its checkpoint
  set ([AR §5a.7] step 8; `Pin` holder 3, [F05 §9.8]) is written when that set exists, by [F16]'s rule, not in the merge's
  group.
- A foreign two-parent commit imported as a merge ([AR §5b.6] step 3) has kind 1 and the git parents' commits as parents;
  its records are [F14]'s.

### 8.2 Ref moves without a commit

`branch` and `tag` creation, deletion, `undo` and `op restore` write `RefUpdate` records with reasons 1–4 and a `RefTable`
record in the same group ([F05 §9.2], [F05 §9.10]); `checkout` writes a `ClientHead` ([F05 §9.3]).

- A new branch, tag or staging ref is created at its fork commit: `RefUpdate` reason 1 with `old` zero and `new` = the fork
  commit (zero for `main` at `init`); its `RefTable` entry has `tip` = `fork_commit` = that commit.
- **Parking on `orphans/<R>`.** Every replay treats a commit whose ref CAS failed as parked (I27′), and the first
  appender whose scan meets it moves `orphans/<R>` (created on its first use) to it, before its own group ([F16] P-70);
  the commit record itself is unchanged ([F06 §4.4.2]). The move is a `RefUpdate` with reason 5 `park` (`old` =
  the previous orphans tip or zero, `new` = the parked commit), which [F05 §9.2] defines and [F16] P-70 writes (pass 1,
  P1-3, S1-11, A1-11; open point 14).

## 9. Staging refs

### 9.1 Names, kinds and pairs (I41′)

- A merge or sync that stages uses `merge/<dst>/from/<src>`: `merge/main/from/lane/l5np`, and a sync of lane L
  `merge/<L>/from/main` ([AR §5a.7] step 8). A revert or cherry-pick of C onto R that stages uses
  `merge/<R>/from/c<64 hex of C>`. An import stages on `import/<ref>` ([AR §5b.6] step 4).
- There is at most one live staging ref per (dst, src) name. While it lives, a second merge of the same pair is refused
  (`staging_exists`, exit 6, [F19 §10.2]); a `sync` of L is refused only while `merge/<L>/from/main` lives; every other pair
  proceeds (I41′, CM5).
- A staging ref is never a merge dst or src, never exported, never pinned by a tag, and never the target of a plain write
  (§3.7).

### 9.2 Creating a staging ref

In the group of the staged commit, in this order:
1. `RefUpdate` reason 1 creating the staging ref G at `new` = tip(dst) — a fork of dst at its tip — with dst's absorbed
   vector plus `absorbed_G[dst] = ref_seq(tip dst)` ([F11 §3.4] fork rule);
2. `RefTable` with G's entry (kind 3, `fork_commit` = tip(dst));
3. the staged `Commit` on G: the kind, parents, `sync_base` and `origin` of the landing commit it stands for (§8.1); its ops
   are the merged candidate's net changeset against tip(dst) with every `Conflict` op and one `Violation` op per structural
   violation (§7.9); `ref_old` = `id16`(tip dst) (bit set), `prev_on_ref` absent, `ref_seq` = 1; for kinds 1 and 2, the
   absorbed vector G has after the landing rule of RE-005, and the command's `--base`, policy override and effective
   `strict` in the `stage` group ([F06 §4.4.16]; the group is absent when there was no `--base`, no override and `strict`
   was false; a staged step-0 sync records the merge's override and `strict` but never its `--base`, §9.6). It emits no
   marker (RE-004).

G's view is the staged candidate: tip(dst) with the merged changes and the conflict values.

### 9.3 Resolving on a staging ref

`resolve KEY …` and `RESOLVE` statements on G append `ordinary` commits to G, each with one parent (tip G) and `Resolve`
ops (§6.5); `resolve --all --policy P` appends one. Only `Resolve` ops and the companion ops §6.5 puts in the same commit
may appear in commits on G after its staged commit (E305 otherwise, [LQ/errors]): the `Move`, `AddEdge` and
`SetEdgeProps` of a live existence side's restore, the `AddEdge` of a `repoint`, and the `RemoveEdge` of a
`SupersedeFork` resolution.

### 9.4 `merge --continue`

`merge --continue [SRC [--into DST]]` names a live staging ref G; without names, the caller's branch must be the dst of
exactly one live staging ref, else usage (exit 2) ([AR §5a.7] step 8). **G's commits** are the commits whose `ref` is G, in
`ref_seq` order; its staged commits are those of kind 1–4 among them. Let s be G's newest staged commit, D₀ its first parent,
P₂ its second parent (kinds 1, 2) or its origin (kinds 3, 4), and D₁ = the current tip(DST).

1. **Candidate.** The operation s stands for is computed afresh against D₁ with s's own arguments ([F06 §4.4.16] `stage`:
   its `--base`, its policy override and its `strict`; without the group, none, none and false), never the continue's
   configuration: for kinds 1 and 2, the merge of P₂ into D₁ over the recorded `--base`, else over the base of §4.3 for
   (D₁, P₂); for kinds 3 and 4, the revert or cherry-pick of the origin onto D₁. When D₁ = D₀ this equals s's candidate.
2. **Overlay.** The **resolved keys** are every key that a `Resolve` op of G's commits names, `new` equal to `old`
   included ([F06 §7.8] NF-3), and every key of a companion op in the same commit (§9.3: `Move`, `AddEdge`, `RemoveEdge`,
   `SetEdgeProps`). For each resolved key k, take the newest commit of G that resolved it, and let D_k be the first parent
   of the newest staged commit of G that precedes that commit. The resolution applies — k takes its value at tip(G) — when
   k's value in state(D₁) equals its value in state(D_k); otherwise it is **stale**: k keeps the candidate's value and the
   command prints one notice per stale key.
3. **Validate** the overlaid candidate in I37′ order ([F13 §5]) over the merge's own states: step 1's base, state(D₁) as
   dst and state(P₂) as src (for kinds 3 and 4, the DM row's base and source). On a key the overlay set (an applying
   resolution), step 1's conflict value, the typed rules' violations and V01's skipped move are dropped. Every step-1
   structural violation whose key the overlay did not set is kept: `IdCollision`, `TombstoneRemoved`, `SchemaConflict`,
   the `stage-take-o` rows, DM-012's `NotFound`, and V01's Kleppmann skips (V01 reports step 1's skipped moves less the
   keys the overlay set). V01 to V13 then check the overlaid candidate as usual.
4. **Land or re-stage** by [RULES/merge-table] `land-or-stage`:
   - land: one `Commit` on DST with parents (D₁, P₂) for kinds 1 and 2, or (D₁) with `origin` for kinds 3 and 4, its net
     changeset against D₁, its markers and absorbed vector; then `RefUpdate` reason 2 and a `RefTable` entry deleting G;
   - re-stage: one `Commit` on G with the same parents and fields, `ref_old` = `id16`(tip G), holding the overlaid
     candidate's net changeset against D₁ and its `Violation` ops; G stays.

This is [RULES/merge-table] PR-014 ("re-runs steps 5–8 against the current dst tip with the staged resolutions as an
overlay") made exact for a dst that moved after staging: no resolution silently overwrites a later dst change (open point 23).

### 9.5 `merge --abort`

`merge --abort [SRC [--into DST]]` names G as `--continue` does and writes `RefUpdate` reason 2 and a `RefTable` entry
deleting G. Nothing else changes: dst never moved and no marker was written (RE-004). G's commits stay reachable through its
reflog until they expire ([F17 §11.2]).

### 9.6 Compound and special stagings

- **Sync-first** ([AR §5a.7] step 0; SRC a branch only, §8.1). Step 0 runs with the merge's policy override and
  effective `strict`, over §4.3's base for (tip SRC, tip `main`); the merge's `--base` applies to the merge only. A
  merge of SRC into `main` whose sync stages writes only the sync's staging group on `merge/<SRC>/from/main`; nothing is
  written for `main`, and the command reports the merge as staged by its sync ([F19] `staged`). A sync that lands with
  no conflict value and a staged merge write the sync commit on SRC and the merge's staging group on
  `merge/main/from/<SRC>` in one group. A step-0 sync that lands but leaves conflict values on SRC is appended on SRC
  alone, in its own group; the merge is refused `conflicted_src` ([RULES/merge-table] PR-003, [API §11.7]), and nothing
  is written for `main` (spec sync 2b).
- **Revert and cherry-pick** stage on `NotFound` ([RULES/merge-table] DM-012) as in §9.2, with the staged commit's kind 3
  or 4, one parent (tip R) and its origin; `DATA` cases land as conflict values (§6.1).
- **Imports.** An import that finds violations appends the imported chain on `import/<ref>` and stops there ([AR §5b.6]
  step 4); `resolve` writes on `import/<ref>` as on G (§9.3), and `merge --continue import/<ref> --into <ref>` merges it as
  src (kind 1). A clean divergent import lands through `merge import/<ref> --into <ref>`, whose staging, if any, is
  `merge/<ref>/from/import/<ref>`. [F14] owns the import's own records.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [80] X-F9 | P11 (b) complete: the ref-name grammar (one grammar, LQ's `ref_name`), device-name and `.lock` segments refused, NFC normalisation of ref-name input, the fold-equality refusal (byte equality under §2.1), with the namespaces, the literal-shape and reserved-segment rules, the length bound and the prefix rule. P11 (a), the hashed `schema/queries/<q>.moi` names, is [F14]'s (its primitives are [F01 §6.4], §7.1) | §2 |
| [60 §2.5] Cross-platform row: "named-query file names and the ref-name rule" | the ref-name rule; the file names are [F14]'s | §2.1, §2.4, §2.5, §2.7 |
| [60 §3.1] item 1 and [60 §3.4]: the recursive-virtual-base addendum to [AR §5a.7] | complete: construction, inner bases, the virtual merge, conflict-valued bases (both sides equal → clean), termination, cycles, criss-cross, determinism, the required cases | §4.3, §5 |
| [AR §5a.8] taxonomy (with [60 §2.5] "Ops and values" `Conflict`/`Violation` ops) | the value-conflict codes 1–7 and their placement, the provisional value, flat sides, resolution, the key text form, violation keys; the structural and hint codes are [F19 §12.2]–§12.3's, the ops' bytes [F06 §7.7]'s | §6, §7.9 |
| [50] F11 (`CONFLICTS`) | the class codes its rows carry and `n` = 0 for schema keys; the row is [F11 §10]'s | §6.1, §6.3 |
| [50] F18 | the text form of the keys of `QueryInvalid` and `QueryCycle` violations (`query:<name>`); the classes and the validator are [F19 §12]'s | §6.6, §7.9 |
| [40] R-2 | the placement of `FieldEdit` and `PathClaim` on the `observation` composite key and the provisional composite; the merge-class enumeration is [F08 §8.4.1]'s | §6.2, §6.3 |
| [40] R-3 | the merge re-key with its edge-complete re-pointing and its record in a sync's residue (FB-3); the derivation is [F08 §11.2]'s, the rows [RULES/link-merge-rules]' | §7.6 |
| [40] R-4 | only the anchor handle in the key text form of an `at` edge; the anchor record is [F08 §10.3]'s, which [F06 §7.5]'s ops carry | §6.6 |
| [60 §2.5] [AR] row "Commit body" | only the values the VCS operations give `ref`, `ref_old`, `prev_on_ref`, `ref_seq`, `sync_base`, the absorbed vector, `origin` and `stmt_origin`; the bytes are [F06 §4]'s | §8.1, §9.2, §9.4 |
| [60 §2.5] [AR] row "Log" (`RefUpdate`, `RefTable`) | only which operations write them and with which reason; the payloads are [F05 §9.2], §9.10's (reason 5 proposed) | §8.2, §9 |

No R-1, R-5 to R-18, F1 to F10, F12 to F17, other X-F item or [90 §10.1] item is specified here.

## Holes

None. No value of this chapter is decided by an M0 measurement or benchmark. The two constants it fixes — the ref-name
bound of 128 bytes (RN-6) and the line diff's `DIFF_MAX_RARITY` of 64 (§7.5) — are chapter decisions under [F01 §2.4]
rule 3 (open points 11 and 13), not measured values.

## Open points for the review

1. **What this chapter closes.** [PLAN §3.3]'s WP-12 gaps (the import-checkpoint kind, the foreign `hlc` unit, the
   length-prefix scheme and value encodings, F10/F14/F16/`actor_src`) are closed in [F06] and [F07], not here. Of the gap
   "stale [60 §2.5] copy of R-7, R-8, R-10 and R-11", none of those rows is specified here; the R-rows this chapter uses,
   R-2 and R-3, are taken from [40 §2.11] (and [60 §2.5]'s R-3 row, which the A1 re-review restated, agrees). This chapter
   closes the obligations `a1-dispositions.md` §5 gives chapter 12: the ref-name grammar and the refused literal-shaped
   segments (S-11, §2.1, RN-3), the edge-complete re-key and its residue (FB-3, §7.6), and dst's text on equal named-query
   hashes (S-10: confirmed as [RULES/merge-table] MR-061, §7.7); and review S-15 (§7.7). It answers [RULES/merge-table] open
   points 9 (the `SupersedeFork` key form), 13 (the diff), 14 (the guard's multisets, over §7.5's lines), 18 and 19 (merging
   over a conflict-valued base; the virtual-merge details) and 20 (`DATA`), and [F11] open point 7.
2. **Conflict: [60 §3.4]'s sentence against I25′** (§5.4). [60 §3.4] and [AR §5a.7] step 1 say a key whose virtual-base
   value is a conflict value "conflicts whenever they differ". Read literally, a side that still holds exactly the virtual
   base's conflict value (it never resolved k) against a side that resolved k would conflict, although that side did not
   touch k since the base — which I25′ forbids — and although the same situation at a real LCA takes the resolution
   ([RULES/merge-table] MR-004). RVB-2 and RVB-3 follow I25′; RVB-1 and RVB-4 keep every GT6 property of [60 §3.13] (equal →
   clean; resolved differently → conflict; agreed and untouched → clean). Decided in review pass 1 (S1-15): [RULES/merge-table]
   runs MR-002 after MR-003 and MR-004 with CS-006 restricted to the `both` case, [F13 §3.5] restates I31′, [60 §3.4],
   [60 §3.13] GT6 (VBC-3) and [AR §5a.7] step 1 are edited at WP-81a, and the owner re-signs the table (V3).
3. **Flattening and MR-005** (§6.4). A conflict value's sides stay flat ([F06] open point 21). RVB-4 uses flat(v) = the
   side's provisional value; the side's own conflict value stays in the `Conflict` op's `old`. Proposal for R-MODEL:
   MR-005 (a plain base, both sides changed, one of them holding a conflict value), today a `gap`, takes the same form —
   a conflict value {class from the key's value rows on (b, flat(o), flat(t)), else the conflicted side's class; base b;
   ours flat(o); theirs flat(t)} — so no merge input leaves the model in `SpecGap`. **Pass 1 (P1-21): adopted** as the
   normative rule of §5.4's last paragraph. **Done** (round 1): [RULES/merge-table] MR-005 (`conflict-plain-base`) with
   RS-015 states it. **Decided 2026-09-28** (OQ-M-1 (a)): the owner accepted the changed rows; the re-signature in
   `rules/SIGNED.md` follows under V3.
4. **The provisional value and the `prov` byte** (§6.3). Readers, indexes, derived predicates and I26′'s `term()` need a
   plain value of a conflicted key, which no chapter defined. For every key but existence it is derived from the sides
   (`ours`, else `theirs` when `ours` is `absent`), which also covers [RULES/merge-table] RS-011 ("the modified definition
   stays"). For an existence key it depends on the policy in force, which `--policy` and `merge.policy.<kind>` override, so
   a byte is needed: [F06 §6.2] is asked to add `prov u8` after `theirs` in a conflict `cstate` of an existence key, and
   [F07] to hash it with the conflict value (it is state: the image already shows it as a tombstone file or a live file).
   Without it the policy override could not be replayed. `CONFLICTS` rows need no change (they hold the conflict; the node
   row holds P). Owners: WP-12a ([F06]), WP-12b ([F07]), WP-15 ([F14]). Known limit: a `PathClaim` raised on a composite
   that the merge composed (LM-005) has dst's composite as its provisional value, not the composed one; the claim is
   settled by observation anyway ([RULES/link-merge-rules] LV rows), and keeping the composed value would need a fourth
   stored side. **Pass 1 (S1-5, A1-9): done** — [F06 §6.2], [F06 §7.7] and [F07 §7.3] carry `prov`; [F11 §10] stores it.
5. **`DATA` gets no code** (§6.1). Conflict inside [AR]: §5a.8 lists `DATA` as a class; I34′, §2.7 and §5a.5 record it as
   `FieldEdit`. This chapter follows the invariant and [RULES/merge-table] DM-013; [F19 §12.1]'s code-range row is asked to
   drop `DATA` from its list. The alternative, a code 8 `DATA` that revert and cherry-pick emit, would change I34′.
6. **Numbering.** Codes 1–7 follow [AR §5a.8]'s order without `DATA`; `PathClaim` is 7. Codes 8–63 stay free for later
   format versions.
7. **`SupersedeFork`** (§6.2, §6.5; [RULES/merge-table] open point 9). The key is the src side's superseder edge with base
   and ours `absent` and theirs its edge value; its provisional value keeps both superseders present. `--take theirs` also
   removes the other active superseders of the target, and `--value` is refused. The `conflicted` flag follows ownership
   (§6.3), so only the src-side superseder is flagged; the merge table's proposal to flag both would need a derived rule
   beyond ownership. [F13] states that I6 does not hold for a target while a `SupersedeFork` value on one of its
   `supersedes` edges stands, as the design lets a `PathClaim` value stand against I-F1. **Confirmed** (pass 1, round 1):
   [RULES/merge-table] open point 9 takes this ownership rule, so only the src-side superseder is `conflicted`.
8. **Virtual merges ignore automatic policies** (VM-2). An automatic `ours`/`theirs` inside a virtual merge would pick one
   LCA's value by generation order — the silent choice the virtual base exists to prevent — and a flag of the outer
   command would make the base depend on the command line. **Done** (pass 1, round 1): [RULES/merge-table] VB-019
   (`auto-policy=ignored`), which §5.3's VM-2 row cites.
9. **Hierarchy order uses the canonical `hlc`** (§7.4), never `append_hlc`, which differs between stores and would make an
   imported merge recompute differently (I30′). "Newest" is the greatest (hlc, id).
10. **Zero LCAs → empty base** ([RULES/merge-table] VB-003), at every level of the recursion (§4.3, §5.1). git refuses
    unrelated histories by default; the design is silent, and the empty base turns every two-sided key into a conflict,
    which is the conservative reading.
11. **The line diff is specified here** (§7.5). The histogram diff of [AR §2.10] T10 is heuristic; model and engine agree
    only on a function stated in the specification. HD strips the common prefix and suffix, then splits at the maximal
    matching region of least rarity (≤ 64, git's histogram chain limit), longest, then leftmost; with no candidate it
    matches nothing (git falls back to Myers; that would need a second specified algorithm). WP-60's `moirai-diff`
    implements HD exactly, and its other uses (the replay's "full histogram-diff mapping") may use it as is. The diff3
    chunk walk and resolution are the classic ones (Khanna, Kunal and Pierce's formalisation). The empty-result and length
    rules are this chapter's. **Spec sync 2a** (WP-60 review): taking the least j among the rarity-1 regions (the
    order (rarity, −len, j, i)) can split a box far off its diagonal, for example at a unique line that moved from one
    end of the text to the other. That gives more conflicting chunks than git's histogram diff would, but never a wrong
    merge: HD still returns a valid common subsequence, and diff3 resolves only chunks where one side equals the base.
12. **Directional rules** (§7.7, review S-15). GT6's "merge commutation" and [40 §8.3.2] P8 are equality up to the listed
    directional rows and side exchange; the alternative the review offered (a state order for the same-path composite)
    would change [40 §5.5]'s "take dst's composite" and is not adopted.
13. **Ref-name decisions** (§2). (a) Every ref name has a namespace; `feature-x` alone is refused. (b) User parts may have
    several segments (`tags/release/1.0`), so RN-8 (the prefix rule git's loose refs need) is required. (c) RN-6's 128
    bytes keeps a loose ref's path within Windows' 260-character limit for an image repository at an ordinary depth; it is
    a chapter decision. (d) A merge's src is a ref; merging an arbitrary commit is refused, because the absorbed vector and
    the staging name need a ref (cherry-pick applies one commit). (e) **Spec sync 2a** (WP-61 review): IN-1's NFC step
    cannot change which names RN-1 accepts (checked against the UCD singletons), so M0 refuses non-ASCII directly and an
    error quotes the input rather than its normalised form; a later format that admits non-ASCII ref names needs NFC's
    decomposition and composition tables.
14. **`RefUpdate` reason 5 `park`** (§8.2) for [F05 §9.2] and [F16]: the park of `orphans/<ref>` is a non-commit
    move ([AR §5a.2]) that no reason 1–4 describes. **Pass 1 (P1-3, S1-11, A1-11):** [F05 §9.2] defines reason 5; §8.2
    cites it. **Closed** (round 1): [F05 §9.2] has reason 5 (`old` zero when it creates `orphans/<R>`), [F05 §9.10] the
    park's partial upsert, and [F16] P-70 cites the bytes.
15. **Namespace completion** (IN-3) is a convenience of `branch`, `checkout --branch-new` and `tag` only; revisions are
    never completed, so a revision never silently names a different ref.
16. **Error codes for [F19 §10.2]** (all exit 2, X-F9 fixing exit 2 for the fold rule): `bad_ref_name` — `<name> is not a
    valid ref name: <reason>`, reasons `use lower-case a-z, 0-9, _ and - in segments joined by . and /` (RN-1),
    `branches start with lane/ or plan/, tags with tags/` (RN-1, IN-3), `--kind <k> does not match <prefix>` (IN-3),
    `the segment <s> is reserved` (RN-2), `the segment <s> reads as a commit or sequence number` (RN-3), `the segment <s>
    is a Windows device name` (RN-4), `the segment <s> ends in .lock` (RN-5), `it is longer than 128 bytes` (RN-6);
    `ref_exists` — `<name> already exists` (RN-7); `ref_prefix` — `<name> and <other> cannot both exist: one is a prefix
    of the other` (RN-8). [F19] owns the final texts. **Pass 1 (A1-39):** [F19 §10.2] adds the three codes.
17. **Reserved field names** (§6.6). [F08 §8.2]'s field-name uniqueness list gains `existence` and `observation`, so
    `#N.<name>` is unambiguous. The other key words (`status`, `body`, `parent`, `order`) are already common fields.
18. **`CONFLICTS.n` for schema keys** (§6.3): [F11 §10] defines `n` as the node's `#N`; a conflict on a schema item or a
    named query has no node, and 0 (never a `#N`) is proposed. The row order (`n`, key bytes) then puts schema conflicts
    first. **Closed** (pass 1, round 1, S1-7): [F11 §10] takes `n` = 0 for schema keys, whose rows fold with `SCHEMA`
    ([F09 §8.3]).
19. **Violation keys** (§7.9) are fixed here because GT2 compares keys exactly and only [F19 §12.5] (F18's classes) and
    [F13 §5] V03 (the `Cycle` witness) had chosen one. [F13] and [F19] cite §7.9.
20. **Revision edge cases** (§3.4, §3.5). A reflog suffix is valid only directly on a ref name or on `HEAD` resolving to a
    ref; a deleted ref's name resolves only with a reflog suffix, to its newest deleted row; `REF@T` answers only where the
    held moves prove the answer (for T inside the reflog window, no dropped move can lie after T). [LQ/errors] E301 needs
    two texts: `<revspec>: a reflog suffix follows only a ref name or HEAD` and `<ref>@<n>: <ref> has <k> recorded moves`.
    [LQ/canonical-ast] keeps nesting `rsuf` nodes; the binder refuses what §3.5 refuses. [F10] must keep `RefUpdate`
    records (or their moves) for the reflog window when it retires an extent, or `@n` and `undo` lose moves early.
21. **Import refs accept `RESOLVE`** (§3.7, §9.6). [50 §3.9] item 6 lists `import/*` as read-only, while [AR §5b.6] step 4
    finishes a staged import with `resolve` and `merge --continue` "like a local merge". Import staging is [AR]'s (not an
    [50] reservation), so [AR] is followed; [LQ/errors] E305's `an import ref` case then applies to plain writes only.
    **Spec sync 2a:** E305 now reads `an import ref accepts only RESOLVE`, and [50 §3.9] item 6 is corrected at WP-81a.
22. **Views of tags and other non-branch refs** (§3.7) are commit views, so runtime relations are E302 there ([50 §3.9]
    item 4: runtime relations exist "only at a branch tip"). A range's base with several LCAs is the virtual base, and
    [LQ/errors] N05 needs a form that names no single LCA commit (for example `their merge base (<k> LCAs)`).
23. **`merge --continue` when dst moved** (§9.4). [AR §5a.7] step 8 and PR-014 re-apply the staged resolutions "against the
    current dst tip"; the design does not say what happens to a key dst changed after staging. §9.4 recomputes the
    candidate and drops a resolution whose key dst changed since the tip it was made against, with a notice; when dst did
    not move the result equals PR-014's. **Done** (pass 1, round 1): [RULES/merge-table] PR-014 states the same rule.
24. **The staging ref's vector and fork fields** (§9.2): a staging ref is a fork of dst at its tip, so its view, its
    absorbed vector and its first commit's `ref_old` follow the ordinary fork rules; staging commits emit no markers
    (RE-004), so the vector only serves reads of the staged view.
25. **Re-key scope** (§7.6). FB-3 names edges, `replaced_by` references and the node's own out-edges; this chapter also
    re-points `ref`-typed field values equal to U that the re-keyed side set, because leaving them on U is the same wrong
    answer S-01 describes for edges. [RULES/link-merge-rules] RK-006 now has this edge-complete scope and RK-011 the residue
    (review pass 1 S1-14); [F08] open point 35 is closed by §7.6.
26. **`fork_ref_id` 0 in [F11 §3.1]** means "no fork", while `main` has `ref_id` 0, so a fork from `main` also stores 0.
    `fork_commit` (zero only without a fork) disambiguates; [F11] may state it.
27. **Empty text** (§7.5). [F06 §5.2] calls empty text "a value distinct from `absent`", while [F08 §5.3] says "the empty
    text is never stored: an empty value is an absent field". The text rule follows [F08] (an empty diff3 result is
    `absent`), which also makes "`absent` reads as the empty text" symmetric; [F06] and [F08] settle the conflict between
    them. **Pass 1 (P1-1, S1-1): closed**: empty text is absent in every stored form ([F08 §5.3]).
28. **Pass 1 changes** (S1-6, S1-22, A1-19, A1-46, P1-29). §7.8 emits [F06]'s `CreateDeleted` for a key that goes from
    absent to deleted; §6.5 restores a taken live side's hierarchy key and out-edges from that side's state at the
    introducing commit (images stay value keys only); §7.3 cites [F07 §7.3] as the one equality rule; §6.2 uses the
    renumbered `ckey` classes; §5.5a charges virtual-base work to the merge's budgets and §7.5 states HD's complexity for
    WP-60 to measure.
29. **An adversarial text key against the merge gate** (§7.5; spec sync 2a, WP-60 review). WP-60's faster HD takes about
    32 ms for one diff3 of repeated rare lines at the 65,536-byte bound, against the ≤ 50 ms for the merge of a 2k-op lane
    at 1e5 of [AR §1] row 12 (GT11). One such key thus takes most of a merge's budget, and two exceed it. To decide
    before GT11's merge row is written (M1): whether GT11 measures workloads only (the 2k-op lane of ordinary edits, where
    a text key costs about 78 µs) and an adversarial text is outside the gate, or whether a merge needs a per-merge text
    budget, which would have to be a format-fixed rule (a merged text enters commit ids, so a budget that changes the
    result could never be a configuration key; one that only reports cannot change the result).
30. **Spec sync 2b** (WP-91 review, fix and closure). (a) **`merge --continue` re-uses the staged merge's arguments**
    (§9.2 step 3, §9.4 step 1; [F06 §4.4.16]). Contested between recording `--base`, the policy override and `strict` with
    the staged commit and stating that continue drops them (the model's reading). Recorded, because a continue is the same
    merge: dropping `--base C` recomputes against another base than the one the staged resolutions were made against, and
    dropping `--policy delete-wins` reopens a `DeleteVsModify` the user had settled by policy, both silently. The bytes
    live in the staged commit, not in G's `RefTable` entry, so no runtime table ([F11]) changes. (b) Step 2's resolved
    keys include a `Resolve` whose `new` equals `old` (NF-3's exception) and the keys of its companion ops; step 3
    validates over the merge's own states and keeps every step-1 structural violation the overlay did not settle; §9.3
    admits §6.5's companion ops; §6.5 names the sides of a violation key and adds flagged edges (`repoint`, `drop`) on work
    and plan branches. (c) **RVB orientation** (§5.4): contested between an exchange-aware equality and keeping byte
    equality with a one-orientation VBC-3. The exchange-aware ≈ is adopted, since a side that never touched k must not
    conflict because of the order of the LCAs' ids (I25′), and it keeps RVB-1 to RVB-4's meaning. (d) **Kleppmann steps**
    (VM-7, §7.4): contested with [RULES/merge-table] RM-01's one-step-per-order-key reading; per-commit steps adopted, as
    RM-01 recommends, because they contain the one-step reading within a commit and replay a linear chain of
    single-parent commits, merged into a dst that made no commit since the base, without a spurious cycle (R37).
    [RULES/merge-table] open point 35 records, not decided, the cases that can still give a spurious result: a
    two-parent commit inside a side; a revert, a cherry-pick or `--base` whose base is not where a side's commits start;
    and a no-op move undone (spec sync 2b, arbiter ruling on RS-007's forest sentence). **Spec sync 3:** the owner
    decided three of them on 2026-10-06 ([AR §11] OQ-A-6 (a), Option A) and §7.4's row now states them: a two-parent
    commit also has as step keys the keys where its state differs from its second parent's that a commit of
    A(p₂) \ A(p₁) has as step keys (case (i), narrow form); a revert or a cherry-pick starts from O, dst has no step,
    neither side has a (0, 0) step, and src's one step leaves out the keys a later dst commit moved (case (ii)); a move
    that sets its node's current value is never undone and never makes its key `kleppmann-skipped` (case (iii)). The
    row gains the second guarantee (a revert or cherry-pick of a commit with no hierarchy entry leaves O's hierarchy).
    `--base`, a virtual base (iv), the limit of the narrow form (v) and a move that changes only its node's order (vi,
    which the undo can take back although it closes no cycle) stay open in [RULES/merge-table] open point 35, to be
    decided before the merge table's V3 signature and before the engine implements RS-007. (e) Step 0 runs only for a
    branch src (§8.1, §9.6). (f) **IN-3's failing rule** (§2.5): contested between "RN-1 fails" and IN-3; IN-3 adopted,
    as [F19 §10.2]'s reason text and rule list and open point 16 already say (three places against one); only the JSON
    `rule` differed. (g) `schema:policy:` keys ([F08 §8.5.6]). (h) After the independent check of the sync: §8.1's
    sync-first row and §9.6 name the third outcome of step 0, a sync that lands conflict values and is appended alone
    before the merge is refused `conflicted_src` ([API §11.7]). (i) After the independent check: §7.4's row "Kleppmann
    steps" takes the rest of RS-007, and VM-7 cites it: the (0, 0) steps, dst's moves first where two steps share a key,
    the undo one move at a time (least uid on a cycle first) and a revert's or cherry-pick's single src step (the
    origin's, valued in src's state). Without them a revert, whose src state(p₁(C)) has no commit outside A(C), gave src
    no step, so its hierarchy change never applied; and under `--base` or a virtual base a side's value that differs
    from the base while no step of that side sets it (the base is not where that side's commits start) never applied
    either.
31. **Spec sync 3: a dropped body in the text rule** (§7.1, §7.5; [AR §11] #33, OQ-A-7). [F06 §8.1] lets the owner drop
    a body's bytes while every record keeps its hash. A body key compares hashes, so every case but diff3 is unchanged;
    diff3 needs the lines of b, o and t, and a dropped body has none. Decided here: the key takes `TextHunk` (b, o, t),
    as a result over the length bound does, so the user resolves it by hand. The alternatives were refusing the merge
    (it would block every merge of a lane forked before the drop that edited the body) and reading a dropped body as
    the empty text (diff3 would then compute a "clean" result from text that is not the body). The merge's result now
    also depends on the store's dropped set, which §7.1 lists among its inputs. [RULES/merge-table] carries the case:
    PR-016 lists the dropped set among the merge's inputs, and CS-011's `both-diff3-clean` excludes a body key with a
    dropped side, which falls to its class's `both` row, MR-032 or MR-038; no case token is added (SM-030).
32. **Wave 3c: OQ-A-11 in §7.4's row "Kleppmann steps"** (§5.3 VM-7, §7.1, §7.4; [RULES/merge-table] RS-007, open
    point 35; [AR §11] OQ-A-11, decided 2026-10-06). The row gains the one-sided merge (11.1 (A)), the replay start R
    (11.1 (B)), the undo of only the moves that change their node's parent (11.2) and the recursive reading of "is a
    step key of" (11.3); §7.1 lists the new inputs (whether B is state(tip dst), and state(R)); VM-7 takes the replay
    start of each virtual pair. The replay start is stated over A(B), which §5.2 defines for every base, so it also
    covers `--base` and the virtual base, which spec sync 3 left open as (iv); the arbiter's check of the statement is
    `docs/spec/reviews/wave-3c-arbiter.md`. Guarantee (1) of spec sync 2b (a linear chain of single-parent commits
    merged into a dst that made no commit since B) is subsumed by the one-sided merge, which holds for any src history.
    E5 of [RULES/merge-table] open point 35 (v) stays open.
