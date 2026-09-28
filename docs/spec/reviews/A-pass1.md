# WP-80 pass 1, lens A: agent fit, tokens and buildability

| | |
|---|---|
| Status | review, WP-80 pass 1 (independent specification review) |
| Work package | WP-80 ([docs/m0/PLAN.md](../../m0/PLAN.md) §3.2 item 8), lens R-REV-A (method of [22]) |
| Targets | everything under `docs/spec/`: `format/` (F01–F20), `os/`, `lq/`, `store-api.md` and `store-api/examples/`, `config.md`, `COVERAGE.md`, `HOLES.md`, `rules/*.md` |
| Sources read | every target file (the large rule tables by their registries, headers, holes, open points and the rows named below); [AR] §0, §7.1–§7.5, §8.3 TOKENS rows, §11, §13; [40] §2.4–§2.7, §4.4, §6.1–§6.5; [50] §2.4–§2.9, §3.8–§3.10, §4.1–§4.4, §5.2, §6.1–§6.6, §7.1–§7.4; [90] §2.1–§2.4, §4.1–§4.4, §6.1–§6.7, §8.1–§8.3, §10.1; [60] §2.5, §3.13, §4; `reviews/a1-A.md`, `reviews/a1-dispositions.md` |
| Coverage rows | none: a review file owns no [60 §2.5], R-n, F-n, X-F-n or [90 §10.1] row |

Severity: **blocker** — two independent implementers following the specification would produce different bytes, or a design
rule is violated; **major** — an ambiguity, a missing case or a cross-chapter inconsistency that must be closed before the
freeze; **minor** — editorial, closed by the owning chapter.

Mechanical checks behind the findings (scripts in the reviewer's scratch space, not committed): every offset table was summed
against its `total` row; every `[Fnn §x]`, `[OS/…]`, `[LQ/…]`, `[API]`, `[CFG]` and `[RULES/…]` citation was resolved against the
headings and numbered paragraphs of its target; every worked example of F01, F02, F06, F07, F08, F11, F19 and F20 was
recomputed; the `moirai-ql` card's byte counts (§3: 36 lines, 3,116 B, 457 words; GQL 3,110 B; 4-example 2,867/2,864 B;
0-example 2,394/2,391 B; frontmatter 206 B, description 168 characters) were recomputed and match; every machine-readable
rule table was parsed by the contract of [RULES/README] §2–§4 (1,858 row ids in 110 tables, no duplicate id, no bad cell);
the 20 Store API examples parse as JSON and carry commit ids as `c` + 64 hex.

## 1. Verdict

**Not ready to freeze.** The texts an agent reads (the card, the error table, the refusals, the reader note, the link markers)
are in good shape and close to their byte budgets, and the card's numbers are exact. The blockers are elsewhere: the format
chapters still disagree with each other on bytes that are hashed or folded. Seventeen blockers remain, in five groups:

1. **Two owners for one byte layout** (A1-1, A1-2, A1-7, A1-8): value encodings, the anchor record, link-state codes and the
   R-15 binding block are each defined twice, with different numbers.
2. **Records that cannot carry what their readers need** (A1-3, A1-6, A1-9, A1-10, A1-11, A1-12, A1-13, A1-15): segment
   edge properties lose half of the pinned commit id and the `flagged` bit; F05 payloads cannot be folded into F11 rows; a
   hashed conflict byte, a `RefUpdate` reason, a record kind, an init parameter and a per-root algorithm field are used by one
   chapter and absent from the chapter that owns the bytes.
3. **Stored forms that no chapter defines** (A1-4, A1-5, A1-14): a tombstone from an absent state, the header-only commit, and
   the scope scanners whose output is hashed.
4. **A hashed value assigned by two rules** (A1-17): the append-time HLC.
5. **A disposition not applied** (A1-16): F19 still writes JSON commit ids without `c`.

Twenty-four majors and twenty-one minors follow. Every blocker has a one-chapter fix; most are "chapter X cites chapter Y's
table verbatim".

## 2. Deferred items this pass closes

