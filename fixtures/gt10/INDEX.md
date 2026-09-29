# fixtures/gt10: the GT10 command-script fixtures

| | |
|---|---|
| Title | The GT10 fixtures whose expectations a person writes: the "node 40" table on one branch and across branches (every edge policy, flagged blockers, the `deleted` marker that keeps a deleted task undispatched on every branch until the branch absorbs the delete), the [AR §7.6] walk-through, and the merge of counters, natively and through a git-side merge of the image |
| Work package | WP-22, part `gt10/` (R-FIX; [PLAN §3.2] item 1). The part `lq/` is separate (`fixtures/lq/INDEX.md`) |
| Acceptance | GT10 ([60 §3.13]); E1 (the owner verifies the core set of §7, V3); E5 (the model's suites of WP-94 run these files) |
| Separation | S1 and S3 ([PLAN §3.1]): written from the specification text only. The author read no line of `moirai-format-oracle`, `moirai-model`, `moirai-toylog` or any product crate, and ran no project code |
| Sources | [AR §2.5], §3.1–§3.6, §5a, §5b.6, §5d, §7.6; [API] (`docs/spec/store-api.md`: every command, argument, result member and refusal the scripts use); [RULES/delete-policy-matrix], [RULES/state-definition], [RULES/merge-table], [RULES/status-machines], [RULES/role-write-policy]; [F06 §2.3], [F08 §9], [F12 §6], [F14 §6.5], §11.2; [LQ/std §7], [LQ/grammar-v1.ebnf]; [CFG §10] |
| Status | Written against the specification after review pass 1 (owner answers of 2026-09-28) and the wave 2b spec sync. A rule change that moves an expectation updates the cases it touches (commit subject `WP-22:` or `WP-73:`); a disagreement between an implementation and a case is triaged by §6 first |

These files are data. Their consumers are the reference model's suites (WP-94: the node-40 suite on one branch and
across branches, the marker suite, the walk-through), later the engine through `moirai-testkit` (M2: the node-40 table
on `main`; M3: across branches; M5: the image cases; the release gate RG4 runs the walk-through as a script), and the
owner, who verifies the core set (§7). Register incidents are owner data: their expectations live in `/private/gt10/`,
never here ([PLAN §3.2] WP-22).

## 1. Files

| Path | Cases | What it asserts |
|---|---|---|
| `node40.cases` | 13 | The node-40 store `c0` ([RULES/delete-policy-matrix] `n40-nodes`, `n40-edges`) and the one-branch rows of the table: the restricted delete, `--replaced-by` with `--reparent`, the flagged blocker and both ways of resolving it, the `drop-notify` policy, `--cascade`; the `DRY` impact, the keyed retry, and the writes that assumed the deleted node ([AR §5d.3] L3) |
| `node40-branches.cases` | 27 | The same store across `main`, `lane/x`, `lane/y` (and `lane/z`): git-like isolation with the `deleted` marker, delete-wins at merge, `DeleteVsModify`, the staged `DanglingEdge` with its resolution and continuation, the lease refusal and `--release`, `branch -D` with and without a fork that keeps the delete, the revert (`Undelete`), the delete on `main` applied to a lane by `sync`, a historical edge added elsewhere, the same delete on both sides and two different deletes, and the image import of a removed node file |
| `markers.cases` | 15 | The `deleted` marker across five refs: every live ref that has not absorbed the delete excludes the task from `ready`, `claim` and `blocking`, a merge and a `sync` absorb it (transitively through `main`), a fork after the delete holds it, and `branch -D`, `undo` and `revert` clear it; a delete on a `plan/*` ref excludes nothing until a lane forks from it; a staged merge writes no marker |
| `policies.cases` | 82 | Every row of [RULES/delete-policy-matrix] `edge-policy` (EG-001 to EG-064 but EG-023), the preconditions DP-001, DP-002, DP-003, DP-007 to DP-010, the flagged-edge rules FL-001, FL-002, FL-003, FL-009, the steps DS-003, DS-010, DS-011, the tombstone rows TB-001 to TB-013 and the revert rows UD-001 to UD-004, on one store of every edge kind |
| `walkthrough.cases` | 23 | [AR §7.6] steps 1 to 10 as one stream: decomposition on `main`, `lane open`, the architect's `TX` and the critic's findings and gating verdict on the lane, the review loop, a rule on `main` reaching the lane at `sync`, dispatch with store-wide leases, `apply` completing a task on the lane (`settled` elsewhere, never re-dispatched, a replay changes nothing), a `TextHunk` resolved before the sync-first merge, `lane close`, `branch -d`, `run close`, and the image round trip of a colleague's hand edit |
| `counter-merge.cases` | 8 | Counters merge by the sum of both deltas over the base (MR-026): through `main` with sync-first, `reopen_count` from two reopens, a merge whose LCA is a commit both sides merged, and a foreign git merge of the image whose tree lost one side's increments: the import takes the counter from the typed merge, never from the text ([AR §3.4] I30′) |