| Item | Outcome |
|---|---|
| **A-m1** (link marker ≤ 50 B, deferred to this pass) | **Not closed; major A1-29.** [F19 §4.6] rule 5 restates the gate as "≤ 90 B per marker, golden median ≤ 50 B", which is option 2 of A-m1, but [AR §8.3] still reads "≤ 50 B per marker" and no owner decision records the change. With [F18 §4.2]'s strings the worst markers are `[moved-needs-confirm -> <20-byte path> 0.81 \| verify #<10 digits>]` = 71 B and `[ambiguous (normalization collision) \| verify #1234]` = 52 B, so the design's gate cannot pass as written. |
| **Dispositions open point 1(c)** (FS-4 agent text "move it with a raw mv; links re-bind by evidence") | **Confirmed with one wording change (minor A1-59).** A raw cross-volume `mv` loses the file id (evidence E3), so an identical copy usually lands as `moved-needs-confirm`, a proposal, not an automatic re-bind. Proposed text: `move it with a raw mv; links re-bind by evidence or show a proposal to confirm` (the `cross_volume` help of [F19 §10.2] and [OS/project] open point 1). |
| **A-m4** (`not_writer_tree` text) | Closed: [F19] tells the agent to use a raw `mv`/`rm` or ask the orchestrator, and names the bind line for the orchestrator and owner only. |
| **A-m7** (JSON commit ids) | Not applied in [F19 §8.3]; blocker A1-16. |
| [OS/shell] open points 3 (Codex `cmd /C` hooks, bare `moirai` on `PATH`) and 5 (stricter teaching alphabet) | Confirmed. The cursor text (`k` + Crockford base-32, [LQ/envelope §8.3]) never starts with `-`, so it cannot be read as a flag. Suggestion: `doctor agents` reports how `moirai` resolves on `PATH` for each harness. |
| [OS/project] open point 9 (read-only file `rm`: clear the attribute, delete, restore on failure) | Confirmed. |
| [F13] invariants | Every invariant has an enforcement point, a model function and a gate. |

## 3. Findings

### 3.1 Blockers

**A1-1 (blocker). Two value-encoding registries.** *Where:* [F06 §5.1]; [F08 §5.1], §5.2; [F07] open point 9; [F08] open
point 45. *Problem:* F06 numbers the closed value set as tags 0–14 (`absent`, `false`, `true` as tags; text-symbol 8; `ref`
as `uvar32`; commit-ref as `b32`; a set of count ≥ 0; `pathmove` class 0–3). F08 numbers the same set as type ids 1–13
(the bool in bit 7; symbol 7; `ref` as fixed `u32`; commit-ref as `b16`; a set of n ≥ 1; `pathmove` class 1–4). F06 says
F08 "may reuse these tags", F08 open point 45 asks F06 to use F08's encodings, and neither did. A value written in a log op
and the same value in a segment field block are two byte strings, and the empty-set/empty-text case has two answers.
*Fix:* one table, owned by one chapter (F08 §5.1 is the natural owner because the segment layouts cite it), with F06 §5.1
reduced to a citation; one commit-ref width (32 B, because canonical form and the image need the full id); one `pathmove`
class numbering; one decision on empty set and empty text ([F07] open point 26).

**A1-2 (blocker). The anchor record is laid out twice.** *Where:* [F06 §7.5.3]; [F08 §10.3], §10.3.1; [F09 §13.3]; [F06]
open point 15; [F08] open point 37; [API] open point 32. *Problem:* F06: `akind` 0–5, `aflags` bits for text/window/hint/span,
`scope` as a `vstr`, blob `algo` 0 allowed (planned target), no uid. F08: `kind` 1–6, the uid inside the record, `scope` in a
binary form (§10.3.1), presence of hint/window/span by kind, blob `algo` ≠ 0. Both chapters claim ownership. The log uses F06's
form and the `ANCHORS` section uses F08's, so a planned-target anchor (blob `algo` 0) cannot be checkpointed, and the
string-to-binary scope conversion is specified nowhere ([F14 §5.6] gives only the text bijection). *Fix:* F08 §10.3 owns the
record; F06 §7.5.3 cites it byte for byte; decide whether `algo` 0 is legal (it must be, for `--planned`); state the scope
conversion once.

**A1-3 (blocker). Segment edge properties lose data.** *Where:* [F09 §7.2] `EDGE_PROPS`; [F08 §10.2]; [F06 §7.5.2]; [F07 §7.1],
§8.1; [F14] open point 13. *Problem:* `EDGE_PROPS` holds `(edge u32, pinned_commit b16)`: no `flagged` bit and only 16 of the
32 bytes of the pinned commit id. The canonical edge value hashes `pmask` (flagged, pinned) and the full `b32` pin, and the image
writes `pin=c<64 hex>`. After a checkpoint the engine cannot rebuild either, so commit ids and image bytes differ from the
model's. *Fix:* `EDGE_PROPS` rows carry the F06 §7.5.2 property block (`pmask` + full 32-byte slot of [F01 §7.5]); F08 §10.2
aligns its `pflags` bit order with F06.

**A1-4 (blocker). A tombstone that arrives from the absent state has no stored op.** *Where:* [F07] open point 5; [F14 §11.2]
and open point 25; [F12]. *Problem:* a merge of a lane that created and deleted a node, or an import checkpoint, moves a key
from absent to tombstone. F07 hashes that transition and F14 needs it, but F06 has no op whose before-image is "absent" for a
delete. *Fix:* F06 adds the op (or a flag on the delete op) with its bytes, and F07 its canonical entry; or F12 proves the case
never reaches a changeset and F07/F14 drop it.

**A1-5 (blocker). `gc`'s header-only commit is undefined.** *Where:* [F10 §4.6], OP-10-10; [F06]. *Problem:* `gc` rewrites
`hist` and replaces dropped commits by "[F06]'s header-only form", which F06 never defines (no presence bit, no `n_ops` rule, no
`RecHdr.len` rule, no statement of how `commit_id` verification treats it). Two `gc` implementations write different bytes.
*Fix:* F06 defines the header-only form (for example the header part with a flag bit and `cs_bytes` = 0, and how
`changeset_digest` is kept); or F10 keeps whole commits.

**A1-6 (blocker). F05 record payloads cannot be folded into F11 rows.** *Where:* [F05 §9.4], §9.5, §9.8, §9.16–§9.18, §9.23;
[F11 §6], §7, §8, §12.4, §12.5; [F11] open point 33. *Problem:* the fold and the replay of the same log produce different rows:
- `PINS`: F11 `PinHolder.kind` 1 base, 2 tag, 3 merge; F05 `Pin.holder` 1 fork base, 2 promotion base, 3 merge, 4 tag;
- settle epoch: F05 `scope_kind` 0 full, 1 lane-owned, 2 partial; F11 1 full, 2 lane-owned, 3 brief (and `partial` ≠ `brief`);
- `FILEOBS` proposals: F05 (`pathv`, evidence `u8`, score `u16`/10,000) against F11 (class `u8`, path `vstr`, evidence token
  `vstr`, rational score), and F11's `detail` and `flags` have no source;
- `PENDING` needs the evidence token and from/to text that F05 does not record; `ANCHORRES` has a `detail` and a rational
  score, F05 a `u16` score;
- `FSINTENT` item outcomes: F11 {0 open, 1 done, 2 aborted, 3 ambiguous, 4 missing}, paths without root; F05 `FsIntentDone`
  {1 done, 2 busy, 3 exists, 4 src missing, 5 other} and `FsIntentAborted` reasons 1–5, `pathv` with root;
- `MARKERS`: F11 origins 4 re-attributed and 5 branch delete with `orig_ref_id`; F05 cause 4 only, no `orig_ref_id`;
  F11 `outcome` is F05 `status`, and F05's `outcome` (`complete --outcome`) has no F11 home ([API] open point 34);
- `REFS` against F05 `RefEntry` ("complete entries"): no `fork_lsn`, `fork_ref_id`, totals, trunk marks, moves or
  `flags.pinned`, and different widths (`ops_since_fork` `uvar64` against `u32`; tip `cid32` against `b16`);
- `LEASES.flags.session_role` has no source field in the F05 claim.