`.gitattributes` gives `fixtures/** -text`: every byte of these files is kept as written.

## 2. The case format

### 2.1 Framing

The framing is that of `fixtures/lq/INDEX.md` §2.1: a `.cases` file is UTF-8 text with LF line ends, a sequence of cases
`%% case <id>` … `%% end`; every line outside a case is a comment (`#`). A line directive is `%% <name> <value>`; a block
directive is `%% <name>` alone on its line, and its block is every following line up to the next line that starts with
`%% `. Line directives marked repeatable may occur several times. Case ids match `[a-z0-9][a-z0-9.-]*` and are unique
in their file. Only comments, `source`, `note` and `finding` lines hold bytes outside ASCII.

### 2.2 Directives

| Directive | Kind | Meaning |
|---|---|---|
| `from` | line | The case whose stream this case continues: an id of the same file, `<file>:<id>` for another file of this directory, or `-` for a new stream. A case's **stream** is its `from` chain's scripts followed by its own |
| `core` | line | `yes` for a case of the owner-verified core set (§7); absent means no |
| `source` | line, repeatable | The specification sections and rule rows the case rests on |
| `note` | line, repeatable | Commentary for the reader; never parsed |
| `requires` | line | Capabilities beyond the M0 reference model, space-separated (§2.8); absent means none |
| `finding` | line, repeatable | A finding or gap of §6 whose resolution the case's expectations depend on: a mismatch on such a case is triaged as a specification finding first |
| `policy` | line, repeatable | `<ref> <row>=<value>`: the value of a policy-data row of [CFG §10.13] on that ref from the start of this case's script (§2.7) |
| `bind` | line, repeatable | `<label> #<N>`: binds a label to a node that the case's script creates where §2.3 has no binding form (a node created inside LQ text, a root node of R4) and asserts its number |
| `script` | block | The case's commands (§2.3); may be empty, for a case that only states facts about its `from` state |
| `result` | block | Facts about the result of the **last** command of the script (§2.5) |
| `expect` | block | Facts about the store after the script (§2.6) |

### 2.3 Command lines

A command line is one command envelope of [API §3.1]:

```
<n> <Command> [<mutation name>] <token> ... [=> <label>]
```

- `<n>` is the envelope's `n`: the command's position in the stream, from 1, increasing by 1 along the `from` chain. Sibling
  cases that continue one case reuse the same numbers, since each is its own stream.
- `<Command>` is a command name of [API §6]–§14 (`Init`, `Tx`, `Mutation`, `Merge`, …).
- A token `@<key>=<value>` is a member of `ctx` ([API §4.1]); a dotted key sets a nested member (`@env.CLAUDECODE="1"` is
  `ctx.env.CLAUDECODE`). Every other token `<key>=<value>` is a member of `args`.
- `Mutation` takes the named mutation's name as its first word (`Mutation tx.rm …`); its `key=value` tokens are members of
  `args.params` ([LQ/std §7.2]), except `message` and `move_lease`, which are members of `args`.
- `=> <label>` binds the label to the node the command created: the node of `tx.add`, `tx.remember` or `tx.answer`, the lane
  node of `LaneOpen`, the run node of `RunOpen`, the file node (`file` of the first yield row) of `LinkFile`.
- **Statement lines.** A `Tx` (and the `stmts` of `Apply`) may be followed by lines indented by two spaces, each one
  data-level statement of [API §9.2]: `<op> <key>=<value> ...` (`create`, `set`, `link`, `unlink`, `move`, `delete`,
  `resolve`, …). A `create` with `as=<label>` binds the label to the node it creates.
- **LQ text.** A `Tx` token `lq=|` takes as its value the following lines indented by four spaces, with those four spaces
  removed, joined by LF (no final LF).
- **Harness steps.** A line `- <Step> <token> ...` without a number is not a Store API command: it is a step the harness
  performs outside the store, with the git CLI in a scratch clone of an image repository (§2.9). It takes no `n`.

### 2.4 Values

| Form | Denotes |
|---|---|
| `"..."` | a JSON string (ASCII, with `\"`, `\\`, `\n` and `\uXXXX` escapes) |
| `-?[0-9]+` | an integer |
| `-?[0-9]+\.[0-9]+` | an `f64` |
| `true`, `false`, `null` | themselves |
| `[v,v,...]` | a JSON array; no spaces outside strings |
| `{k=v,k=v,...}` | a JSON object; no spaces outside strings |
| `$<label>` | the node the label is bound to, as `"#N"` ([API §5.1]); a label is `[a-z][a-z0-9_]*`. Inside a bare word it is replaced in place: `edge:$n40:blocks:$n12` is `"edge:#3:blocks:#2"`, `$s91.body` is `"#5.body"`. Inside a quoted string nothing is replaced. In an `Apply` `results` object, `task`, `recorded` and `findings[].about` take labels, which there denote the integer N ([90 §7.2]) |
| `c@<n>` | the commit id that command `n` of the stream created (its result's `commit`) |
| any other word | a JSON string: ref names, names, enumeration values, `L-2`, `#3` |

A label is bound once per stream, by the first command that creates its node, and every case restates in its header
comments the `#N` each label receives: allocation is in command order ([API §2.5] DT-4, §9.6), so the numbers are part of
the expectation, and a harness should check them.

### 2.5 Result facts

One fact per line, about the result of the script's last command, without a basis token. A list fact is an exact set
unless noted (`ready []` says that no task became ready); a fact repeated on several lines (`marker`, `conflict`,
`violation`) lists every entry of the set.

| Fact | Meaning |
|---|---|
| `exit <n>` | the exit code ([F19 §7]) |
| `error <code>` | the code of the first error of the envelope ([F19 §8.6]): an LQ code (`E409`) or a [F19 §10] code (`not_found`) |
| `mentions <word>` | the first error's message or detail lines contain the word: a text check. Texts are outside the model–engine comparison ([API §16.3]); GT10 checks them against the templates of [LQ/errors] and [F19] |
| `replayed <yes\|no>`, `dry <yes\|no>` | the envelope's `replayed` and `dry` |
| `commit none` | the command created no commit (`rev_new` null) |
| `ready [..]` | the newly ready tasks ([API §5.8]): `affected.ready` of family T, `yields[].ready` of `Complete` |
| `affected-includes <$label>` | the created commit's `affected` set holds the node ([API §3.3], [F13 §6.3]) |
| `marker <kind> <$label> <origin ref> [<cause>]`, `markers none` | the `settled`, `deleted` and `cleared` entries the command wrote ([API §10.8]); `cause` defaults to `ops` |
| `outcome <value>`, `staging-ref <ref>` | `data.outcome` and `data.staging_ref` of `Merge`, `Sync`, `MergeContinue`, `Revert` |
| `sync.outcome <value>` | `data.sync.outcome` of a sync-first `Merge` |
| `conflict <key> <class>`, `conflicts none` | the value conflicts the command landed (`data.conflicts`); `sync.conflict`, `sync.conflicts none` for step 0 |
| `violation <key> <class>`, `violations none` | the structural violations (`data.violations`); `sync.violation` for step 0 |
| `lca [..]`, `virtual-base <yes\|no>` | `data.lca`, `data.virtual_base` |
| `lease <L-n> task=<$label\|-> holder=<actor> branch=<ref> run=<name\|->` | a `Claim` yield row |
| `released [..]`, `recorded [..]`, `completed <$label>` | `Apply`, `BranchDelete`, `RunClose`: the leases ended; `Apply`: the recorded ids, an entry turned into a completion |
| `dropped commits=<n> completions=<n> deletions=<n>` | `BranchDelete`'s `data.dropped` |
| `ids [..]` | the ids a `Query` with `ids` returned, in order |
| `hint <code>` | the envelope carries the hint ([F19 §12.3]); where a family-T result carries it is gap G-6 |
| `status <value>` | `data.status` of `LaneClose` and `RunClose` |
| `imported ref=<ref> native=<n> foreign=<n> outcome=<value>` | one row of `ImageImport`'s `data.refs` |

### 2.6 State facts

One fact per line: `<view> <subject> <property> <value> [<basis>]`. The view is a ref name, whose tip is read, or `*` for
store-wide runtime state. The optional basis is the rule row or section tag the fact rests on (`EG-007`, `AR-5d.3`). A
value is `yes`, `no`, an integer, a label, `-` (none or absent), a name, a JSON string, or a list without spaces.

**Node subjects** (`$label`), read at the view's tip:

| Property | Meaning |
|---|---|
| `exists`, `deleted` | the node is live; the node is a tombstone. A node the view never held has `exists no` and `deleted no` |
| `status`, `resolution`, `priority`, `criticality`, `title`, `body` | the node's values ([API §15.3]); `body` as a JSON string |
| `field.<name>`, `counter.<name>` | a kind field ([F08 §9.3]), `absent` when it has no value; a counter's total |
| `parent`, `edges` | the parent label or `-`; the node's out-edges as `<kind>:<$dst>[:flagged]`, sorted by (kind, dst `#N`), a tombstone's retained out-edges included (I39′) |
| `reason`, `replaced_by` | a tombstone's reason (JSON string) and replacement |
| `open_blockers`, `unblocked`, `blocked`, `is_blocker`, `container`, `children_total`, `has_dangling`, `suspect`, `gated`, `answered`, `conflicted` | the derived predicates of [RULES/state-definition] PD rows and BT rows, [F13 §6.2] and [API §15.5] |
| `review-loop` | on a target node: the number of findings `about` it with status `confirmed`, severity `blocker` or `important`, and not `fixed` ([AR §3.5] "review-loop termination") |
| `conflict.<part>` | the class of the conflict value on the node's key `<part>`: `existence`, `status`, `body`, `parent`, a field name, or `edge:<kind>:$dst` |
| `hold` | `done`, `cancelled`, `deleted` or `none` ([RULES/state-definition] HV rows) |
| `excluded`, `deleted_elsewhere`, `settled_elsewhere` | PD-012, PD-013, PD-014 on the view |
| `blocking-listed` | PD-018: listed by `blocking` on the view |
| `ready`, `ready@<actor>` | PD-009 to PD-016 at the tip for the caller `orch` (the orchestrator of every stream), or for the actor named |

**Other subjects:**

| Subject | Properties |
|---|---|
| `edge:$a:<kind>:$b` | `exists`, `flagged`. For the symmetric kinds (`contradicts`, `relates`) the subject names the unordered pair ([F08 §10.1]) |
| `tasks` | `ready-set [..]`: every task `ready` for `orch` on the view |
| `lease:L-<n>` (view `*`) | `live` ([RULES/state-definition] `lease-live`; `no` once ended), `ended` (the lease is gone from [API §15.7] `leases`), `holder`, `task`, `branch`, `run_scoped` |
| `ref:<name>` (view `*`) | `exists` (a live ref holds the name), `deleted`, `kind`, `absorbed.<ref>` (the absorbed-vector entry, 0 when absent, [RULES/state-definition] VR-001), `ref_seq_next` ([API §15.7]), `violations [<class>:<key>,..]` (the staged violations of a staging ref, as a set) |
| `marker:<kind>:$label:<origin ref>` (view `*`) | `holders [..]` (sorted by name), `active`, `active_on [..]` ([API §15.7] `markers`, [RULES/state-definition] MF-006). The subject names the marker by its key: the node, the ref the origin commit landed on, and the hold's kind (`settled` or `deleted`), which it keeps after a `cleared` entry ends it |
| `c@<n>` (view `*`) | `kind` ([F06 §3.1]), `affected_complete` |

### 2.7 Streams, conventions and inputs

- **One executor.** A harness replays a case's stream from command 1 (or from a snapshot of its `from` case) and checks
  the `result` facts after the last command and the `expect` facts after the script. Facts of a `from` case are not
  re-checked.
- **Every stream** starts `EnvSlots hold=[claude:s1]`, `Init` with a seed and `params=["query.safelist.model.unknown=off"]`,
  and the orchestrator's session role lease `L-1` (holder `orch`, anchor `session`, [API] example 02). The key keeps
  free-form `TX` blocks from being refused by the model-profile rule ([RULES/role-write-policy] WR-012), which these files do
  not test (finding F-11). No other store parameter is set, so the production values apply.
- **Writes** present `L-1` (`@lease=L-1`) unless a case is about another role; `L-1` is a session role lease and writes on
  any branch ([RULES/role-write-policy] WR-005). The clock is never advanced unless a script says so, so no TTL lapses.
- **Repeated commands.** A command whose default idempotency key ([API §7.2]) can equal an earlier command's in its
  stream (the same actor and the same payload: the same `args`, or for a `TX` the same statements) carries
  `@no_dedupe=true` where it is meant to run: on the same branch it would replay the earlier result (finding F-14), on
  another branch [API §7.4] row 5 would refuse it with E408. The cases that test idempotency use explicit keys.
- **Policy data** has no Store API command yet ([RULES/policy-keys] open point 1, finding F-6): a `%% policy` line is an input
  the model takes as `PolicyData`; an engine harness applies it by whatever schema write WP-14 adds.
- **Edge kinds** are written by their stored names (`blocks`, `derived_from`) in every command.

### 2.8 `requires`

| Word | The case needs | Model at M0 | Engine from |
|---|---|---|---|
| `lq` | a `Tx` in its `lq` form | yes (LQ-3) | M7 |
| `files` | group F over a simulated tree ([API §6.5], §12) | yes (WP-92) | M6 |
| `image` | `ImageExport`, `ImageImport` and harness steps (§2.9) | no ([API §2.2]: the model supplies states and ids, no `.moi` bytes) | M5 |

### 2.9 Harness steps

Both run the git CLI in a scratch clone of the image repository a previous `ImageExport` wrote, then push the named ref
back; the resulting commit carries no `Moirai-*` trailer, so the importer classifies it as foreign ([F14 §10.9]).

| Step | Tokens | Effect |
|---|---|---|
| `ImageEdit` | `repo`, `ref`, `node=$label`, one edit, `author` | checks out `ref`, applies the edit to the node file of the label's node ([F14 §3.4]), and commits with one parent. Edits: `remove-file` (deletes the file), `set-title=<string>` (replaces the value of the `title:` line) |
| `ImageMergeOurs` | `repo`, `ref`, `other`, `author` | checks out `ref` and runs `git merge --no-ff -s ours <other>`: a two-parent commit whose tree is `ref`'s tree |

## 3. The stores

Each file builds its store in its first case; the header comments of each file list the labels and their `#N`.

| File | Store | Refs |
|---|---|---|
| `node40.cases`, `node40-branches.cases` | seed 40: the node-40 store of [RULES/delete-policy-matrix] §11, with the design's numbers as labels (`n40` is the design's `#40`; it is `#3` here) | `main`, `lane/x`, `lane/y`, forked at `c0` |
| `markers.cases` | seed 26: a leaf task and a blocker pair | `main`, `lane/a`, `lane/b`, `lane/c`, `plan/p` |
| `policies.cases` | seed 64: one node pair per edge kind (the "zoo"); seed 65 for the `at` rows | `main` |
| `walkthrough.cases` | seed 76: the campaign of [AR §7.6], labels carrying the design's numbers (`t89` is the design's `#89`) | `main`, `lane/l10`, `lane/l11`, `lane/l5np`, and a staging ref |
| `counter-merge.cases` | seed 100: one note with an `incidents` counter, one task | `main`, `lane/a`, `lane/b`, `lane/x` |