*Fix:* adopt F11 open point 33 (each record carries every field its row holds, in the row's encoding) and give every shared
enumeration one owner, cited by the other chapter.

**A1-7 (blocker). Link-state codes.** *Where:* [F18 §4.2]; [F11 §12.5] `FILEOBS.state`; [F05 §8.7]; [F18 §4.10]. *Problem:* F18
has 5 `deleted`, 6 `replaced`, 7 `stale-anchor` (link only), 8 `missing`; F11 has 5 `replaced`, 6 `missing`. F05 and F18 both say
F18 owns them. *Fix:* F11 §12.5 cites F18 §4.2's codes.

**A1-8 (blocker). The R-15 binding block has three shapes.** *Where:* [F18 §3.2] `BindingExt`; [F11] `HEADS`; [F05]
`ClientHead`. *Problem:* F18 says its 40-byte block (`bflags` bit 0 designated, `base_algo`, `expected_ref` as a `u32` symbol of
the **short** branch name, `base[32]`) is "embedded by `HEADS` and `ClientHead`". F11 stores flags bits detached/binding/designated,
an `OidSlot` base and a heap `git_ref` with the **full** name (`refs/heads/u/l5np`); F05 stores a `vstr` `expected_git_ref`, an
`oidv` base and `bflags` binding/designated/has_dir_id. The I-F12 on-the-line check compares a short name in one chapter and a
full name in another. *Fix:* F11 and F05 embed F18 §3.2 verbatim, or F18 stops claiming embedding; one ref form (the full name,
which is what git reports).

**A1-9 (blocker). A hashed byte missing from the owner of `cstate`.** *Where:* [F12 §6.3], open point 4; [F06 §6.2]; [F07 §7.3].
*Problem:* F12 requires `prov` (`u8`: 0 ours, 1 theirs) in every existence-conflict `cstate`, and it enters commit ids. F06's and
F07's `cstate` layouts have no such byte, so a policy override cannot be replayed and commit ids diverge. *Fix:* F06 §6.2 and
F07 §7.3 add `prov` at a stated position with its presence rule, or F12 drops it.

**A1-10 (blocker). Import-checkpoint data with no home.** *Where:* [F14 §11.3], open point 22; [F06 §3.2]; [F09]. *Problem:* the
image-only data an import checkpoint must keep (per-node `created`/`updated`/`deleted` values, ledger lines) is stored nowhere,
so gate 2 (byte-identical re-export) cannot pass and format v1 freezes without the field. *Fix:* F06's import-checkpoint
payload or an F09 section carries it with a layout; or the owner relaxes gate 2 for checkpoint imports.

**A1-11 (blocker). `RefUpdate` reason 5 `park`.** *Where:* [F16] P-70, open point 16; [F05 §9.2] (`reason` 1–4). *Problem:* P-70
appends reason 5; to F05 that value is invalid, so the record is invalid and the scanner reports corruption on a legal log.
*Fix:* F05 §9.2 adds 5 = `park` ([F12] open point 14).

**A1-12 (blocker). Record kind 27 `Reserve`.** *Where:* [F16] P-84, P-6, open point 5; [F05 §3], §7 ("0 and 27–255 are
invalid"). *Problem:* bulk commits need a durable reservation group of kind 27, which F05 does not define. *Fix:* F05 adds kind
27 with its payload, durability class and fold target.

**A1-13 (blocker). `InitParams` bytes 16–31.** *Where:* [F17 §2.1] (`_reserved`, reserved-zero, tested by IP-2); [F04 §4.4]
(`store_id`, never all zero); [F04] open point 2. *Problem:* one chapter requires zeros where the other requires a non-zero id.
*Fix:* F17 §2.1 names the field `store_id` and IP-2 checks it non-zero (WP-16c's promised edit).

**A1-14 (blocker). Scope scanners.** *Where:* [F20] open point 30; [40 §2.7.1]; [F08 §10.3]. *Problem:* the Rust, Markdown and
TOML scanners decide the `scope` bytes of an anchor; those bytes enter `captured`, hence the anchor uid and commit ids, and no
chapter specifies the scanners. *Fix:* specify each scanner's grammar and output bytes before the freeze (F20 or a new
chapter), or exclude `scope` from every hashed input in v1.

**A1-15 (blocker). The per-root oid algorithm has no field.** *Where:* [F20] open point 27 (S-18: a per-root, `init`-fixed
algorithm); [F04 §4.4]; [F17 §2]. *Problem:* the value is `init`-fixed but has no byte in `InitParams`, so a store cannot record
it and two implementations pick differently. *Fix:* F17 adds the init-fixed parameter and F04 its offset.

**A1-16 (blocker). JSON commit ids in [F19].** *Where:* [F19 §8.3], open point 24; A-m7 in `reviews/a1-dispositions.md`
(varied: `c` + 64 hex); [LQ/envelope §7.1], §7.3; [LQ/errors §5.7]; [API §5.1] and open point 1; [F12 §3.8]. *Problem:* F19
still writes "64 hexadecimal digits without the `c` prefix". *Fix:* F19 §8.3 and its open point 24 follow the disposition.

**A1-17 (blocker). The append-time HLC has two rules.** *Where:* [F16] P-36; [OS/clock §7]; [API §6.2] CK-4, open point 39.
*Problem:* P-36 takes `h_last` over every append-time HLC in the log (lazy records and `Checkpoint.append_hlc` included), so a
maintenance record between two commits raises the next commit's hashed `hlc`; CK-4 advances only over semantic durable
records. The model follows CK-4, the engine P-36, and their commit ids differ; a lazy record lost in a crash changes later
ids. *Fix:* P-36 and OS/clock §7 take `h_last` over the semantic durable records of CK-4 (I43′ still holds), or F16 states
another rule under which [F17 §1.5] SP-1 and crash determinism hold.

### 3.2 Majors

**A1-18 (major). Swap recovery at discovery.** *Where:* [OS/fs §4.9.4]; [F02 §3.2], open point 17; [F16] P-86. *Problem:* OS/fs
runs `swap_recover` "by doctor and by store discovery"; F02 and P-86 forbid it at discovery, and P-86's seeded bug is exactly
that. *Fix:* OS/fs §4.9.4: `doctor` only.

**A1-19 (major). Equality of existence conflicts.** *Where:* [F12 §7.3] rule 2; [F07 §7.3]. *Problem:* F12 compares without the
node image, F07 compares byte-equal `cstate` including it. *Fix:* one rule, cited by the other chapter.

**A1-20 (major). Schema item ids in ops.** *Where:* [F08 §8.3]; [F06 §7.6]. *Problem:* F08 says the op that first lands a schema
item carries its store-local id "([F06])"; F06's `Schema` op has no id field. *Fix:* F06 §7.6 adds the unhashed id, or F08 derives
it at fold.

**A1-21 (major). `cs_ref.b3`.** *Where:* [F09] OP-09-03; [F06 §9] BK-2; [F04 §4.1]. *Problem:* BLAKE3-128 of the file against the
first 16 bytes of `seg_digest`. *Fix:* choose one.

**A1-22 (major). The `cs` op list.** *Where:* [F10 §8], OP-10-11; [F09 §16.4], OP-09-15. *Problem:* F10 relies on the `cs` file's
"OPS" list; F09 stores none and has no such tag. *Fix:* align.

**A1-23 (major). Fold targets that do not exist.** *Where:* [F05 §9.11] (`Lazy` cursors), §9.13 (backup registry), §9.14
(`SessionMark`); [F11]. *Problem:* F05 names "[F11]" tables for them; F11 has none. Cursors and session marks are token features
(delta hooks), and `backup.max-age` needs the backup's HLC. *Fix:* F11 adds the tables, or F05 states that these records are read
by replay only and names the reader.

**A1-24 (major). Extent length.** *Where:* [F05 §2.2]; [F17] IP-6; [F16] open point 4. *Problem:* "any other extent length is
corruption" (exit 7), while F16 treats a shorter extent beyond the valid end as a legal leftover of an interrupted preparation.
*Fix:* F05 and F17 admit the leftover case F16 defines.

**A1-25 (major). The initial `HEAD`.** *Where:* [F04 §10]; [F16] P-88, open point 14. *Problem:* F04's table has `next_ref_id` 0 and
zero table pointers; P-88 creates `main` at `init` (`next_ref_id` 1, `refs_lsn` ≠ 0). *Fix:* F04 §10 follows P-88.

**A1-26 (major). Author-flagged decisions still open.** *Where:* [F13] OP-13-05 (I26′ against the marker cache when a parent ref
reopens); [F17] OP-17-15 (the suspect-budget reading, pending the owner). *Problem:* both change model semantics and neither is
on the owner list. *Fix:* decide before the freeze; add both to the owner list of WP-80b.

**A1-27 (major). Protocol rules without a seeded bug.** *Where:* [F03] SR-2, SR-5, §8.6 (seqlock re-read), §10.3 (anchor
construction); [OS/proc §6.2] (liveness); [OS/lock] grant table (only P-3 has a bug); [PLAN §7] E4. *Problem:* every F16 rule has
a seeded bug, but these protocol rules outside F16 have none, so GT18 (lease liveness) has no bug to catch. *Fix:* give each a
seeded-bug row (in F16's table or in the owning chapter).

**A1-28 (major). Stale fault-model rows.** *Where:* [F15 §6.5] FM-3, FM-5 (`ProjectFs`). *Problem:* they still describe the
cross-volume copy (copy, flush, verify, delete) that FS-4 removed. *Fix:* rewrite for the refusal path.

**A1-29 (major). Link-marker gate (A-m1).** *Where:* [F19 §4.6] rule 5, open point 23; [AR §8.3] TOKENS row "Link marker";
[F18 §4.2]. *Problem:* see §2: the design's ≤ 50 B cannot hold for R-16's strings; F19 restated it as ≤ 90 B max with a ≤ 50 B
golden median without an owner decision. *Fix:* owner sign-off and the [AR §8.3] edit at WP-81a; or shorter shapes
(for example `[moved? -> <path> 0.81 | verify #N]`), measured on goldens.

**A1-30 (major). Header extras over their limit by template.** *Where:* [LQ/envelope §9.3] (`DRY`), §3.2 (composite parts); [F19
§4.2], open point 1. *Problem:* the `DRY` header's fixed extras (` | tx (dry) | IF TIP ok | IF TARGETS ok | would commit <n>
changes | nothing written`) are about 83 B against the 60 B extras limit, and composite parts that each print `<c8>` exceed it
too, so the GT12 and WP-71a golden checks fail by construction. F19 open point 1's proposals (drop `nothing written`, which the
`would commit` field implies; drop the per-part `<c8>`) were not adopted by LQ/envelope. *Fix:* adopt them in LQ/envelope, or
record an owner-approved limit for these headers.

**A1-31 (major). The `--ids` cut page.** *Where:* [LQ/envelope §6.5]; [F19 §6.2]; [CFG] `output.ids-max-bytes`; [AR §7.1].
*Problem:* LQ/envelope says a cut page holds up to `output.ids-max-bytes` (24,000 B); F19 cuts at min(L, N) = 8,000 B. A 30,000 B
result prints 24,000 B in one reading and 8,000 B in the other. *Fix:* LQ/envelope cites F19 §6.2; the owner confirms the
8,000 B cut against [AR §7.1]'s 24,000 B.

**A1-32 (major). The 600 B error bound is not guaranteed.** *Where:* [LQ/errors §3.4] (fitting and its bound check), §5.5 (E401,
E404, E409). *Problem:* the bound check assumes detail lines of at most 120 B of fixed text, but detail templates interpolate
values (E401's line carries every field the `WHERE` reads, the rev, `<c8>`, the actor, the ref and the message: well over three
values). Three such lines at `V` = 24 exceed 600 B on their own, and the last step only cuts the message. Worse for the agent,
step 2 drops "the last detail line" first, which for E4xx is `nothing was written`. *Fix:* pin `nothing was written` (never
dropped); cut every detail line to 120 B by §2.3's rule; keep at most two target lines; add a final step that drops target
lines down to the pinned one; redo the arithmetic.

**A1-33 (major). VFS error kinds.** *Where:* [OS/project] open point 6; [OS/fs §6.1]. *Problem:* OS/project needs `CloudOnly`,
`IsSymlink`, `IsDirectory`, `OutsideRoot` and `Stale`; the `VfsErrorKind` enum lacks them, so WP-30 cannot build the seam as
specified. *Fix:* add them to OS/fs §6.1 with their mapping rows.

**A1-34 (major). git's similarity index.** *Where:* [F20 §5.11.4], open point 36; [F01 §2.2]. *Problem:* E6's inexact pairs are
scored "as git's source at a pinned version", summarised informatively; the score enters the hashed `relink` value
(`git-pair/<score>`). *Fix:* specify the function normatively in F20, or list the pinned git version as a normative external
reference in F01 §2.2 with golden vectors.

**A1-35 (major). The rule-table registry is incomplete.** *Where:* [RULES/README] §1.1, §3, §7. *Problem:* the registry lists
only the 30 tables of README, `merge-table.md` and `link-merge-rules.md`; the 80 tables of `delete-policy-matrix.md`,
`pack-classes.md`, `role-write-policy.md`, `state-definition.md` and `status-machines.md` are absent, and by §3 an unregistered
marker is a parse error, so the model's parser (WP-90) refuses five of the eight files. §1.1 lists `delete-policy.md` and
`i26-state.md` as planned, omits `role-write-policy.md`, and marks drafts as planned (COVERAGE open point 6). *Fix:* register every
table with its kind, prefix and typed columns; update §1.1.

**A1-36 (major). `complete --outcome failed|abandoned`.** *Where:* [LQ/std §7.3] `tx.complete`; [RULES/status-machines] CO-002,
CO-003; [API] open point 13; [AR §6.2]. *Problem:* LQ/std says `failed` and `abandoned` release the lease without the transition;
the rule table and the API write `done` for every outcome, as [AR §6.2] does. *Fix:* LQ/std §7.3 follows [AR].

**A1-37 (major). Policy values hard-coded in LQ/std.** *Where:* [LQ/std §7.3] `tx.claim` (`$ttl: text = '15m'`), §2.4 (`BUDGET`
classes 2e5/2e6/2e7); [CFG] `lease.ttl-default`, open point 18; [API] open point 36. *Problem:* a store that sets
`lease.ttl-default` gets 15 min anyway through the named mutation's default; the budget classes ignore
`query.budget.default.work` (AGENTS.md: runtime policy goes into config keys). *Fix:* `$ttl: text? = NULL`, meaning the key; the
classes as fractions of `query.budget.default.work`, as [CFG] proposes.

**A1-38 (major). `std.ready`'s order is not deterministic.** *Where:* [LQ/std §4.1] (`ORDER BY t.priority, t.topo, t.id`); [API]
open point 11, §16.6. *Problem:* among ready tasks any topological order is valid, so `topo` breaks ties differently in the model
and the engine; the first page of `ready` differs, and GT2 has to compare it as a set. *Fix:* order by `t.priority, t.id`, or
define `topo` as a total, deterministic order.

**A1-39 (major). Error codes that other chapters need are missing from [F19].** *Where:* [F19 §10.2], §10.4; [API] open points 5,
20, 38; [CFG §6.2], open point 21; [F12] open point 16. *Problem:* F19 owns the store, file-verb and VCS code table, but lacks
`name_taken`, `not_merged`, `revert_refused`, `not_fresh`, `not_found` with `what` = `node` (the exit-3 write to a node not live
on the view), `bad_ref_name`, `ref_exists`, `ref_prefix`, the warning `graph_only_revert`, and CFG01–CFG17. Their texts and exit
codes are not frozen, and two chapters already use different codes for ref-name errors. *Fix:* WP-18 adds them to F19 §10.

**A1-40 (major). Temporary names outside the grammar.** *Where:* [F02 §6.3]; [OS/env] open point 5 (`probe`, `probe.1`,
`probe.2`); [OS/project] open point 18 (`settle.stamp`); [F16] P-79. *Problem:* by F02 §5.6 these names are foreign files, never
swept. *Fix:* F02 §6.3 adds them.

**A1-41 (major). `SCHEMA` row order.** *Where:* [F09 §8.3] ("sorted by [F08]'s item key"); [F08 §8]. *Problem:* F08 defines no item
key as a sort key (names or symbol ids?), so the byte-identical rebuild of [F09 §17] is ambiguous. *Fix:* state the key and its
comparison in F09 §8.3.

### 3.3 Minors

**A1-42 (minor). Hole ids.** *Where:* [CFG] L1123 and [OS/lock] L193–194 (`lock-writer-wait-ms`, `lock-flush-wait-ms`, owned by
[F17] as `F17-lock-writer`, `F17-lock-flush`, with two owners claimed); [OS/lock] L364 (`lock-release-delay`); [F03] L515
(`os-win-boot-source`); [F16] L887, [OS/fs] L725 (`share-retry-ms`); [F19] L1045, [LQ/canonical-ast] L811, [LQ/gql-spelling]
L247 (`display-spelling`); [F19] L1131, [RULES/pack-classes] NR-001 and Holes, [API] (`pack-digest-param`); [RULES/role-write-policy]
WQ-004, WZ-004, WZ-005, WZ-010 and Holes (`unknown-model-write-code`, `exit5-codes`). *Problem:* ids that break [F01 §2.5]'s
`<part>-<name>` form, and three "holes" that are naming decisions already made (E411; two E407 rows; `--pack-digest`/`pack_digest`).
*Fix:* apply `HOLES.md` §3 and §4 in the files.

**A1-43 (minor). `DATA` as a conflict class.** *Where:* [F07 §2.2], [F14 §6.8], [F19 §12.1]; [F12 §6.1]. *Problem:* F12 says `DATA`
has no code. *Fix:* drop it or define it once.

**A1-44 (minor). F18 cross-references.** *Where:* [F18 §2.2] ("[F08] and [F14] encode" the foreign-uid mark; [F14] open point 36
says it is not encoded); [F18 §2.15] against [F13 §3.9] (four I-F statements worded differently); [F18] open point 17 (pending-marker
shape superseded by F19's table). *Fix:* F13 cites F18's statements; correct §2.2; close open point 17.

**A1-45 (minor).** [F20 §1.5] says `unreadable` has no detail; [F18] added detail code 59. *Fix:* align.

**A1-46 (minor). Editorial inconsistencies in the format part.** [F04 §10] "every other byte identical" (the checksums differ);
[F07 §6.1] class codes 1–8 against [F06 §6.1] `ckey` 0–8 (same classes, two numberings); [F17 §4.3] counts commits per frame, [F10]
counts records (OP-10-03); [F17 §5.4] lists four runtime-only fold targets, [F09 §15.1] ten (`ANCESTRY` missing). *Fix:* align.

**A1-47 (minor). Stale OS statements.** [OS/README §4.2] "open for part 2: cross-volume copy or refusal" (decided by FS-4);
§1.3 and §3 say `ipc`, `spawn`, `term`, `test_host` are "not yet named" ([OS/proc §11–§13], [OS/shell §10] name them); [OS/fs
§6.4] writes a literal 60 s where `gc.delete-grace` governs. *Fix:* update.

**A1-48 (minor). LQ/envelope details.** §5.13's links grammar prints `| verify #N` while anchor rows print `verify aN` ([F18]
example `a31`); §5.16 writes `\u{XX}` where [F19 §2.5] writes `\u{h}` (no leading zeros; [LQ/errors §2.1] writes `\u{XX}` with "no
leading zeros"); §8.1's cursor layout is an offset table with non-numeric offsets and should be a sequence table ([F01 §2.4]).
*Fix:* one spelling for each.

**A1-49 (minor). Offset tables without a `total` row** ([F01 §2.4]): [OS/clock §3.1], [OS/proc §3.1], [OS/project §3.1], §3.3,
§7.1, [OS/fs §4.9.3] and the table at [OS/fs] L149. *Fix:* add the rows.

**A1-50 (minor). Citations.** [CFG] L969 and L985 cite `[RULES/delete-policy]` (the file is `delete-policy-matrix`); [CFG] L10,
L623, L668, L977 cite `[RULES/policy-keys]`, a planned file with no content yet. *Fix:* correct the name; write `policy-keys.md`
or cite the rows that exist.

**A1-51 (minor). E406's unleased text.** *Where:* [LQ/errors §5.5]; [F19 §11.3]; [RULES/role-write-policy] WZ-001; [AR §7.3];
[90 §4.3]. *Problem:* the design's text is one line, `this write needs a lease; an orchestrator presents its session lease with
--lease (mint it once per session: moirai claim --role orchestrator --session)` (152 B). LQ/errors and F19 split it into the
message and a `= help:` line while calling it verbatim; WZ-001 quotes the one-line form. The split teaches the mint equally well.
*Fix:* keep the split, record it as a deliberate rendering of the design text, and update WZ-001's note.

**A1-52 (minor). E408's JSON.** [LQ/errors §5.7] types `"key"` as a string and `"original"` as an object; [API §7.4] sends `null`
for a default key and for an entry without a commit, and the text `key <key> was used ...` has no rendering for a default key.
*Fix:* LQ/errors allows `null` and gives the default-key text (for example `the default key of this write was used ...`).

**A1-53 (minor). E304's help** says `(at most 8)`; the orchestrator and owner ceiling is 80 ([CFG] open point 13). *Fix:* render
the caller's ceiling.

**A1-54 (minor). The scalar `subtree()` signature.** *Where:* [LQ/std §2.9]; [LQ/canonical-ast] Table 5.3; [50 §2.6]. *Problem:* the
only signature table lists the relation `subtree(n, depth: int = 3)`; no LQ chapter gives the scalar built-ins' arities and
defaults, so `t IN subtree(#88)` (the card, `std.ready`) could be read as depth 3. *Fix:* a signature table for the scalar
built-ins, with `subtree(n [, depth])` unbounded by default.

**A1-55 (minor). Two duration grammars.** [LQ/lexical §5.6] accepts `s m h d w`, [CFG] `duration` accepts `ms s m h d`; [API
§5.1] takes LQ text for durations. An agent that writes `2w` into a config value gets CFG05. *Fix:* one unit set, or say so in
both chapters.

**A1-56 (minor). `SUBTASK_OF`.** [LQ/canonical-ast] C-8 asks F08 to add a forward-synonym list; [F08] open point 43 says LQ's
alias table carries it. *Fix:* LQ/canonical-ast §5.4 fixes the table (`SUBTASK_OF` → `CHILD_OF`) and closes C-8.

**A1-57 (minor). Ceilings stated as fixed where a hole governs.** [F19 §3.2] and [RULES/pack-classes] PE-004 print the Codex MCP
ceiling as 16,000 B, which [CFG] makes `HOLE(CFG-codex-mcp-result)` (candidates 16,000, 12,000, 8,000); [F19 §3.2] bounds generic
hook context at 8,000 B while PE-006 gives 10,000 B for every client. *Fix:* cite the hole; one hook ceiling.

**A1-58 (minor).** [LQ/std §1] numbers its paragraphs 1.1, 1.2, 1.4, 1.3. *Fix:* renumber.

**A1-59 (minor). FS-4 help text** (§2): `move it with a raw mv; links re-bind by evidence or show a proposal to confirm`.

**A1-60 (minor). Windows device names.** [OS/path] open point 3: extend rule P5's warning list with `CONIN$`, `CONOUT$`, and
`COM¹`–`COM³`, `LPT¹`–`LPT³` (superscript digits, which Windows also reserves).

**A1-61 (minor). COVERAGE maps conflicting chapters.** Row R-4 cites [F08 §10.3] and row R-10 [F06 §7.5.3], the two anchor
layouts of A1-2, and both count as mapped. *Fix:* until A1-2 closes, mark such rows as conflicting rather than mapped.

**A1-62 (minor). CFG diagnostics in the footer.** [CFG §6.2] renders `CFGnn: <message>` (7-byte prefix) in the footer of
[LQ/errors §4.2], whose continuation indent is "five spaces (the width of `Wnn: `)" and whose order covers only W and N codes.
*Fix:* state the indent and the place of CFG codes in the footer order.

## Coverage

| Item | Where |
|---|---|
| none | a review file specifies no [60 §2.5], R-n, F-n, X-F-n or [90 §10.1] row |

## Holes

None. The review proposes no value that a measurement decides; A1-29 and A1-31 ask the owner to confirm limits, and A1-57 asks
two chapters to cite an existing hole.

## Open points for the review

1. **Owner decisions this pass adds to the WP-80b list:** the link-marker limit (A1-29), the `--ids` cut page (A1-31), the
   header-extras remedy if LQ/envelope keeps its `DRY` fields (A1-30), gate 2 for checkpoint imports if A1-10 is closed by
   relaxing it, and the two author-flagged model decisions of A1-26.
2. **Ownership pattern.** Seven of the seventeen blockers are one layout written in two chapters. Pass 2 should check each
   repaired pair by grepping for every enum value and field name of the owning table in the citing chapter, not only the
   citation.
3. **Not re-checked in depth by this lens:** the row-by-row semantics of `merge-table.md`, `delete-policy-matrix.md` and
   `state-definition.md` against [AR] (lens S and R-MODEL's own pass), and the Annex P/R detection points of the grammar against
   [LQ/errors]; both were checked for structure and cross-references only.