The design numbers cannot be the store's numbers: they are not in allocation order ([AR §7.6] gives `#9` to a question
created after `#93`, finding F-9), and reaching `#203` would need filler nodes that no expectation concerns.

## 4. Coverage

### 4.1 The node-40 table ([RULES/delete-policy-matrix] `n40-cases`)

| Row | Case | Row | Case |
|---|---|---|---|
| NC-001 C0 | `node40.cases` `c0` | NC-012 C7b | `node40-branches.cases` `c7b` |
| NC-002 C1 | `c1` | NC-013 C8a | `c8a` |
| NC-003 C2 | `c2` | NC-014 C8b | `c8b` (and the continuation `c8c`, `c8d`, `c8e`) |
| NC-004 C3 | `c3` | NC-015 C9 | `c9` (after the preparation `c9p`, finding F-2) |
| NC-005 C3r | `c3r` | NC-016 C9r | `c9r` |
| NC-006 C3p | `c3p` | NC-017 C10 | `c10` (and `c10d`, the refused `-d`) |
| NC-007 C3n | `c3n` | NC-018 C10f-a | `c10f-a` |
| NC-008 C4 | `c4` | NC-019 C10f-b | `c10f-b` |
| NC-009 C5 | `node40-branches.cases` `c5` | NC-020 C11 | `c11` |
| NC-010 C6 | `c6` | NC-021 C12 | `c12` (`requires image`) |
| NC-011 C7a | `c7a` | | |

Beyond the table: [AR §5d.3] row 2 (`rm` on `main`, a lane applies it by `sync`: `c13`, `c13s`, `c13r`), XB-005 (`c14a`,
`c14b`), XB-006 (`c15a`, `c15b`, `c16a`, `c16b`), L3 (`l3-set`, `l3-link`, `l3-rev`), DS-011 (`c2d`), [API §7.4] (`c2k`).

### 4.2 [RULES/delete-policy-matrix]

| Rows | Cases (`policies.cases` unless named) |
|---|---|
| EG-001 to EG-004 | `eg-001`, `eg-002`, `eg-003`, `eg-004` |
| EG-005 to EG-010 | `eg-005-008`, `eg-006-007`, `eg-009`, `eg-010`, `eg-008-question` |
| EG-011 to EG-016 | `eg-011`, `eg-012`, `eg-013`, `eg-014`, `eg-014-complete`, `eg-015`, `eg-016` |
| EG-017 to EG-022 | `eg-017`, `eg-018`, `eg-019`, `eg-020`, `eg-021`, `eg-022` |
| EG-023 | not constructed (gap G-3) |
| EG-024 to EG-032 | `eg-024`, `eg-025`, `eg-026` (finding F-5), `eg-027`, `eg-028`, `eg-029`, `eg-030`, `eg-031`, `eg-032` |
| EG-033 to EG-062 | `eg-033` … `eg-062`, one case per row; the symmetric pairs by `eg-057-a`, `eg-057-b`, `eg-061-a`, `eg-061-b` |
| EG-063, EG-064 | `eg-063`, `eg-064` (`requires files`) |
| DP-001 to DP-010 | `dp-001-mcp`, `dp-001-unleased`, `dp-002`, `dp-003`, `dp-007-dead`, `dp-007-deleted-set`, `dp-007-fit`, `dp-008`, `dp-009`, `dp-010` (`requires files`); DP-004 is not expressible through [API] (one `policy` value) and waits for the CLI (M8); DP-005 and DP-006 are `node40-branches.cases` `c9`, `node40.cases` `c1` |
| DS-003, DS-010, DS-011 | `ds-003`, `ds-010`, `ds-011`; DS-001 to DS-009 in every delete case |
| FL-001 to FL-011 | `fl-001`; FL-002 `eg-014-complete`; FL-003 every flag case; FL-005, FL-006 `node40.cases` `c3p`, `c3r`; FL-009 `fl-009`; FL-010 `node40-branches.cases` `c13s`, `markers.cases` `m2`; FL-008 `node40-branches.cases` `c12`; FL-004, FL-007, FL-011 are rendering and role rows asserted by every flag case's facts or not by GT10 |
| TB-001 to TB-015 | `tb-contents` (TB-001 to TB-013); TB-014, TB-015 are image and rendering rows |
| UD-001 to UD-008 | `node40-branches.cases` `c11` (UD-001, UD-003, UD-004, UD-006), `fl-009` (UD-002), `ud-notfound` (UD-003's staged case) |
| XB-001 to XB-010 | `node40-branches.cases` and `markers.cases` (XB-001 `c5`, `m1`; XB-002 `c6`; XB-003 `c7b`; XB-004 `c8b`; XB-005 `c14b`; XB-006 `c15b`, `c16b`; XB-008 `c10`, `c10f-b`, `m6`, `m9`; XB-009 `c12`; XB-010 `c9`); XB-007 is R4's re-key (`fixtures/r4/`, WP-92) |

### 4.3 [RULES/state-definition]

The `deleted` holds of `markers.cases` cover HV-003, OR-003, OR-005, PD-012, PD-013, PD-015, PD-017, PD-018, VK-001,
VK-002, VK-003, ME-001, ME-002, ME-004 (by `revert`), ME-005, ME-006, ME-007, ME-008, AB-001, AB-004, VR-002 to VR-005 and
DC-008, DC-009, DC-010, DC-014, DC-016, DC-017, DC-018, DC-020. The walk-through covers the `settled` holds (HV-001,
ME-001 by `apply`, ME-002 by the merge into `main`, LE-002, LE-004, LE-008, LF-001, LF-002, LF-004, DC-006) and BT-005,
BT-006, PD-024 (the gating verdict).

### 4.4 [AR §7.6]

| Step | Cases | Not asserted (why) |
|---|---|---|
| 1 Session start | `w-setup` (the staged `merge/lane/l10/from/main`), `w7d` (the brief's facts at the point step 1 describes) | the brief's text and its `dropped:` line (rendering, [RULES/pack-classes]) |
| 2 Decompose | `w2` | — |
| 3 Open the lane | `w3` | the pin and the flushed group (engine-internal) |
| 4 Design, review loop | `w4a`, `w4b`, `w4c`, `w4d` | `pack` output; the `stats loop` text (its count is `review-loop`) |
| 5 A rule on `main` | `w5a`, `w5b` | the `~main` pack marker and the `SubagentStart` hook ([API] open point 25); the sync is the orchestrator's |
| 6 Dispatch | `w6a`, `w6b` | the Codex dispatcher variant (the same claims) |
| 7 Implement, test, verdict, `apply` | `w7a`, `w7b`, `w7c`, `w7d` | `check 89` (needs a git history, `requires git` of M4); the journal adapter (`apply` takes the `result.v1` records directly) |
| 8 Merge queue | `w8a` … `w8f` | `merge-check`'s text; `git merge u/l5np` and `links sync` (the project repository, M6) |
| 9 Image | `w9` (`requires image`) | `moirai backup`; `git push` |
| 10 Next session | `w10` | the brief's text |

### 4.5 Counters

MR-026 (`k1a`, `k1b`, `k2`, `k3`), SM-003, RS-004, DM-010 and DM-015 with I30′ (`k4`, `requires image`), [F14 §6.5] and §11.2.

### 4.6 Rows of `docs/spec/COVERAGE.md`

The fixture column of these rows can cite (R-SPEC fills the column): [60 §3.13] GT10 node-40 table on one branch and
across branches (`node40.cases`, `node40-branches.cases`); [AR §3.4] I2, I3, I32′, I39′ (`policies.cases`,
`node40-branches.cases` `c9`); I26′ (`markers.cases`); I30′ (`counter-merge.cases` `k4`).

## 5. Labels and numbers

Every file's header comments list its labels with the `#N` each receives. The numbers follow from the stream
([API §2.5] DT-4): `#N`s in the order of the `create` statements and label-binding commands, lease ids `L-n` and fencing
tokens from `fence + 1` in `Claim` order ([API §10.1]).

## 6. Specification findings and gaps

Found while authoring; to be filed with the review. A **finding** (F) is a contradiction or an error; a case follows the
reading stated here until it is resolved. A **gap** (G) is something the specification does not decide; the cases leave it
unasserted. The final text of the WP-22 `gt10/` report repeats each finding with its proposed text.

| # | Where | Finding | Cases |
|---|---|---|---|
| F-1 | [RULES/delete-policy-matrix] FL-005, FL-006; [F12 §6.5], §6.6; [API §9.1], §9.2 | FL-005 resolves a flagged edge on a branch with `--take repoint:ID`; [F12 §6.5] resolves only conflict values and, on staging refs, violations (`repoint` "violations only"), and §6.6 makes any other key `not_found`. No spelling drops a flagged edge: data-level `resolve` has no drop, and `unlink`/`MATCH (#40)-…` cannot bind a tombstone ([API §9.1] `not_found`, [50 §3.6]); only LQ's `DELETED` pseudo-label reaches it | `c3r` (LQ `MATCH (d:DELETED …)`), `c3p`, `c13r` |
| F-2 | [RULES/delete-policy-matrix] NC-015, NC-016 | `c0/lease-L-19` cannot be reached: at `c0` #40 has an open blocker (#203) and a live child (#41), so no claim can take a lease on it (PD-005, PD-006, PD-017) | `c9p`, `c9`, `c9r` prepare `lane/y` first |
| F-3 | [AR §5d.3] L3; [F06 §2.3]; [API §15.4] | L3 says `--if-rev` on #12 with an old rev fails after #40's delete; #12's `rev_seq` does not change, since the delete changes only keys that #40, #41 and #203 own (an edge key belongs to its source, and #40's flagged edge to #12 is #40's) | `l3-rev` |
| F-4 | [AR §5a.7] step 0; [RULES/delete-policy-matrix] NX-055, NX-057; [RULES/merge-table] PR-003; [API §2.3], §11.7 | A sync-first merge whose sync lands a conflict value: the sync commit lands and the merge is refused (`conflicted_src`), but a refused command appends nothing ([API §2.3]) and §11.7 has no such outcome | `c7b`, `c16b` |
| F-5 | [RULES/delete-policy-matrix] EG-025, EG-026; [LQ/grammar-v1.ebnf] `delete_opt`; [API §9.2]; [LQ/std §7.2] | `--reassign` has no spelling: `POLICY` takes `RESTRICT`, `CASCADE`, `REPARENT`; `delete.policy` and `tx.rm`'s `$policy` the same | `eg-025`, `eg-026` use `policy=reassign` |
| F-6 | [CFG §10.13]; [API] | Policy-data rows are versioned schema rows but no command writes them ([RULES/policy-keys] open point 1) | `%% policy` lines (§2.7) |
| F-7 | [API §11.5]; [RULES/status-machines] TR-059 to TR-061 | `LaneClose` sets `merged`, but only `merge_pending → merged` is a transition; [AR §7.6] step 8 closes a lane it never moved from `active` | `w8c` moves the lane node first |
| F-8 | [API §11.5], §10.6 LP-5, §9.4 | Whether `LaneOpen`'s fork precedes its lane-node commit is unstated; if it does, the lane lacks its own lane node, `RunOpen` on the lane cannot add `runs_in`, and a run-scoped `Claim` on the lane cannot find its run (LP-5 resolves the run on the caller's branch) | `w3` syncs the lane after `RunOpen` |
| F-9 | [AR §7.6] | Step 2's question gets `#9`, after `#93` and while step 1's owner question is `#9`; step 7 makes `#93` ready while that question, which blocks `#93`, is still open at step 1; step 8's `section #91` is a task in step 2; step 8's `TextHunk` "landed during the sync" although step 5's sync is clean and step 8's has 0 conflicts; step 8's `merge_after` prerequisite `lane/l10` is staged | `walkthrough.cases` header |
| F-10 | [AR §7.6] step 7; [AR §3.1]; [F08 §9.4] | `finding --confidence observed`: findings take `confirmed`/`plausible` by [AR §3.1]; [F08 §9.4]'s "`confirmed` and `plausible` only on findings" leaves open whether findings may take the others | `w7a` omits the confidence |
| F-11 | [RULES/role-write-policy] WR-012; [CFG] `query.safelist.model.<profile>`; [API] example 02 | Example 02's `Tx` (n = 4) succeeds, but by CX-6 and CX-7 its caller's profile is `unknown`, whose default `named-only` refuses a free-form `TX` with E411 | every stream sets the key `off` |
| F-12 | [RULES/merge-table] DM-015; [AR §5b.6] step 3; [F14 §11.2] | DM-015 and [AR] take a foreign merge's counters from "the union of the `incr` ledger lines"; [F14 §11.2] from the typed merge. The two differ when both parents' first-parent ledgers carry a merge line for one shared increment | `k4` expects the typed merge (13, not 14) |
| F-13 | [RULES/merge-table] MR-051; [RULES/delete-policy-matrix] XB-006 | Two different deletes of one node on two branches (one re-pointing its blocker edge, one flagging it) give a `FieldEdit` on the tombstone's edge key (MR-051), so the sync-first merge is refused; XB-006 names only MR-041 and MR-046 | `c16b` |
| F-14 | [API §7.1]–§7.4; [F17 §11.1] P29 | Default idempotency keys hash only a command's `args`, so a second `Sync` of one lane, or a second `Merge` of one pair, within `idempotency.default-window` (10 min) replays the first result and syncs nothing, even when `main` moved in between; only `LinksSync` is exempt | `@no_dedupe=true` on repeated commands (§2.7): `node40-branches.cases` `c8e`, `walkthrough.cases` `w5b`, `w8a` |
| F-15 | [API §15.3]; [F08 §3.5]; [RULES/delete-policy-matrix] TB-012; [AR §5b.2] rule 8; [F07 §6.4] | A tombstone's `content` in [API §15.3] keeps "the header enumerations" (status, resolution, priority, …) as [F08 §3.5]'s row does, but TB-012 drops the status and every field, and neither the canonical form nor the tombstone file carries them, so `content` (defined as what canonical item 10 can express) would differ between a store and one that imported its image | `policies.cases` `tb-contents` leaves them unasserted |

| # | Gap | Cases |
|---|---|---|
| G-1 | Whether a `--cascade` descendant's tombstone takes the delete's `reason` and `replaced_by` (DS-005) | `c4` leaves them unasserted |
| G-2 | Whether a staged sync-first merge reports its violations in `violations` as well as in `sync.violations` ([API §11.7]) | the cases assert `sync.*` only |
| G-3 | EG-023 ("another answer remains") needs two live `answers` edges into one question, which `answers`' cardinality "≤ 1 active" forbids at every head; "active" is not defined | not constructed |
| G-4 | Whether the resolution of a task completed by `set --done` (not `complete`) is `completed` | `resolution` is not asserted for such tasks |
| G-5 | Whether a data-level `Tx`'s `message` enters its idempotency payload: the LQ equivalent of [API §9.3] has no `MESSAGE` option, while [LQ/canonical-ast] open point C-2 keeps `MESSAGE` in `H` | two blocks that differ only in their message carry `@no_dedupe` (`counter-merge.cases` `k1a`, `k3a`) |
| G-6 | Where a family-T result carries a hint: [RULES/delete-policy-matrix] DS-010 prints `SuspectBudget` for a delete, but [API §3.3]'s family T has no `hints` key (family W has one, [API §3.4]) | `policies.cases` `ds-010` asserts only that the hint is carried |

## 7. The core set for owner verification (E1, V3)

GT10's core set is verified by the owner; the rest was written by an author who saw neither engine nor model code (S3).
The core cases carry `%% core yes`:

1. `node40.cases`: `c0`, `c1`, `c2`, `c3`, `c3r`, `c3p`, `c3n`, `c4` (the one-branch rows NC-001 to NC-008).
2. `node40-branches.cases`: `c5`, `c6`, `c7a`, `c7b`, `c8a`, `c8b`, `c9p`, `c9`, `c9r`, `c10`, `c10f-a`, `c10f-b`, `c11`, `c12`
   (NC-009 to NC-021).
3. `walkthrough.cases`: every case (the [AR §7.6] walk-through).

The register incidents, the third part of the core set, are owner data in `/private/gt10/`.

## 8. How these files were made and are maintained

- Every expectation was derived by hand from the rule rows and sections each case cites, and cross-checked against the
  worked rows of [RULES/delete-policy-matrix] §11 and [RULES/state-definition] §8 where a case restates one; the two
  agree except where §6 records a finding.
- `#N`s, lease ids, `ref_seq`s and absorbed-vector entries were counted from the streams by the allocation rules of
  [API §2.5] and §9.6; commit ids are never written out (they are hashes), only named by `c@<n>`. A throw-away checker
  (not committed, sharing no code with any crate) parsed every file by §2, followed each `from` chain, checked the
  command numbering and the `c@<n>` references, and recomputed every label's `#N` against the header comments.
- The files hold no owner-derived data: every id, title, ref and actor is synthetic or taken from the design's fictional
  campaign ([AR §7.6]).
- A disagreement between an implementation and a case is triaged against the specification: an expectation the cited rows
  support is a defect of the implementation; a case marked with a `finding` line, or any other disagreement, is a finding
  for the review first.
