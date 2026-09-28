# 20 — R4 resolver constants (R-14)

| | |
|---|---|
| Title | The resolver-version constant table of R4: content functions (`is_text`, EOL normalisation, `oid`, normalised and non-trivial lines, fingerprints, the window hash, token winnowing, similarity), `fold_v1`, candidate eligibility, the file-cascade constants and per-OS rules, and the anchor capture and resolve constants — resolver version 1 |
| Chapter | [F20], `docs/spec/format/20-r4-resolver-constants.md` |
| Status | draft, pass 1 pending |
| Work package | WP-14b (R-SPEC-R), [PLAN §3.2] item 1; reconciled with WP-10 ([F01]) |
| Sources | [40 §2.11] R-14 (authoritative), with R-13, R-16, R-17, R-18 for the boundaries; [40 §2.2] (the closed `relink` vocabulary: `git-pair` and the other scored evidence tokens); [40 §2.3] (exact-byte identity, hard-link note); [40 §2.4] (fold, twins, "spelling differs on disk", reparse points, hard links); [40 §2.5] (`is_text`, `norm`, `oid`, "Why this function", "Streaming, bounded memory", `FPRINT`); [40 §2.6] (the stat quadruple, `FILEOBS` fields `verified_at`, `last_oid`, state, proposals); [40 §2.7] (anchor fields, authoring forms, BOM and U+FFFD, capture steps 1–5); [40 §2.9] (the states the rules produce); [40 §3.2] (`planned` binding); [40 §4.1] P2, P4, P6–P10; [40 §4.2] ("Quiescence", CAS, quiet mode); [40 §4.3] (steps 0–6, the copy rule, "Clock domains", the never-candidate patterns); [40 §4.4] (classification, thresholds, `replaced`, path reuse, automatic `path_moves`); [40 §4.5] (anchor cascade steps 1–8, watch semantics); [40 §4.6] (atomic saves, cloud-synced roots); [40 §4.8] (read-path limits); [40 §5.2] (gate rows G1–G4); [40 §5.7] (`window` in the image, `text-unavailable`); [40 §5.8] (ignore rules without git); [40 §8.3.2] P1, P3, P11 and the paragraph below the table; [40 §8.3.4] (replay targets); [80 §2.10] P3, P5, P6, P8, P9; [80 §2.11.1] (capabilities), [80 §2.11.2] (identity of `OsFileId`), [80 §2.11.3] (frontier, racy threshold), [80 §2.11.4] (E1–E8 per OS, rules 1–9), [80 §3.1] X-F7, X-F8; [AR §5e.3] (line-hash cap, `unverified (size)`), [AR §13] (`files.max-read-bytes`, `files.max-line-hashes`, `files.ignore`, `files.read.*`, "Never a key"), [AR §14] (per-OS creation-time row); [60 §2.5] (R4 row R-14; audit row "Resolver constants (R-14)"), [60 §3.1] ("Decisions fixed at M0 exit"); reviews `docs/spec/reviews/a1-S.md` S-12, S-13, S-16, S-17, S-18 and `a1-P.md` A1P-04, A1P-05, A1P-06, A1P-10 (dispositions in the open points); informative: [10 §3.3, §5.5, §5.8, §5.8b, §7, §8.1–§8.2], [11 §2.4, §2.7, §4.2–§4.3] |
| Depends on | [F01] (notation, hash set, `algo` registry, time values, order); [F07] (the anchor selector block of canonical item 10, R-10); [F08] (the anchor record, uid derivations, value type `oid`); [F09] (`PATHIDX`); [F10] (the fingerprint blob class, R-9); [F11] (`FILEOBS`, `TREES`, `DIRMAP`, `PENDING`, `FSINTENT`, `ANCHORRES`, `GITFACTS`, `OsFileId`, `VolumeCaps`); [F14] (`.moi` anchor lines, R-11); [F18] (state, detail and header strings, R-16; `relink`, R-17); [F19] (exit codes and refusals); [OS/project], [OS/path] (the per-OS calls behind `ProjectFs`); [CFG] (the `files.*` keys) |

## 1. Scope and conventions

### 1.1 What this chapter fixes

[40] R-14 reserves "the resolver-version constant table as a spec appendix". This chapter is that appendix. The R4
resolver ([40 §4], [AR §5e.3]) is a pure function of the inputs listed in §1.3 (I-F10). Everything the resolver uses
that is not an input is fixed here and together forms **resolver version 1**:

- the content functions: `is_text`, EOL normalisation, `oid` (§2.1–§2.4); anchor text, normalised and non-trivial lines
  (§2.5); fingerprint lines, the sketch line hash and the fingerprint value (§2.6); the window hash and the window value
  (§2.7); the span hash (§2.8); token winnowing (§2.9); the similarity and containment measures (§2.10);
- the case fold `fold_v1` and the path equivalence rules: twins, "spelling differs on disk", the normalization rule
  (§3);
- candidate eligibility: order, unbound paths, root containment, ignored output, cloud-only entries, trash locations,
  never-candidate names, denials (§4);
- the file cascade's constants and per-OS rules: time comparisons, stat tuples, identity, the checks at a present path,
  E1 and E3d–E8, similarity, classification, automatic `path_moves` entries, the 50 ms quiescence, the E6 window bounds,
  the E3d identity rule and the path-reuse check (§5);
- the anchor capture and anchor cascade constants (§6).

[40 §4.3–§4.5] stays normative for the order of the cascade steps; this chapter restates a step only as far as its
constants need context. It owns no storage layout except the byte contents of two values that are pure functions of
file content, the fingerprint value (§2.6.4) and the window value (§2.7.3), which the owning chapters store. Everything
else is cited: runtime rows [F11]; the fingerprint blob class [F10]; strings [F18]; keys [CFG]; the anchor record and
uid derivations [F08]; the selector block [F07]; anchor lines [F14]; per-OS calls [OS/project] and [OS/path].

No value of this chapter is a configuration key, an environment variable or a flag ([AR §13] "Never a key").

### 1.2 Notation

- **Bytes.** `b[i]` is the byte at index i (from 0); `b[i..j)` the bytes from i up to but excluding j; `len(b)` the
  length; `‖` concatenation. Integers are mathematical integers unless a width is given.
- **Hash functions** are [F01 §7.1]'s: XXH3-64 (seed 0, default secret), SHA-1, SHA-256, BLAKE3. `low(v, m)` is
  `v mod 2^m` of an unsigned integer value `v`; for an XXH3-64 value it equals the first m/8 bytes of its stored form
  read little-endian ([F01 §7.2]).
- **Whitespace.** `WS` is the byte set {`09`, `0A`, `0B`, `0C`, `0D`, `20`}.
- **Order.** "Path order" and "sorted" are [F01 §6.6]'s: bytewise, unsigned, a proper prefix first.
- **ASCII case-insensitive equality** `eqi(a, b)`: equal length and, byte by byte, equal after mapping `41`–`5A` to
  `61`–`7A`; every other byte compares exactly.
- **Characters.** `chars(x)` is the number of bytes of `x` outside `80`–`BF`. For valid UTF-8 it is the number of
  scalar values; for other bytes it is a fixed, total count.
- **UTF-8-safe cuts.** `cutp(x, n)` is the longest prefix of `x` of at most n bytes whose next byte, if any, is not in
  `80`–`BF`. `cuts(x, n)` is the longest suffix of `x` of at most n bytes whose first byte, if any, is not in `80`–`BF`.
  Neither splits a scalar value of valid UTF-8.
- **`basename(p)`, `dirname(p)`**: the bytes of a root-relative path after, and before, its last `/`; `dirname` is empty
  for a path at the root.
- **Rationals.** Every score, ratio and threshold in this chapter is an exact non-negative rational number. A decimal
  written as a threshold denotes the exact rational (0.29 is 29/100). Comparisons, differences and margins are
  evaluated exactly by cross-multiplication in integer arithmetic; every intermediate value of this chapter fits in
  `i128`. Floating point is never used by the resolver, the capture, or the model's implementation of either.
- **Time** ([F01 §5.7]): file timestamps are `unix_ns` values, each with a granularity G in nanoseconds (≥ 1): G =
  `max(10^gran, VolumeCaps.mtime_granularity_ns)` of the tree's volume, as [F11 §12.2] defines it; an `hlc` is `(unix_ms << 16) | counter`, and `hlc_ns(h) = (h >> 16) × 1,000,000`. Git committer
  times are whole seconds since the epoch, as the commit object states them.
- **States and details** are named by their frozen strings ([40 §2.9], [F18]); this chapter never defines a string.

### 1.3 The resolver version and the resolver's inputs

- **Value.** `resolver_version` is a `u16`. This chapter defines version **1**; 0 is invalid. The value is stamped in
  the anchor field `resolver` at capture ([40 §2.7], [F08]), in `FILEOBS.resolver_version` and the `ANCHORRES` key
  ([40 §2.6], [F11]), in the fingerprint value (§2.6.4), and in every rendered link result ([F18]). The meaning of the
  `.moi` anchor line's `v=` field is [F14]'s.
- **Bump rule.** Any change to a function, constant, pattern or per-OS rule of this chapter is a new resolver version.
  Filling a named hole before the `format-v1` tag is not a change: version 1 is what this chapter says at the tag.
  Building a reserved evidence source (E2, [40 §4.7]) is a new version.
- **Captured values are permanent.** An anchor's captured selectors (quote, prefix, suffix, end, window, hint,
  `span_hash`, occurrence) keep the values computed at capture under the version in its `resolver` field. A later
  resolver version must still evaluate the capture-dependent functions of that version when it resolves such an anchor
  (the window hash of §2.7, the header cut of §2.8, the normalisation of §2.5); it may add rules, never reinterpret
  stored bytes.
- **Runtime values of another version are absent.** A `FILEOBS` row, an `ANCHORRES` row or a fingerprint value computed
  under another version counts as absent and is recomputed.
- **Inputs** (disposition of review S-13). `resolve` is a pure function of: (a) the versioned link — the file node's
  fields, the anchors' selectors, the root node's `path_moves`; (b) the tree snapshot — its files and directories with
  their metadata and the volume's `VolumeCaps`, and with git, HEAD H, τ(H) and the object store; (c) the tree's runtime
  rows — `FILEOBS`, `PENDING`, `FSINTENT`, `TREES` (epochs, first-settle flag, last settle `hlc`), `DIRMAP`; (d) the
  resolver version; (e) the command's budgets and the `files.*` keys. The caches `FPRINT`, `GITFACTS` and `ANCHORRES`
  are output-neutral: a result is identical with or without them. Budgets and keys can only turn an answer into
  `unverified` (§1.5), never into another target ([40 §4.1] P7). The model (WP-92) takes the same tuple. I-F10's text is
  [F13]'s and [F18]'s.

### 1.4 Fixed values and named holes

[60 §3.1] lists "R4's resolver constants (R-14) and anchor layout (replay corpora)" among the decisions fixed at M0
exit, and [PLAN §3.2] has WP-76 propose them to this chapter and WP-81a fill them. [40 §4.4–§4.5] states values for
them as "resolver v1 constants". This chapter reconciles the two (open point 1):

- **Named holes** (§2.5 of [F01]) are the values an M0 measurement can decide:
  - the anchor capture and anchor cascade constants, which replay row 2 of [40 §8.3.4] (the citation sample, WP-76)
    exercises — their first candidate is [40]'s value;
  - the winnowing k and w, which [40] leaves unnamed — decided by WP-66's measurements and WP-76;
  - the NTFS creation-time class, the ChangeTime-on-rename fact and the clock-skew margin — decided by measurements 15
    and 22 (WP-55, WP-52).
- **Draft values.** Until WP-81a fills a hole, its first candidate is the draft value that WP-62, WP-64, WP-66, WP-77 and
  WP-92 implement. No fixture depends on a draft value ([F01 §2.5]). A fill that differs is a specification change
  those WPs follow.
- **Fixed values.** Every other number is written. The file-level thresholds of [40 §4.4] are exercised by replay rows
  5–7, which gate M6, not M0; they keep [40]'s values, measured in [10 §5.8, §7]. The E6 bounds, the quiescence delay
  and the pattern lists are [40]'s. If a replay target at M0 fails because of a fixed value, that is a specification
  finding for WP-81a, not a fill.

### 1.5 Evidence classes and unavailable inputs

- **Evidence classes**, strongest first: `exact`, `strong`, `copy` (an identical-copy proposal, [40 §4.3] copy rule),
  `weak`, `none`. Only `exact` re-binds automatically, and `strong` too under `files.policy.auto = strong` ([CFG]);
  every other class is a proposal or nothing ([40 §4.4]).
- **Unavailable.** A step whose input cannot be read yields `Unavailable(r)` with a reason r ∈ {`budget`, `size`,
  `cloud-only`, `unreadable`, `unstable`, `git`, `no tree`, `commit not in this repository`}. A source that is
  Unavailable contributes nothing: it never yields a candidate, never proves absence, and never changes a rule. A
  three-valued `oid` test that is *unknown* (§2.3) contributes nothing in the same way.
- **`missing` needs every source.** If the cascade would end `missing`, and a source that applied to the link was
  Unavailable, the link is `unverified` with the reason of the first such source in source order (§5.5) instead
  ([40 §4.3] step 5). An anchor whose file content is Unavailable is `unverified` with that reason. The reasons render as
  [F18 §4.6]'s `unverified` details: `budget` 53, `cloud-only` 54, `commit not in this repository` 55, `no tree` 56,
  `git` 57, `size` 58 and `unreadable` 59; `unstable` renders `budget` (pass 1, A1-45; [F18] adds 59 to [40 §2.9]'s
  closed set, with 60 `unmapped root` and 61 `oid algorithm differs`, for the owner's sign-off, [F18] open point 12;
  open point 22).

## 2. Content functions

Every function here is over bytes supplied by `ProjectFs` ([OS/project]) or by the git object reader. The functions
open nothing ([OS/README §2], review A1P-10).

### 2.1 Byte statistics and `is_text`

`is_text` is git's `convert_is_binary` over git's `gather_stats`, applied to the whole content ([40 §2.5]; [D, git
`convert.c`]). For a byte string b of length n:

```
crlf = lonecr = nul = printable = nonprintable = 0
i = 0
while i < n:
    c = b[i]
    if c == 0x0D:
        if i + 1 < n and b[i+1] == 0x0A: crlf += 1; i += 2; continue
        lonecr += 1; i += 1; continue
    if c == 0x0A: i += 1; continue
    if c == 0x7F: nonprintable += 1
    elif c < 0x20:
        if c in {0x08, 0x09, 0x0C, 0x1B}: printable += 1          # BS, HT, FF, ESC
        else:
            if c == 0x00: nul += 1
            nonprintable += 1
    else: printable += 1                                           # 0x20..0x7E and 0x80..0xFF
    i += 1
if n >= 1 and b[n-1] == 0x1A: nonprintable -= 1                   # a final ^Z is not counted
is_text(b) = (lonecr == 0) and (nul == 0) and ((printable >> 7) >= nonprintable)
```

- `>>` is an integer shift (division by 128, rounding down). A final `1A` was counted as non-printable, so the
  decrement never goes below 0.
- CR and LF are counted in neither `printable` nor `nonprintable`. A CR followed by LF is one CRLF pair; the pairs never
  overlap.
- The statistics cover the whole content, never a prefix. (git's diff heuristic — no NUL in the first 8,000 bytes — is
  not this function, [41 m1].)
- The empty content is text.

### 2.2 EOL normalisation

```
norm(b) = is_text(b) ? b with every CRLF pair (0D 0A) replaced by 0A : b
```

For text content `len(norm(b)) = n − crlf`. A text content has no lone CR, so `norm(b)` of a text content contains no
`0D`. Nothing else is changed: no BOM is removed, no trailing newline added or removed.

### 2.3 `oid` and its algorithm

```
oid_H(b) = H("blob " ‖ dec(len(norm(b))) ‖ 00 ‖ norm(b))
```

`dec` is [F01 §6.5]'s decimal text. `H` is SHA-1 (algorithm `sha1`) or SHA-256 (`sha256`) of the `algo` registry
([F01 §7.5]); the stored value is the `algo` byte and the digest ([F08], [40] R-1).

- **Algorithm of a root** ([40 §2.5], review S-18). A(R) is a per-root, `init`-fixed parameter: the `project` root uses
  the object format of the store's repository as read at `init` (`extensions.objectFormat`; `sha1` when the store has no
  repository), for the store's life; every named root and `abs` use `sha1`. Every `oid` the resolver or a capture
  computes for a file of root R — the node's `oid`, `FILEOBS.last_oid`, an anchor's `blob`, a `PENDING` captured `oid`,
  an `FPRINT` key — uses A(R). A(`project`) is one `u8` of [F01 §7.5]'s `algo` registry (1 `sha1` or 2 `sha256`),
  recorded once at `init` and kept by `restore` and `repair`; a reader takes it from the store, never from the
  repository, so adding or converting a repository later changes nothing. It is `HEAD.project_oid_algo`, the `u8` at slot
  offset 1072 ([F04 §5.16]), an `init`-fixed parameter under [F17 §2.2] IP-1–IP-3 (open point 27).
- **Comparison.** Two `oid` values are equal iff their `algo` bytes and digests are equal; `none` equals nothing. Two
  values of different algorithms are never equal and never different: the pair is **content unknown** ([40 §2.5]). A
  test "`oid(q) ∈ S`" for a set S of stored values is three-valued: *true* if `oid_A(R)(q)` equals an element of S;
  *false* if it equals none and every element of S has algorithm A(R); *unknown* otherwise. A rule that needs "∈" holds
  only on *true*, a rule that needs "∉" (the `replaced` test, "changed since …") only on *false*; *unknown* makes the
  evidence line contribute nothing. The copy rule, `last_oid` and the `replaced` test follow this ([40 §2.5]).
- **Symbolic links** ([80 §2.10] P8). The `oid` of a symbolic link is `H("blob " ‖ dec(len(t)) ‖ 00 ‖ t)`, where t is
  the link's target text as [OS/path] reads it (`/` separators, the bytes git records). `norm` is not applied. A
  directory has no `oid`.
- **Git evidence never uses `oid`.** Git blob ids are read from git trees ([40 §2.5]); `oid` equals a git blob id only
  where git converts a file exactly as `norm` does. The one rule that compares an `oid` with a git blob id is the twin
  rule's content match ([80 §2.11.4] rule 2, §3.5), where a mismatch caused by git's conversion yields `ambiguous`, never
  a wrong target.

*(Informative)* Worked values (SHA-1 unless stated):

| Content bytes | `is_text` | `norm` | `oid` |
|---|---|---|---|
| (empty) | true | (empty) | `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391` |
| `68 65 6C 6C 6F 0D 0A` ("hello" CR LF) | true | `68 65 6C 6C 6F 0A` | `ce013625030ba8dba906f756967f9e9ca394464a` |
| same | true | same | SHA-256: `2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4` |
| `61 0D 0A 62` | true | `61 0A 62` | `0a207c060e61f3b88eaee0a8cd0696f46fb155eb` |
| `61 0D 62` (lone CR) | false | unchanged | `2fe40ba389048204a83882bc3f75bf2188db6d47` |
| `78 0D 0A 1A` (final ^Z) | true (`nonprintable` 1 − 1 = 0) | `78 0A 1A` | `2f484e3cd37e421218bc8a9929df89ceb50dbb1a` |
| 127 × `41`, then `01` | false (127 >> 7 = 0 < 1) | unchanged | — |
| 128 × `41`, then `01` | true (1 ≥ 1) | unchanged | — |
| `61 00 62` | false (NUL) | unchanged | — |

### 2.4 Reading project content

- **Availability.** The content of a file is available to a command only if:
  1. its size is at most `files.max-read-bytes` ([CFG]); otherwise `Unavailable(size)` ([40 §2.5]);
  2. it is not a cloud-only entry (§4.5) — an automatic path never reads one; an explicit verb reads it only with
     `--allow-hydrate` — otherwise `Unavailable(cloud-only)`;
  3. the open and every read succeed; a denial (Windows `ERROR_ACCESS_DENIED` 5, Unix `EACCES`, `EPERM`), a sharing
     or lock violation (Windows 32, 33) or any other read error gives `Unavailable(unreadable)` (§4.8). Of the
     `ProjectFs` kinds ([OS/project §2.3]): `CloudOnly` — the entry is, or became between its stat and the open, a
     cloud placeholder ([OS/project §5.5] steps 1 and 3; pass 1, P1-38) — gives `Unavailable(cloud-only)` as item 2;
     `IsSymlink` is no error: the content is the link's target text, read by `read_link`, and its `oid` is §2.3's;
     `IsDirectory`, `OutsideRoot` and `Stale` are read errors of this item;
  4. the content is stable (below); otherwise `Unavailable(unstable)`.
- **Two passes, one handle** (disposition of review A1P-05). Both passes read through one handle from offset 0. Before
  pass 1 and after pass 2 the handle's size and last-write time are read. Pass 1 computes the statistics of §2.1, the
  raw length n1, `N1 = len(norm(b))`, `r1 = XXH3-64(b)` over the raw bytes, the line hashes (below), and the
  fingerprint (§2.6). Pass 2 streams the `oid` header and `norm(b)` into the hash of A(R) (§2.3) and recomputes n2 and
  r2.
  The content is **stable** iff the two sizes, n1 and n2 are equal, the two last-write times are equal, `r1 = r2`, and
  pass 2 emitted exactly N1 normalised bytes. An unstable read is repeated once from the start (`READ_RETRIES` = 1); a
  second unstable read gives `Unavailable(unstable)`.
- **Buffers never change a result.** The product reads through one fixed 128 KiB buffer per thread ([40 §2.5]); a CR
  at the end of a buffer is classified with the next buffer's first byte, and the final-`1A` adjustment uses the last
  byte of the content. Results equal the whole-buffer definitions for every buffer size.
- **Line-hash array.** For a text content, pass 1 records `XXH3-64(nl(l))` (§2.5) for each line l, up to
  `files.max-line-hashes` lines ([CFG]). The window steps (§6.2, §6.3) and `lines` anchors (§6.5) use this array; when
  the content has more lines than the key allows, those steps are `Unavailable(size)` ([AR §5e.3]). `oid`, fingerprints,
  similarity and winnowing never depend on the key.

### 2.5 Anchor text, lines and trivial lines

- **Anchor text.** `atext(b)` is defined only when `is_text(b)`: it is `norm(b)` with one leading `EF BB BF` removed, if
  present. Span anchors exist only on text content (§6.1, §6.5); a `file` anchor works on any content.
- **Lines.** `lines(t)` splits t at every `0A` and drops the last piece if it is empty. Line numbers start at 1. A text
  whose last byte is not `0A` has a last line without a terminator; the empty text has no lines. `norm` and the BOM
  removal change no line count, so line numbers of `atext(b)`, of `norm(b)` and of the raw content agree.
- **Normalised line.** `nl(l)` is l with every leading and trailing byte of `WS` removed ([40 §2.7]: "CRLF→LF, lines
  trimmed"). Nothing inside the line is changed.
- **Normalised anchor text.** `N(t) = nl(l1) ‖ 0A ‖ nl(l2) ‖ … ‖ 0A ‖ nl(lm)` over the m lines of t, with no trailing
  `0A` (the empty string when m = 0). `start(i)` is the offset in N of line i's first byte and `end(i)` the offset just
  after its last byte. Every quote search, every prefix and suffix, and every fuzzy match of §6 is over N.
- **Span text.** `ST(s, e) = N[start(s) .. end(e))` for lines s ≤ e.
- **Trivial line** (the "non-trivial line" gap of [PLAN §3.3]). A line l is **trivial** iff every byte of `nl(l)` is in
  `WS` ∪ {`{` `7B`, `}` `7D`, `(` `28`, `)` `29`, `[` `5B`, `]` `5D`, `;` `3B`, `,` `2C`}. A blank line is trivial.
  Every other line is **non-trivial**. [40 §2.7] names "lines that are blank or contain only braces"; this chapter reads
  "braces" as the bracket and separator bytes above (open point 4).

### 2.6 Fingerprint lines and the fingerprint value

#### 2.6.1 Fingerprint lines

For a text content with anchor text t: for each line l of `lines(t)`, `f = collapse(nl(l))`, where `collapse`
replaces every maximal run of `WS` bytes by one `20`. f is a **fingerprint line** iff `chars(f) > 3` (`FP_MIN_CHARS` = 3:
"lines ≤ 3 characters dropped", [40 §2.5]; characters, not bytes, open point 5). Fingerprint lines keep their
multiplicity.

#### 2.6.2 Line hashes

- **Sketch line hash**: `sh(f) = low(XXH3-64(f), 32)`, a u32.
- **Full line hash**: `fh(f) = XXH3-64(f)`, a u64, used by the exact measures (§2.10.1).

#### 2.6.3 Quantities

For a text content b with anchor text t:

| Quantity | Definition |
|---|---|
| `nlines` | the number of lines of `lines(norm(b))` ("normalised lines") |
| `nbytes` | `len(norm(b))` |
| `weight` | the sum of `len(f)` over all fingerprint lines, with multiplicity |
| V | the set of values `sh(f)` over all fingerprint lines |
| `sketch` | the `min(64, #V)` smallest values of V, ascending, where #V is the number of values in V (`SKETCH_K` = 64, `SKETCH_BITS` = 32; "bottom-64 u32", [40 §2.5]) |
| `distinct` | #V when #V ≤ 64; otherwise `max(65, ⌊63 × 2^32 / (s64 + 1)⌋)` with s64 the largest sketch value, and the flag `distinct_estimated` set |

- A streaming implementation knows whether `|V| > 64`: it saw a value outside the final sketch.
- The `distinct` estimate is the k-minimum-values estimator with k = 64. It keeps the fingerprint's memory constant
  whatever the file size, and it depends on no key (open point 6).
- A fingerprint exists only for text content with `nbytes < 2^32`; binary content has none.

#### 2.6.4 The fingerprint value

The fingerprint value is a pure function of the content bytes. [F10] (R-9, the fingerprint blob class) and [F11]
(`FPRINT`, keyed by `oid`) store it; its bytes are these (sequence table, [F01 §2.6]):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `ver` | `u16` | always | the resolver version that computed it: 1. Another value makes the fingerprint absent (§1.3) |
| 2 | `n_sketch` | `u8` | always | number of sketch entries, 0–64 |
| 3 | `flags` | `u8` | always | bit 0 `distinct_estimated`; bits 1–7 reserved-zero ([F01 §10]) |
| 4 | `nlines` | `u32` | always | §2.6.3 |
| 5 | `nbytes` | `u32` | always | §2.6.3 |
| 6 | `weight` | `u32` | always | §2.6.3; at most `nbytes`, so it fits |
| 7 | `distinct` | `u32` | always | §2.6.3, at most 2^32 − 1 |
| 8 | `sketch` | `n_sketch` × `u32` | always | the sketch, strictly ascending |

Length `20 + 4 × n_sketch`, at most 276 bytes ("about 300 B per content version", [40 §2.5]). The value is invalid if
`n_sketch > 64`; if the sketch is not strictly ascending; if `n_sketch < 64` while `distinct ≠ n_sketch` or the flag is
set; if `n_sketch = 64` with the flag clear while `distinct ≠ 64`, or with the flag set while `distinct < 65`; or if a
reserved bit is set.

#### 2.6.5 Tiny content

Content X is **tiny** ([40 §4.4]) iff: X is text and (`nlines < 5` or `nbytes < 64`) (`TINY_LINES` = 5, `TINY_BYTES`
= 64); or X is binary and its length is below 64. A file node F is tiny iff the fingerprint of `last_oid` (else of `o`)
exists and says so, or no such fingerprint exists and `F.bytes < 64`. Tiny content carries no content signal: §5.5 and
§5.14 restrict it.

### 2.7 Window hash and the window value

#### 2.7.1 Window hash

`wh(l) = low(XXH3-64(nl(l)), 16)`, a u16 (`WINDOW_BITS` = 16, "u16 hashes", [40 §2.7]). It is computed only for
non-trivial lines. Its input is the normalised line of §2.5 (trimmed, not collapsed), the same normalisation as quotes
(open point 7).

#### 2.7.2 The window around a span

For lines s ≤ e of a text t and `WIN` = HOLE(F20-window-lines) (draft 16):
- `before(s)`: the window hashes of the last `min(WIN, k)` non-trivial lines with index < s, in file order, where k is
  the number of such lines;
- `after(e)`: the window hashes of the first `min(WIN, k′)` non-trivial lines with index > e, in file order.

The window of an anchor is taken around its **quote span** (§6.1): the lines its quote covers. At resolve time, the
window of a candidate is taken the same way around the candidate's quote span in the current text.

#### 2.7.3 The window value

The window value W is the byte string that the anchor's `window` selector holds ([40 §2.7]: "up to 16 non-trivial
normalised lines before and after the span, as u16 hashes, plus the span's offset in the window, ≤ 68 B"). Sequence
table:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_before` | `u16` | always | number of hashes before the span, 0–`WIN`; it is also the span's offset in the window |
| 2 | `n_after` | `u16` | always | number of hashes after the span, 0–`WIN` |
| 3 | `before` | `n_before` × `u16` | always | `before(s)`, in file order |
| 4 | `after` | `n_after` × `u16` | always | `after(e)`, in file order |

Length `4 + 2 × (n_before + n_after)`, at most 68 bytes at the draft `WIN` = 16. A value with a count above `WIN` is
invalid. [F07] (R-10) carries W as the `window` selector and [F14] (R-11) writes it as base64url without padding
(open point 8).

### 2.8 Span hash and header text

- `span_hash` for `watch = span` is `XXH3-64(ST(h1, h2))` over the anchor's hint lines [h1, h2] ([40 §2.7]: "xxh3-64 of
  the normalised span"). Trivial lines inside the span are included.
- **Header text.** For a line l: `header(l, symbol)` is `nl` of the bytes of l before its first `{` (`7B`) or `;`
  (`3B`), or of all of l if it has neither; `header(l, heading)` is `nl(l)`. The header line of an item is the first
  line the scope scanner reports for it ([40 §2.7.1]).
- `span_hash` for `watch = header` is `XXH3-64(header(l, kind))` over the full header of the item's header line.
- A `file` anchor has no span hash; [F08] states its absent encoding. A `file` anchor with `watch = span` compares
  contents instead (§6.5).

### 2.9 Token winnowing

Token winnowing ([10 §5.8b], Schleimer, Wilkerson and Aiken 2003) is computed only in stage 2 of the similarity search
(§5.14), when the old content is readable, over the anchor text t of a text content.

- **Tokens.** A byte is a *word byte* if it is `30`–`39`, `41`–`5A`, `61`–`7A`, `5F` or ≥ `80`. The token sequence
  T = t0 … t(m−1) is: every maximal run of word bytes is one token; every byte that is neither a word byte nor in `WS`
  is a one-byte token; `WS` bytes separate tokens and are not tokens.
- **k-gram hashes.** With `K` = HOLE(F20-winnow-k) (draft 5): `th(ti) = XXH3-64(ti)`; for j = 0 … m − K,
  `g_j = XXH3-64(u64(th(tj)) ‖ … ‖ u64(th(t(j+K−1))))`, each `u64` little-endian. There are G = m − K + 1 k-grams
  (none when m < K).
- **Selection.** With `W` = HOLE(F20-winnow-w) (draft 4): when G = 0 the fingerprint set is empty; when 1 ≤ G < W there
  is one window over all k-grams; otherwise the windows are `g_j … g_(j+W−1)` for j = 0 … G − W. In each window the
  minimum value is selected, the rightmost position among equal minima.
- **Fingerprint set** `FW(t)`: the set of selected values.
- **Measure.** `J(A, B) = |FW(A) ∩ FW(B)| / |FW(A) ∪ FW(B)|`, 0 when the union is empty. It is defined only when both
  sets have at most `EXACT_LIMIT` = 65,536 values (§2.10.5).

### 2.10 Similarity and containment measures

"Old" is the recorded content (the node's last known content, or git's old blob) and "new" the candidate.

#### 2.10.1 Exact measures (both contents read)

`M(X)` is the multiset of pairs `(fh(f), len(f))` over the fingerprint lines of X; `c_X(κ)` the multiplicity of pair κ;
`w(X) = Σ c_X(κ) × len(κ)`; `inter(A, B) = Σ_κ min(c_A(κ), c_B(κ)) × len(κ)`.

| Measure | Definition | Name in [40] |
|---|---|---|
| `oin(A, B)` | `inter / w(A)` | old-in-new containment |
| `nio(A, B)` | `inter / w(B)` | new-in-old containment |
| `sym(A, B)` | `inter / max(w(A), w(B))`, which equals `min(oin, nio)` | symmetric similarity |

A measure whose denominator is 0 is 0. *(Informative: [10 §5.7]'s pairs, for example sym 0.47 with old-in-new 0.47 and
new-in-old 0.61, satisfy sym = min.)*

#### 2.10.2 Sketch estimates (old side known only by its fingerprint)

For an old content A known by its fingerprint (sketch `S_A`, `distinct` D_A) and a new content B read in full: `hit` is
the number of values of `S_A` that equal `sh(f)` for some fingerprint line f of B (computed while streaming B, with
`S_A` as a 64-entry sorted array); D_B is B's `distinct` (§2.6.3).

| Estimate | Definition |
|---|---|
| `eoin(A, B)` | `hit / #S_A`, where #S_A is the number of values in `S_A`; 0 when `S_A` is empty |
| `enio(A, B)` | `min(1, eoin × D_A / D_B)`; 0 when D_B = 0 |
| `esym(A, B)` | `min(eoin, enio)` |

The estimates are count-based over distinct lines, as [10 §8.2] step 7 specifies (σ ≈ 0.06 [I]). The `replaced` test
(§5.4.1) and E8 (§5.13) always use these estimates, even when the old content is readable, so their outcome does not
depend on whether git has the old blob (open point 9).

#### 2.10.3 Sketch resemblance (stage 1)

For two fingerprints with sketches S_A and S_B: X is the `min(64, |S_A ∪ S_B|)` smallest values of `S_A ∪ S_B`;
`r(A, B) = |X ∩ S_A ∩ S_B| / |X|`, 0 when X is empty (the bottom-k resemblance estimator).

#### 2.10.4 Pair score

For an old content A and a new content B, the **pair score** is:
- when both contents are read and the exact limit holds: `score = max(sym(A, B), J(A, B))`, with J omitted when it is
  not defined ([40 §4.3] step 4: "Score = max(line measure, token winnowing) when the old blob is available");
- when A is known only by its fingerprint, or the exact limit fails: `score = esym(A, B)`.

The containments reported with a score are `oin` and `nio` when exact, else `eoin` and `enio`.

#### 2.10.5 The exact limit

`EXACT_LIMIT` = 65,536: the exact measures are computed only when each side has at most 65,536 fingerprint lines, and J
only when each winnowing set has at most 65,536 values. It is a resolver constant, not the key `files.max-line-hashes`
(whose default it equals), so similarity scores never depend on configuration (open point 10).

## 3. `fold_v1` and path equivalence

### 3.1 Definition

```
fold_v1(x) = NFD( CF( NFD(x) ) )          at Unicode 17.0.0
```

x is a sequence of Unicode scalar values (a valid UTF-8 path, I-F8); the result is written as UTF-8.

- **NFD** is Normalization Form D of the Unicode Standard §3.11 and UAX #15 at version 17.0.0:
  - full canonical decomposition: every code point with a canonical decomposition mapping in `UnicodeData.txt` field 5
    (a mapping without a `<…>` tag) is replaced by its mapping, recursively; Hangul syllables U+AC00–U+D7A3 are
    decomposed arithmetically (SBase `AC00`, LBase `1100`, VBase `1161`, TBase `11A7`, LCount 19, VCount 21, TCount 28,
    NCount 588, SCount 11,172); compatibility mappings are never applied;
  - canonical ordering: in every maximal run of code points whose `Canonical_Combining_Class` (`UnicodeData.txt`
    field 3) is not 0, a stable sort by that class.
- **CF** is full case folding: each code point c is replaced by the mapping of the `CaseFolding.txt` line for c with
  status `C` or `F`, if there is one; lines with status `S` or `T` are ignored; every other code point is unchanged.
  (A code point has at most one `C` or `F` line.)
- The composition equals the Unicode Standard's canonical caseless match normalisation of D145, `NFD(toCasefold(NFD(X)))`,
  with full case folding ([80 §2.10] P6).
- Code points unassigned in Unicode 17.0.0 map to themselves (no decomposition, class 0, no folding). `fold_v1` never
  changes when a later Unicode version assigns them; a different fold is a new function and a new resolver version.

*(Informative)* Examples:

| x | `fold_v1(x)` | Why |
|---|---|---|
| `Plan.md` | `plan.md` | C folding |
| `ß` U+00DF, `ẞ` U+1E9E | `ss` | F folding |
| `İ` U+0130 | U+0069 U+0307 | NFD gives U+0049 U+0307; C folds U+0049 |
| `café` NFC (U+00E9), `café` NFD (e U+0301) | `cafe` U+0301 | both decompose to the same sequence |
| `Å` U+212B ANGSTROM SIGN | `a` U+030A | canonical decomposition to U+0041 U+030A, then C folding |
| `Ω` U+2126 OHM SIGN | U+03C9 | canonical singleton to U+03A9, then C folding |
| `ﬃ` U+FB03 | `ffi` | F folding |
| `ǅ` U+01C5 | U+01C6 | C folding; its compatibility decomposition is not applied |
| `한` U+D55C | U+1112 U+1161 U+11AB | Hangul decomposition |

### 3.2 Inputs and conformance

- The normative inputs are `UnicodeData.txt` and `CaseFolding.txt` of Unicode 17.0.0 in `fixtures/ucd/17.0.0/`, pinned
  by SHA-256 in its `INDEX.md` (WP-61, [PLAN §6.2] R6). `NormalizationTest.txt` of the same version is the conformance
  input for NFD. The Unicode-3.0 licence and NOTICE section follow [PLAN §2.4].
- Conformance (the condition of review a1-S on R6): the product's generated tables reproduce the NFD column of every
  `NormalizationTest.txt` line, every `C` and `F` line of `CaseFolding.txt`, and the model's independent derivation
  (WP-92) over every scalar value U+0000–U+10FFFF except the surrogates.

### 3.3 Where `fold_v1` is used

- the key order of `PATHIDX`, `(root, fold_v1(path), path)` ([40] R-8, [50] F13, [F09]): case and normalization variants
  are adjacent, so a collision probe is one index step;
- twin-candidate grouping (§3.5);
- the sibling check of [80 §2.10] P5 ([OS/path], [CFG] `files.portable-names`);
- the ref-name rule of [80] X-F9 ([F12]).

`fold_v1` is **never** used for identity: uids derive from exact bytes ([40 §2.3], [F08]). It is a superset relation:
it merges every pair NTFS, APFS or an ext4 casefold directory merges (ß = ss included), and more; what a directory really
merges is observed at resolve time (§3.5).

### 3.4 Canonical equivalence

`ceq(a, b)` ⟺ `NFD(a) = NFD(b)` (NFD as in §3.1, no folding). For every a and b, `NFC(a) = NFC(b)` ⟺ `ceq(a, b)`, so the
normalization rules of [80 §2.11.4] rule 2, which the design states with NFC, are evaluated with NFD and the resolver
needs no composition table.

### 3.5 Twin sets ([80 §2.11.4] rule 2)

- **Spellings.** In a view V and tree T with HEAD tree τ(H), the spellings of a root are the paths of its live file
  nodes with status `present` or `planned` on V, and, with git, the blob and link paths of τ(H) under the root.
- **Twin candidates.** A node F's twin-candidate group is the set of spellings s with `fold_v1(s) = fold_v1(F.path)`.
  It matters only when it has at least 2 members.
- **Twin sets.** Among the members that stat successfully in T through their own spelling, members that denote the same
  file (equal `OsFileId`, §5.3) form a class; every class with at least 2 members, at least one of them a node's path,
  is a **twin set**. This is the directory's actual equivalence as the volume observes it ([80 §2.11.4] rule 2): a
  case-insensitive directory resolves `docs/Plan.md` and `docs/plan.md` to one file, a case-sensitive one to two, and
  NTFS resolves an NFC and an NFD `café.md` to two. Members that do not stat are not twins and resolve normally.
  (Where a read has no id — Windows `GetFileAttributesExW` — the ids of the group's members are read by an
  attribute-only open; [OS/project].)
- **Resolution.** For a twin set Z, let c be the file all its members denote. For each member z its recorded contents
  are R(z) = {the blob id at z in τ(H), if z ∈ τ(H)} ∪ {`FILEOBS.last_oid` of z's node in T, if z is a node's path and
  the row exists}. A member **matches** iff c's content equals some value of R(z) (compared as §2.3 prescribes).
  - Exactly one member matches: it resolves normally (the cascade continues for it as a present path). Every other
    member that is a node's path is `missing (not representable on this OS)` and runs no evidence source.
  - No member or several members match: every member that is a node's path is `ambiguous (normalization collision)`
    if all members of Z are pairwise `ceq`, else `ambiguous (case collision)`.
  - c's content is Unavailable: every member that is a node's path is `unverified` with that reason.
- No link is `ok` on a spelling match alone ([40 §2.4]).

### 3.6 Spelling differences outside twin sets ([80 §2.11.4] rule 2)

- **Case- or normalization-only difference.** p stats in T, is in no twin set, and T's on-disk spelling of p (every
  component as the directory holds it: a settle's enumeration, `GetFinalPathNameByHandleW` on Windows, or
  `FILEOBS.path_seen` when the read tuple equals the row, §5.2) differs from p bytewise: the link is `ok` with the
  detail `spelling differs on disk`, and nothing is written.
- **git/case.** At a settle in a writer tree only: when p ∉ τ(H) and exactly one path q ∈ τ(H) has `fold_v1(q) =
  fold_v1(p)`, q ≠ p, and q denotes the same file as p in T, the settle re-binds to q with provenance `git/case` ([F18]
  R-17). Two or more such q: no re-bind (the twin rule applies to them).
- **Normalization rule.** p is absent; its parent directory D = `dirname(p)` exists; D is normalization-sensitive
  (`VolumeCaps` not normalization-insensitive, and on Linux no casefold flag on D). Let E be the entries e of D with
  `ceq(e, basename(p))`, `e ≠ basename(p)`, and `D/e` unbound (§4.2):
  - |E| = 1: the link is `ok` with the detail `normalization differs on disk`, resolved at `D/e`; nothing is written;
  - |E| ≥ 2: `ambiguous (normalization collision)`;
  - |E| = 0: the cascade continues.
  This check runs before the tree gate (§5.5) and applies to the last component only (open point 11).

## 4. Candidate eligibility

A path q is a **candidate** for file node F only if it passes every test of this section ([40 §4.1] P4, P6; [40 §4.3]
step 3), §4.9's representability test first.

### 4.1 Candidate order ([80 §2.11.4] rule 4)

Every candidate list is sorted in path order before any selection, uniqueness test or tie check, because enumeration
order differs by file system. Where a list is ranked by a score, its order is (score descending, path ascending); for
anchors, (score descending, byte offset ascending). A tie is never broken by this order to pick a winner: it only orders
the listing ([11 §4.3]). An `ambiguous` state lists at most the first 3 candidates, and `FILEOBS` keeps at most 3
proposals (`LIST_MAX` = 3, [40 §2.6, §4.4]).

### 4.2 Unbound paths ([40 §4.1] P4)

q is **bound** iff some live file node G ≠ F with status `present` or `planned` on the reading view has a path g such
that g = q bytewise, or g and q both stat in T and denote the same file (equal `OsFileId`). A bound path is never a
candidate. This also excludes the other names of a hard-linked file that another node holds ([40 §2.4]).

### 4.3 Inside the root

q is inside root R iff q's canonical path ([80 §2.10] P9: absolute, `/`-separated, the on-disk spelling of every
component, the Windows drive letter upper-cased) has R's canonical root as a proper prefix ending at a component
boundary. A path reached through a symbolic link or junction resolves to its target; resolution never follows one out of
the root ([40 §2.4]). A path located by id outside R is never a candidate; when E3 or E3d locates one, F renders
`missing` with the place ([40 §4.3] step 5, [F18]).

### 4.4 Ignored output

q is **ignored** iff q ∉ τ(H) (or T has no git) and the ignore matcher matches q or one of its ancestor directories.
The matcher has git's semantics over `.gitignore` files, `.git/info/exclude` and `core.excludesFile`; without git, over
`.gitignore`-format files if the root has any, else over `files.ignore` ([CFG]; defaults `target/`, `node_modules/`,
`build/`) ([40 §5.8], WP-61b). An ignored path is never a candidate ([40 §4.1] P6). A tracked path is never ignored.

### 4.5 Cloud-only entries

- **Windows**: an entry is cloud-only iff its attributes carry any of `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`
  (`0x00400000`), `FILE_ATTRIBUTE_RECALL_ON_OPEN` (`0x00040000`) or `FILE_ATTRIBUTE_OFFLINE` (`0x00001000`) [D]. An
  automatic path never enumerates a directory that carries `RECALL_ON_DATA_ACCESS` ([40 §4.6]).
- **macOS**: iff `st_flags` has `SF_DATALESS` (`0x40000000`) [D], checked before every read ([80 §2.11.1]).
- **Linux**: no entry is cloud-only.

A cloud-only entry is never a candidate. For a cloud-only p, the resolver decides from size, mtime and id only; every
answer that needs content is `Unavailable(cloud-only)` ([40 §4.3] step 1).

### 4.6 Trash locations ([80 §2.11.4] rule 5)

A path inside a trash location is never a candidate; when E3 or E3d locates F there, F is `missing` with the place
"in the Recycle Bin" or "in the trash" ([F18]):

| OS | Trash locations |
|---|---|
| Windows | the directory `$Recycle.Bin` at the root of any volume (component compared with `eqi`) |
| Linux | `$XDG_DATA_HOME/Trash` (default `$HOME/.local/share/Trash`); on every mount point `$topdir`: `$topdir/.Trash/$uid` and `$topdir/.Trash-$uid` [D, FreeDesktop Trash specification] |
| macOS | `$HOME/.Trash`; `/.Trashes/$uid` and `<volume root>/.Trashes/$uid` [I] |

### 4.7 Never-candidate names ([40 §4.3]; [80 §2.11.4] rule 5)

#### 4.7.1 Pattern syntax and matching

A pattern is matched against the **last component** of a path (its basename), whole-name (anchored at both ends):
`*` matches any byte sequence, the empty one included; `?` matches exactly one byte; every other byte matches a byte
that is `eqi`-equal to it. No other metacharacter exists: `$`, `#`, `.` and `~` are literal. The same list applies on
every OS (open point 12).

#### 4.7.2 The list (resolver version 1)

| # | Pattern | Source | What it catches |
|---|---|---|---|
| 1 | `*.tmp` | [40 §4.3] | temporaries |
| 2 | `*.tmp.*` | [40 §4.3] | temporaries with a suffix |
| 3 | `*___jb_tmp___` | [40 §4.3] | JetBrains safe write |
| 4 | `*___jb_old___` | [40 §4.3] | JetBrains safe write |
| 5 | `*~` | [40 §4.3] | editor backups |
| 6 | `*.bak` | [40 §4.3] | backups |
| 7 | `*.orig` | [40 §4.3] | merge and patch leftovers |
| 8 | `*.old` | [40 §4.3] | backups |
| 9 | `*.rej` | [40 §4.3] | rejected patch hunks |
| 10 | `*.swp` | [40 §4.3] | vim swap files |
| 11 | `*.swo` | [40 §4.3] | vim swap files |
| 12 | `4913` | [40 §4.3] | vim's write probe |
| 13 | `.#*` | [40 §4.3] | Emacs lock files |
| 14 | `~$*` | [40 §4.3] | Office owner files |
| 15 | `sed??????` | [40 §4.3] | MSYS `sed -i` temporaries |
| 16 | `._*` | [80 §2.11.4] rule 5 | AppleDouble |
| 17 | `.DS_Store` | [80 §2.11.4] rule 5 | Finder metadata |
| 18 | `.fuse_hidden*` | [80 §2.11.4] rule 5 | FUSE unlinked-open files |
| 19 | `.nfs*` | [80 §2.11.4] rule 5 | NFS silly renames |
| 20 | `.goutputstream-*` | [80 §2.11.4] rule 5 | GIO atomic saves |
| 21 | `.~lock.*#` | [80 §2.11.4] rule 5 | LibreOffice lock files |

#### 4.7.3 Contextual rules

- **Old name plus a suffix** ([40 §4.3], [41 M10]): q is never a target when `basename(q) = x ‖ s` with s non-empty and
  `eqi(x, basename(p))`, for F's current path p.
- **Cloud conflict copies** ([40 §4.6]): when q lies under a cloud sync root (the tree's cloud-root flag, [F11]), q is
  never a target when some live file node's path in the same directory as q has basename `stem ‖ ext` and
  `basename(q) = stem ‖ "-" ‖ X ‖ ext`, where ext is empty or a `.` followed by the bytes after that basename's last
  `.`, stem is the rest, and X is non-empty and contains neither `.` nor `/`.
- **Located paths.** When E3 or E3d locates F at a path whose basename matches a pattern or a contextual rule, F is
  `missing` with the place ([F18]), never re-bound ([40 §4.3] step 5).

### 4.8 Denials and unavailable sources ([80 §2.11.4] rule 9)

| Situation | Outcome |
|---|---|
| `EPERM` or `EACCES` from `fsgetpath`, from enumerating a TCC-protected folder, from `setiopolicy_np`, or from a lock probe (macOS, Linux); `ERROR_ACCESS_DENIED` (5) from `OpenFileById` or an enumeration (Windows) | the source contributes nothing (`Unknown`); never "gone", never `missing` |
| a denial on the stat of p itself | p's presence is unknown: the link is `unverified (unreadable)`; no evidence source runs |
| a denial or sharing violation on a content read | `Unavailable(unreadable)` (§2.4) |
| an `OsFileId` row whose kind this OS cannot interpret, or whose `vol_key` names no mounted volume | the row is absent ([80 §2.11.2]); the id sources contribute nothing |
| a Linux frontier search that exhausts its budget before it finds or excludes an id | `Unavailable(budget)` ([80 §2.11.3] step 5) |

### 4.9 Paths this OS cannot represent (pass 1, P1-15)

Before any OS call on a path — a node's path p, an alias, a candidate q, a path an evidence source or a `path_moves` entry
yields — the resolver tests every segment of it with `representable_here` ([OS/path §8.1]), so no such path ever
reaches `ProjectFs` or the OS (a `\\?\` path would otherwise bypass Win32's name checks, [OS/path §6]), and the outcome
is a state, never an error:

- a node whose own path p has a segment that fails is `missing (not representable on this OS)` (detail 44, [F18 §4.6]),
  decided with no OS call and no evidence source; on Windows this covers a segment containing `:` (an NTFS alternate
  data stream, `x::$DATA`), ending in `.` or a space, or naming a device;
- a candidate or a derived path with a failing segment is never a candidate, and an evidence line that would need it
  contributes nothing;
- `--allow-nonportable` relaxes only [OS/path §8.2]'s portability warnings for other OSes, never this test.

This is the first step of the cascade, before §3.5's twin grouping and §5.4's checks.

## 5. The file cascade

### 5.1 Time comparisons and clock domains (disposition of review S-17)

- **File time with file time.** `teq(a, b)` ⟺ `⌊a / G⌋ = ⌊b / G⌋` and `tge(a, b)` ⟺ `⌊a / G⌋ ≥ ⌊b / G⌋`, with G the
  larger of the two values' granularities (floor division toward −∞).
- **Across domains** ([40 §4.3] "Clock domains"). Every comparison between a file timestamp, an `hlc` and a git
  committer time is made in nanoseconds since the epoch — an `hlc` as `hlc_ns(h)`, a committer time t (seconds) as
  t × 10^9 — and takes the conservative side by one margin `SKEW` = HOLE(F20-clock-skew), in nanoseconds. For a file
  timestamp x and an `hlc` h:
  - x is **clearly after** h ⟺ `x > hlc_ns(h) + SKEW`;
  - x is **possibly before** h ⟺ `x < hlc_ns(h) + SKEW` (copy-rule line 3: "q existed while F was verified");
  - x is **possibly after** h ⟺ `x > hlc_ns(h) − SKEW` (E7: "later than the last settle").
  The uses are fixed by [40 §4.3]: copy-rule line 3 (§5.9), E7 and E8 (§5.12, §5.13), the G4 window (§5.11.2) and
  `planned` binding (§5.18); this chapter adds the ChangeTime condition of copy-rule line 2 (§5.9). Each takes the side
  that never adds an automatic re-bind.

### 5.2 Stat tuples ([80 §2.11.4] rule 3)

The **stat quadruple** of [40 §2.6] is (size, mtime, file id, creation time). What a path reads depends on the path and
the OS:

| Path | Windows | Linux | macOS |
|---|---|---|---|
| read (verbs, hooks) | size, mtime, creation time, attributes (`GetFileAttributesExW`; no id) | size, mtime, creation time where `statx` returns it, and the id (`statx` plus one `name_to_handle_at` for `hgen`) | size, mtime, creation time, id (`lstat`/`getattrlist`) |
| settle | all four (`FileIdExtdDirectoryInfo`) | all four where available (`getdents64`, `statx`, `name_to_handle_at`) | all four (`getattrlistbulk`) |

A tuple **equals** a `FILEOBS` row iff every component both hold is equal: size exactly, timestamps by `teq`, ids by
identity (§5.3). A component one side lacks is not compared. On Windows a read therefore cannot see a swap of two files
with equal size, mtime and creation time; the next settle sees it ([40 §2.6]).

### 5.3 Identity ([80 §2.11.4] rule 8)

- Two file identities are equal iff their whole `OsFileId` (`kind`, `vol_key`, `id`) are equal ([80 §2.11.2]); the
  `parent`, `aux` and `docid` fields take no part. Kind 0 (`none`) is equal to nothing.
- On Linux the `id` is `ino ‖ hgen`: an inode number alone is never identity, and a `d_ino` hit of the frontier counts
  only after `name_to_handle_at` on it yields the stored `hgen` ([80 §2.11.3] step 4). `rm` and a re-create at one path,
  or the lowest-free reuse of a directory's and a file's inode numbers, never yields an exact match.
- **Hard links.** When a file located by id has more than one link, E3 and E3d yield at most `strong` for it (open point
  13).

### 5.4 Checks at a present path

p is stat-ed only after §4.9's test has passed. When p stats in T, these checks run in this order; the first that decides
the state ends the file cascade ([40 §4.3] step 1):

1. **Twins** (§3.5), when F's twin-candidate group has at least 2 members.
2. **Cloud-only p** (§4.5): decide from size, mtime and id only.
3. **Unchanged.** The read or settle tuple equals `FILEOBS(F, T)` → `ok` (the row's state is shown as recorded, so a
   recorded `ambiguous (path reused; original at q)` renders as such), with the spelling detail of §3.6.
4. At a **settle**, when `FILEOBS(F, T)` exists and the tuple differs:
   1. the `replaced` test (§5.4.1);
   2. the path-reuse check (§5.4.2);
   3. the rename-over and swap check (§5.4.3);
   4. in a writer tree, `git/case` (§3.6).
5. Otherwise `ok`, with the detail "changed since …" when `oid(p) ∉ {o, last_oid}` (hashed only when anchors need the
   bytes, or at a settle), and the spelling detail of §3.6.

#### 5.4.1 The `replaced` test ([40 §4.4])

At a settle, p is `replaced` (nothing is written) iff all hold:
- `oid(p) ∉ {o, last_oid}`;
- p's file id is known and differs from `FILEOBS.file_id` (§5.3);
- F's `artifact_kind` is not `generated`;
- the reference fingerprint R — of `last_oid` if present, else of `o` — exists, and R and p's content are both text with
  `nlines ≥ 5` (`REPLACED_MIN_LINES` = 5);
- `eoin(R, p) < 29/100` and `enio(R, p) < 29/100` (`REPLACED_MAX` = 0.29, the p90 background containment of
  [10 §5.8]).

#### 5.4.2 The path-reuse check ([40 §4.4]; [72 M13])

At a settle, when p's file id differs from `FILEOBS.file_id`: one lookup of the recorded id — `OpenFileById` on Windows,
`fsgetpath` on macOS, the `DIRMAP` frontier on Linux (a hit counts only with the stored `hgen`; an incomplete search is
`Unavailable(budget)`) — locates the original. If it is alive at a path q ≠ p inside R with (size equal and mtime `teq`
`FILEOBS`) or `oid(q) ∈ {o, last_oid}`, F is `ambiguous (path reused; original at q)`, recorded in `FILEOBS` as state 4
with detail 29 and q ([F11 §12.5]). A denial contributes nothing (§4.8).

#### 5.4.3 Rename-over and swap ([40 §4.3] step 1, §4.6)

At a settle: when `oid(p)` ∈ {`o_G`, `last_oid_G`} of another live node G whose own path is absent in T, F and G are
`ambiguous (rename-over)`. When, in addition, G's path is present and `oid(path(G))` ∈ {`o_F`, `last_oid_F`}, both are
`ambiguous (swap)`.

### 5.5 Source order and selection

**Which sources run** ([40 §4.3] step 2, §5.2):

| Gate row | Sources, in this order | Runs on |
|---|---|---|
| G1 (p ∈ τ(H)) | E1, E3d, E3, E4, E5, E7, E8, then similarity | E1–E5 on reads and settles; E7, E8 at settles; similarity with `--deep` |
| G2 (g ⪯ H) | E1, E3d, E3, E4, E5, E6 over the G2 window, E7, E8, then similarity | as G1; E6 within the read-path caps on reads |
| G3 (an alias in τ(H)) | none: `pending` | — |
| G4 (otherwise) | E6 over the G4 window only | within the read-path caps on reads |
| no git | E1, E3d, E3, E4, E5, E7, E8, then similarity | as G1 |

E2 is not part of resolver version 1 ([40 §4.7], [AR §11] #41).

**Selection.**
1. The first source that yields exactly one `exact` candidate q decides: `moved-auto` to q. Later sources do not run.
2. Otherwise, if any source yielded two or more `exact` candidates: `ambiguous`, listing those of the first such source.
3. Otherwise, the `strong` and `copy` candidates of all sources, in source order and then path order, without
   duplicates, are the proposals: one distinct target → `moved-needs-confirm` with its detail; two or more →
   `ambiguous`. Under `files.policy.auto = strong`, exactly one distinct `strong` target (never `copy`) is applied as a
   marked guess ([40 §9.2] #1).
4. Otherwise the similarity search (§5.14), where it runs, then `weak` proposals.
5. Otherwise `missing`, or `unverified` under §1.5.

**Tiny files** ([40 §4.4]). For a tiny F (§2.6.5), only E1, E3d, E3 and E6 may yield `exact`; E4, E5, E7 and E8 yield no
`exact` or `strong` candidate, and the similarity search does not run. Their equal-`oid` candidates make F `ambiguous`
(listed); with none, the result is `missing`.

### 5.6 E1: intent and hook evidence ([40 §4.3])

- An open or recovered `FSINTENT` for F naming q: `exact`.
- A `PENDING` row for F from this tree: `exact` when its recorded evidence class is exact (explicit intent, a file-id
  chain, a journal chain); `strong` when it is a hook argument parse (`hook/argv`).
- A `PENDING` row for F from another tree counts only when its captured `oid` equals `oid(q)` in T; then as above.

### 5.7 E3d: the parent directory's id ([40 §4.3]; [72 M13])

E3d runs when `dirname(p)` is non-empty and absent in T, once per distinct parent directory per command.
1. Look up `FILEOBS.parent_dir_id` (per OS as §5.4.2) → the directory's current path D′, which must be inside R (§4.3).
2. q = `D′ / basename(p)`. If q is absent, E3d yields nothing.
3. q is `exact` iff its id equals `FILEOBS.file_id`, or (its size equals and its mtime `teq` `FILEOBS`), or
   `oid(q) ∈ {o, last_oid}`.
4. Otherwise the `replaced` test (§5.4.1) runs at q: if it holds, F is `replaced` (at q); if not, F is
   `moved-needs-confirm (directory moved, file replaced)`, a proposal that is never applied automatically, under either
   policy (open point 14).

### 5.8 E3: the file's own id ([40 §4.3])

Look up `FILEOBS.file_id` (per OS as §5.4.2) → q, which must pass §4. q is `exact` iff (its size equals and its mtime
`teq` `FILEOBS`) or `oid(q) ∈ {o, last_oid}`; otherwise `strong` ("moved and edited in place"). A file with more than one
link yields at most `strong` (§5.3).

### 5.9 E4: near candidates and the copy rule ([80 §2.11.4] rule 1)

- **Scope.** The directories `dirname(p)` and its parent (never above the root), `dirname(a)` for every alias a of F,
  and `dirname(Y ‖ rest)` for every `path_moves` entry X/ → Y/ (any class) whose `from` X/ is a prefix of p with
  `rest = p[len(X/)..]`. Directories that do not exist are skipped. E4 enumerates the regular files directly inside
  these directories, not recursively. "The files E4 enumerated" below means all of them, before the tests of §4.
- **Candidates.** Enumerated files that pass §4 with `oid(q) ∈ {o, last_oid}`. An implementation may skip a file whose
  size proves inequality — for a recorded content X with a fingerprint, a candidate needs `nbytes_X ≤ size(q) ≤
  nbytes_X + nlines_X` — and must hash every other file of the scope.
- **The copy rule** for an equal-`oid` candidate q from E4 or E7 ([40 §4.3]); the first matching line decides:
  1. E6 shows p → q inside one commit of the window, or E1 names q → `exact`.
  2. q came from E4, and all of the following hold → `exact`:
     - `VolumeCaps.btime` of q's volume is `TunneledNotCopied` (on Windows NTFS: HOLE(F20-btime-ntfs), draft
       `TunneledNotCopied`; on Linux `Unforgeable` or `Absent`, and on macOS `CopiedByClones`, so this line never
       applies there);
     - q shows no clone indicator (macOS: `EF_MAY_SHARE_BLOCKS` set or `ATTR_CMNEXT_CLONE_REFCNT` > 0; Windows and
       Linux define none);
     - q's creation time `teq` `FILEOBS.creation`;
     - no other file E4 enumerated has a creation time `teq` q's, and no other file node whose path lies in E4's
       directories has a recorded creation time (`FILEOBS` in T) `teq` q's;
     - q's ChangeTime (Unix: ctime) is **clearly after** V, where V is the effective `verified_at` of F in T — required
       iff HOLE(F20-ctime-rename) confirms that a same-volume rename sets it (draft: required); if the hole is filled
       the other way, this line is never `exact` on Windows (disposition of review S-16).
  3. q's creation time is not `teq` `FILEOBS.creation`, and it is **possibly before** V (`q.creation < hlc_ns(V) +
     SKEW`, [40 §4.3]) → q is a copy that coexisted with F: never a candidate.
  4. Otherwise, including "no `FILEOBS` row for F in T" and every E7 candidate that reaches this line → `copy`:
     `moved-needs-confirm (identical copy)`.
- **Effective `verified_at`** V = the larger of `FILEOBS.verified_at` and the `hlc` of the newest `TREES` epoch that
  covers F: a `full-tree` epoch, or a `lane-owned` epoch whose scope digest equals the digest of the lane's current
  `files_owned` globs that F matches; a `partial` epoch never counts ([40 §2.6], [F11]; review A1P-04). A smaller V only
  turns line-3 outcomes into `copy` proposals, never into `exact` ones, except through the S-16 condition of line 2,
  whose residue is recorded in open point 15.

### 5.10 E5: recorded directory moves ([40 §4.3])

- **Composition.** Start with x = p. For every `path_moves` entry e of the root node of class `explicit`, `confirmed`
  or `committed`, in (`hlc`, `from`, `to`) order: if `e.from` is a prefix of x, x := `e.to ‖ x[len(e.from)..]`. If
  x ≠ p, q = x. (The order is the entries' stored order, [40 §2.4]; no entry is compared with a commit's `hlc`.)
- q present with `oid(q) ∈ {o, last_oid}` → `exact` (the entry is recorded intent); q present with another `oid` →
  `strong`.
- Entries of class `observed` compose the same way but yield `strong` only.
- **Sibling inference.** When, in the same settle, at least 2 other nodes under X/ have `exact` candidates from E1, E3d,
  E3 or E6 that map `X/rest_i` to `Y/rest_i` for one pair (X/, Y/), then `Y ‖ p[len(X/)..]`, if present, is `strong` for
  F (`PREFIX_MIN_NODES` = 2). The inference is computed from those sources only, once per settle, so it does not depend
  on the order in which nodes are processed.

### 5.11 E6: per-commit renames and the window bounds

E6 applies to root `project` of a tree whose root directory is the top-level of a git worktree; for other roots it
contributes nothing (open point 16). Commits, trees and blobs come from the in-process git object reader ([60] M4).

#### 5.11.1 Per-commit changes and exact pairs

- **Changes.** For a commit c with first parent c1 (the empty tree for a root commit), `deleted(c)`, `added(c)` and
  `modified(c)` are the entries of class `file` (modes `100644`, `100755`) or `link` (`120000`) that are in τ(c1) only,
  in τ(c) only, or in both with different blob ids. Gitlinks (`160000`) take no part. A merge commit is diffed against
  its first parent only.
- **Exact pairs** ([40 §4.3], [41 m6]). For x ∈ `deleted(c)` with blob X and class k: let D be the deleted and A the
  added entries of c with blob X and class k.
  - |D| = 1 and |A| = 1: the exact pair x → a.
  - |A| ≥ 1 and (|D| ≥ 2 or |A| ≥ 2): an ambiguous group with candidates A; never paired by order.
  - |A| = 0: no exact pair (§5.11.4).

#### 5.11.2 Windows

A window is a first-parent walk from H: c0 = H, c(i+1) = the first parent of ci. The walk stops before the first
commit that meets the stop rule, after a root commit, or after `E6_MAX_COMMITS` = 2,000 commits, whichever comes first
([40 §4.3]; "a resolver constant, not a budget", [AR §13]). The window is processed oldest first.

| Window | Used by | Stop rule |
|---|---|---|
| g..H | G2 | the commit is g or an ancestor of g |
| merge-base(g, H)..H | G4, g in the local object store | the commit is g or an ancestor of g (on H's first-parent walk this is exactly "an ancestor of a merge base") |
| time-bounded | G4, g not in the local object store | the commit's committer time t satisfies `t × 10^9 < hlc_ns(h_obs) − E6_SLACK_MS × 10^6 − SKEW`, with `E6_SLACK_MS` = 86,400,000 (1 day), h_obs the `hlc` of the commit that last set F's observation composite, and the margin of §5.1 ("1 day + margin", [40 §4.3]) |

On a read, the caps `files.read.max-uncached-ancestry` and `files.read.max-e6-commits` ([CFG]) apply; exceeding either
is `Unavailable(git)`. Settles and `check` run the whole window ([40 §4.3]).

#### 5.11.3 Chains

A chain starts at a path x0 (p, or under G4 also each alias of F) with class `exact`. For each commit c of the window,
oldest first, when the current path x ∈ `deleted(c)`:
- an exact pair x → y: x := y;
- an ambiguous group: the chain ends `ambiguous` with the group's candidates;
- otherwise the inexact pairs of §5.11.4: a split or a merge ends the chain with that proposal; a `strong` or `weak`
  pair y continues the chain with x := y and the class lowered to that pair's;
- otherwise the chain ends: x was deleted in c (`missing (deleted in git c…)`, [F18]).

A chain "starts at p" ([40 §4.3] G4) when it has at least one step from p. The chain's result is the final x with its
class; x must be present in T and pass §4, or E6 yields nothing. Under G4, a result reached only by chains that start at
an alias is `moved-needs-confirm (moved differently on this line)`, never `exact` ([40 §4.3] G4, §5.2).

#### 5.11.4 Inexact pairs

For x ∈ `deleted(c)` with no exact pair and no ambiguous group, with old blob X (from τ(c1)): the **added candidates**
are the entries of `added(c)` of the same class that are in no exact pair or ambiguous group of c; the **host
candidates** are the entries of `modified(c)`. Both blobs of every pair are read in process.

- **The git pair score** `gs(X, a)` is an integer percentage 0–100, defined here normatively (pass 1, A1-34; the value
  enters the hashed `relink` text `git-pair/<score>`, [F18 §5.1]). For a blob b of n bytes, its **span counts** are:

  ```
  text = no byte 00 in b[0 .. min(n, 8000))
  S = {}                          # map from a hash value to a byte count
  a1 = a2 = 0; k = 0              # a1, a2 are u32: every step below is modulo 2^32
  for i in 0 .. n-1:
      c = b[i]
      if text and c == 0x0D and i + 1 < n and b[i+1] == 0x0A: continue    # a CR before LF is skipped
      t = a1
      a1 = (a1 << 7) ^ (a2 >> 25)
      a2 = (a2 << 7) ^ (t >> 25)
      a1 = a1 + c
      k = k + 1
      if k < 64 and c != 0x0A: continue
      h = ((a1 + a2 × 97) mod 2^32) mod 107927
      S[h] = S[h] + k;  k = 0;  a1 = a2 = 0
  if k > 0: h = ((a1 + a2 × 97) mod 2^32) mod 107927;  S[h] = S[h] + k
  ```

  Then, with S_X and S_a the span counts of the two blobs and m = max(len(X), len(a)) their larger raw length (CR bytes
  included): `gs(X, a) = ⌊100 × Σ_h min(S_X[h], S_a[h]) / m⌋`, and `gs` = 0 when m = 0 or when X or a is of class
  `link` (only regular files are scored). `gs` is symmetric. This is git's similarity index of the pair —
  git 2.54.0's `estimate_similarity` over `diffcore-delta.c`'s span hashes, as `git diff -M` prints it
  (`⌊⌊60,000 × shared / m⌋ × 100 / 60,000⌋`, which equals the formula) — for every pair git scores, with two deliberate
  differences: no `.gitattributes` binary or text setting is read (the text test is the NUL test above), and the
  product is exact, where git on an LLP64 platform overflows it for more than 71,582 shared bytes. git's size pre-filter
  only skips pairs that cannot reach its minimum score, so it is not part of the definition ([40 §4.4] "E6 per-commit
  pair ≥ 90 %", "git pair 20–49 %"; [40 §2.2] `relink` "`git-pair` git's per-commit similarity divided by 100").
- **Golden vectors** (checked against `git diff --no-index -M1% --name-status` of git 2.54.0 with `core.autocrlf=false`;
  WP-74 keeps them as fixtures and adds its differential against `git diff-tree -M20%` on synthetic histories):

  | X | a | `len` X, a | `gs` |
  |---|---|---|---|
  | `alpha␊beta␊gamma␊delta␊` | `alpha␊beta␊gamma␊epsilon␊` | 23, 25 | 68 |
  | `alpha␊beta␊gamma␊delta␊` | the same lines ending in CR LF | 23, 27 | 85 |
  | 130 × `x`, then LF | 128 × `x`, `yy`, then LF | 131, 131 | 97 |
  | `one␊` … `ten␊` (the English numbers one to ten, one per line) | `one␊` … `five␊` | 49, 24 | 48 |
  | 10 × `line␊` | 5 × `line␊` | 50, 25 | 50 |

  (␊ is the byte `0A`.)
- **Containments** `oin` and `nio` of a pair are §2.10.1's exact measures over the two blobs (their estimates when the
  exact limit fails).

The tests run in this order ([40 §4.4]):

| # | Test | Result |
|---|---|---|
| 1 | tiny X (§2.6.5) | no inexact pair; the chain ends (deleted) |
| 2 | **split**: c has at least 2 added candidates, at least `SPLIT_MIN_PIECES` = 2 of them have `nio(X, a) ≥ 4/5`, and their `oin(X, a)` sum to at least 3/5 | `moved-needs-confirm (split)`, the pieces listed |
| 3 | exactly one added candidate has `gs ≥ 90` (`E6_STRONG`) | `strong` to it |
| 4 | the best added candidate by (`gs` descending, path) has `gs ≥ 50`, `gs` at least 20 above the next, and the same `dirname` or the same basename as x (the "similarity ≥ 0.5 with margin ≥ 0.2 and one directory or basename corroboration" row of [40 §4.4], with `gs / 100` as the score) | `strong` to it |
| 5 | a host or added candidate m with `oin(X, m) ≥ 4/5` and `nio(X, m) < 1/2`; several: the first in path order | `moved-needs-confirm (merged)` into m |
| 6 | the best added candidate has `20 ≤ gs < 50` and `max(oin, nio) ≥ 4/5` (`E6_WEAK`, "git pair 20–49 % confirmed by containment") | `weak` to it |
| 7 | none | no pair |

### 5.12 E7: changes since the last settle

- **When.** At a settle with budget left, or with `--deep`; never on reads, hooks or a tree's first settle (`TREES`
  first-settle flag clear) ([40 §4.3]).
- **Candidate set.** Let h be the tree's last settle `hlc` ([F11] `TREES`). E7's candidates are the regular files q of R
  that pass §4 and satisfy one of:
  - a relevant time of q is **possibly after** h (§5.1) — Windows: ChangeTime or CreationTime; Linux: ctime; macOS:
    `ADDEDTIME` or ctime;
  - q lies under a directory whose relevant time is possibly after h (Windows), or under a changed directory of the
    frontier (Linux, macOS; §5.12.1).
  Equal-`oid` candidates go through the copy rule (§5.9); E7 candidates never pass its line 2.
- **Windows.** The candidate set is defined by this predicate over the whole root. [80 §2.11.3] leaves to an M6
  measurement whether Windows prunes the walk with `DIRMAP`; a pruning that yields a different set would be a new
  resolver version.

#### 5.12.1 The frontier's racy threshold (Linux and macOS; [80 §2.11.3])

- **Threshold.** At the start of a settle the process writes the single byte `00` at offset 0 of
  `<store>/tmp/settle.stamp` (creating it with length 1 if absent), then reads that file's mtime T0 with its
  granularity. When the tree's volume differs from the store's (different `vol_key`), T0 is the largest mtime of the
  tree's `DIRMAP` rows; with no rows, every directory counts as changed.
- **Changed directory.** A directory d is changed iff it has no `DIRMAP` row, or its mtime is not `teq` the row's, or
  `tge(mtime(d), T0)`.
- **Racy rows are not recorded.** A settle never records a `DIRMAP` mtime m with `tge(m, T0)`: it keeps the previous row
  (or none), so the next settle treats the directory as changed again. This replaces a stored "racy" mark, which
  [80 §2.11.2]'s `DIRMAP` layout does not have (open point 17).
- **Documented hole.** Tools that restore directory mtimes can hide a change; the result is `missing` or `unverified`,
  never a wrong binding, until a full enumeration ([80 §2.11.3]).

### 5.13 E8: edited and moved ([40 §4.3])

At a settle that is not the tree's first: a candidate q that passes §4, is text, has `basename(q) = basename(p)`
bytewise, satisfies E7's time predicate (§5.12), and, with R the fingerprint of `last_oid` (else of `o`), has
`eoin(R, q) ≥ 4/5` and `enio(R, q) ≥ 4/5` (`E8_MIN` = 0.8), is `strong`: `moved-needs-confirm (edited+moved)`. Without R,
E8 yields nothing. Not for tiny F.

### 5.14 Similarity search ([40 §4.3] step 4)

Only under `links check --deep` and `links sync --deep`; never on reads or hooks; not for tiny or binary F.
- **Candidates**: unbound text files of R that pass §4. They are read and fingerprinted in this order, within the budget:
  the key (same basename as p ? 0 : 1, same extension as p under `eqi` ? 0 : 1, −(number of leading directory
  components shared with p), new since the last settle by E7's predicate ? 0 : 1, path order).
- **Stage 1**: rank by `r(FP_F, FP_q)` (§2.10.3) against F's reference fingerprint (of `last_oid`, else of `o`),
  descending, then path; keep the first `STAGE1_TOP` = 10.
- **Stage 2**: re-read those candidates and compute the pair score (§2.10.4) — exact on the old side when git's old blob
  is readable in process (by `observed_blob`), else the sketch estimate.
- **Budget.** If the budget ends before stage 1 has covered every candidate, the link is `unverified (budget)`; the
  proposals found so far are kept in `FILEOBS` and never applied automatically, under either policy (open point 18).

### 5.15 Classification thresholds ([40 §4.4])

| Class | Condition | State |
|---|---|---|
| strong (similarity) | best pair score ≥ `SIM_STRONG` = 1/2, margin over the runner-up ≥ `SIM_MARGIN` = 1/5, and `dirname(q) = dirname(p)` or `basename(q) = basename(p)` | `moved-needs-confirm` (similar …) |
| merged | `oin ≥ 4/5` (`MERGED_OIN`) and `nio < 1/2` (`MERGED_NIO_MAX`) against a host file | `moved-needs-confirm (merged)` |
| split | the test of §5.11.4 row 2, over stage 2's candidates | `moved-needs-confirm (split)` |
| weak | best score in [`SIM_WEAK` = 3/10, 1/2), or margin < 1/5 | `moved-needs-confirm` when one candidate scores ≥ 3/10, `ambiguous` when two or more do |
| none | no score ≥ 3/10 | `missing` |

Thresholds are exact rationals; "≥ 1/2" includes 1/2 and "< 1/2" excludes it. The rationale is [10 §7]: 1/2 lies above the
p90 background of unrelated files (0.24 symmetric, 0.29 containment) and below the p10 score at 30 % churn (0.62).

### 5.16 Automatic `path_moves` entries ([40 §4.4])

- **`committed`**: in a writer tree, one commit c of the window has every tracked entry under `from/` in τ(c1) in an
  exact pair to `to/ ‖ rest`, and no entry of τ(H) starts with `from/`.
- **`observed`**: `PREFIXEV` shows every linked present node under `from/` re-bound exactly to `to/`, `from/` is absent
  in T, and at least `PREFIX_MIN_NODES` = 2 nodes moved.

### 5.17 Quiescence ([40 §4.1] P10, §4.2)

- `QUIESCE_NS` = 50,000,000 (50 ms) on the process's monotonic clock.
- A settle that would write at least one re-bind re-checks, no earlier than 50 ms after the last stat that found each
  re-bind's source path p absent: it re-stats every p and drops every re-bind whose p is present again, and it re-stats
  every target q and drops every re-bind whose q is absent or whose read tuple differs from the tuple its evidence used.
  One wait serves all re-binds of the settle. The target re-stat includes the racy-entry re-check of [40 §4.2].
- On hook and server paths a settle never sleeps: a re-bind whose 50 ms have not passed when the path's slice ends is
  dropped and left to the next settle (review A1P-06; the rule of the paths is [F16]'s).

### 5.18 `planned` binding times ([40 §3.2])

A writer-tree settle binds a `planned` node at a path that now exists only if the tree's HEAD descends from the planning
commit, or the file's creation time is **clearly after** the planning commit's `hlc` (§5.1; "exceeds the planning
commit's time by more than the margin", [40 §4.3]). Without a creation time, only the descent test applies.

### 5.19 Cross-volume and busy states on Unix ([80 §2.11.4] rules 6 and 7)

- `EXDEV` (another mount, a btrfs subvolume, an overlay lower directory) is the Unix form of "cross-volume", like
  Windows `ERROR_NOT_SAME_DEVICE` and two parents with different `vol_key`s: `file mv` refuses any cross-volume move, of
  a file as of a directory (exit 7), and a rename that fails that way aborts the intent; moirai never copies a project
  file and deletes the original ([40 §3.4], review A1P-01).
- Unix has no sharing violations: `EBUSY`, `EACCES` and `EPERM` replace Windows errors 5 and 32 in `file mv`/`file rm`
  diagnoses, and a process whose working directory is inside a directory being renamed does not block the rename
  ([F19] gives the texts; [40 §8.3.1] row 21 gains these variants).

### 5.20 Per-OS rules ([80 §2.11.4] rules 1–9)

| Rule | Windows (M6) | Linux (port) | macOS (port) | Section |
|---|---|---|---|---|
| 1 copy rule | line 2 on `TunneledNotCopied` volumes, with unique creation times and the ChangeTime condition | line 2 never | line 2 never; clone indicator defined | §5.9 |
| 2 twins, spelling | per-directory case flag; normalization-sensitive, so the normalization rule applies | casefold directories fold case and normalization; elsewhere exact bytes | per-volume case; normalization-insensitive | §3.5, §3.6 |
| 3 reads with ids | no id on reads | id with `hgen` | id | §5.2 |
| 4 sorted candidates | path order | same | same | §4.1 |
| 5 never-candidates, trash | the list of §4.7; `$Recycle.Bin` | same list; XDG and `.Trash` locations | same list; `.Trash`, `.Trashes` | §4.6, §4.7 |
| 6 cross-volume | different `vol_key` | `EXDEV` | `EXDEV` | §5.19 |
| 7 busy states | errors 5, 32; `RmGetList` | `EBUSY`, `EACCES`, `EPERM` | same as Linux | §5.19 |
| 8 identity | whole `OsFileId` (`FILE_ID_128`) | whole `OsFileId` (`ino ‖ hgen`) | whole `OsFileId` (file id) | §5.3 |
| 9 denials | `Unknown` | `Unknown` | `Unknown`, also for TCC, SIP and sandbox denials | §4.8 |
| racy threshold | E7 by `hlc` and `SKEW` | `settle.stamp` | `settle.stamp` | §5.12 |

## 6. Anchor constants

The anchor holes of this section are decided by replay row 2 of [40 §8.3.4] (WP-76); their draft values are [40]'s
(§1.4).

### 6.1 Capture ([40 §2.7])

Capture runs on the captured content b, which must be text for every kind except `file` (a span anchor on binary
content is refused: [F19 §10.2] `anchor_spec`, case `binary`, exit 2). Let t = `atext(b)` and N = N(t).

1. **Span.** `path:L-M` (or `path:L`, M = L) names lines [L, M]; 1 ≤ L ≤ M ≤ the number of lines, else the form is
   refused ([F19 §10.2] `anchor_spec`, case `range`). s is the first and e the last non-trivial line in [L, M] ("spans skip lines that are blank or contain only
   braces"). If there is none, the kind is `lines`, with hint [L, M] and the window around [L, M].
2. **Kind.** `quote` iff [s, e] has at most `QUOTE_LINES` = HOLE(F20-quote-lines) (draft 4) non-trivial lines and
   `len(ST(s, e)) ≤ QUOTE_MAX` = HOLE(F20-quote-max) (draft 128); otherwise `range`.
3. **Quotes.**
   - `quote`: `exact = ST(s, e)`.
   - `range`: start `exact = cutp(ST(s, e), QUOTE_DEFAULT)` and `end = cuts(ST(s, e), QUOTE_DEFAULT)`, with
     `QUOTE_DEFAULT` = HOLE(F20-quote-default) (draft 64) ("`exact` ≤ 64 B by default, up to 128 B when needed",
     [40 §2.7]; open point 19).
   - `symbol`, `heading`: `exact = cutp(header(l, kind), QUOTE_MAX)` for the item's header line l.
   - Quote text from `--quote-file` or stdin: one leading `EF BB BF` is removed; input containing `EF BF BD` (U+FFFD) is
     refused ([F19 §10.2] `anchor_spec`, case `fffd`, exit 2); CRLF pairs become LF; the text is split by `lines`, each
     line is `nl`-trimmed, leading and trailing lines that are then empty are dropped, and the rest is joined by `0A`.
     The result must be non-empty and must occur in N (else `anchor_spec`, case `empty` or `not-found`); the lines it
     covers are [s, e], and the kind is chosen by step 2 with this text as the span text.
4. **Quote span.** The lines [qs, qe] the quote covers: [s, e] for `quote` and `range`; the header line for `symbol` and
   `heading`.
5. **Context.** `prefix = cuts(N[0 .. o), CTX)` and `suffix = cutp(N[o′ ..), CTX)`, where o is the offset of the quote
   (for `range`, of the start quote) and o′ the offset after the quote (for `range`, after the end quote), with `CTX` =
   `CONTEXT` = HOLE(F20-context) (draft 32). Context may cross lines and contains `0A` bytes.
6. **Window** W around the quote span (§2.7), with `WIN` = HOLE(F20-window-lines) (draft 16).
7. **Hint**: `quote`, `range`: [s, e]; `symbol`, `heading`: the item's line span as the scanner reports it; `lines`:
   [L, M]. 1-based and inclusive.
8. **Uniqueness ladder** ([40 §2.7] step 4: "the resolver runs on the captured file itself"). The anchor is unique when
   the exact step of §6.2, run on the captured content with the anchor's selectors (window included) and no occurrence,
   yields its captured position. If it does not:
   1. `prefix` and `suffix` are recomputed with `CONTEXT_MAX` = HOLE(F20-context-max) (draft 64);
   2. then the enclosing scope, if a scanner finds one and it was not already recorded — skipped while the interim rule
      below holds;
   3. then `occurrence` is recorded: the 1-based index of the captured hit among the exact hits of the search region, in
      offset order.
   Once the scanner appendix exists, the scope of a `path:L-M`, `symbol` or `heading` form is recorded whenever a scanner
   finds one ([40 §2.7] authoring table), so rung 2 applies only to quote-file forms; until then no form records one.
   `lines` anchors have no quote and skip the ladder.
9. **`span_hash`** (§2.8), `blob` = `oid(b)` (§2.3), `git`, `captured` and `pred` ([F08]; `captured` covers the widened
   prefix and suffix and, for `lines`, the window, [40 §2.7] as revised for review S-03) and `resolver` = 1.

**The interim scanner rule** (pass 1, A1-14, S1-4, P1-20; [F08 §10.3.1]). The scope scanners of [40 §2.7.1] decide the
`scope` bytes and, for the `symbol` and `heading` forms, the item's header line, hence its quote, its hint and its header
span hash; every one of these enters `captured` or the hashed selector block ([F08 §11.4], [F07 §8.2]). Until the scanner
grammar exists as a normative appendix of this chapter (open point 30), engine and model must not depend on a scanner
for a hashed byte, so:

- no capture records a scope: `has_scope` is clear, `captured` takes `lp("")` for it, and step 8 skips rung 2
  ([F08 §10.3.1]);
- the `path::A/B` (`symbol`) and `path#H` (`heading`) authoring forms are refused with exit 2, the text naming the
  `path:L-M` form of the same lines ([F19 §10.2] `anchor_spec`, case `no-scanner`); `path`, `path:L-M`, `path@<commit>:L-M` and quote-file forms are
  unaffected;
- an imported anchor that carries a scope, or is of kind `symbol` or `heading`, keeps its bytes ([F08 §10.3.1]); it
  resolves without scanner steps: the scope-only step of §6.5 does not run, and its header quote is matched as a quote.

If the appendix is not written by the freeze, the owner chooses between keeping this rule in format v1 and removing the
scanner-derived bytes from the hashed inputs (a change of [PLAN] FB-4); `reviews/owner-questions.md` records the question.

### 6.2 Resolve: hint and exact quote ([40 §4.5] steps 1–3)

The cascade runs on the current content b′, with t′ = `atext(b′)` and N′ = N(t′).

1. **Hint.** If the hint lines exist in t′: for `watch = span`, `XXH3-64(ST(h1, h2)) = span_hash` → `fresh`; for
   `watch = header`, `XXH3-64(header(l_h1, kind)) = span_hash` → `fresh` (a changed body keeps the anchor `fresh`,
   [40 §4.5]).
2. **Marker** (opt-in prose only): as [40 §4.5] step 2; no constant.
3. **Search region.** If the anchor has a scope and the scanner resolves it uniquely in t′, the scope's byte range in
   N′ first; if the quote has no hit there, the whole of N′.
4. **Hits.** Every offset h with `N′[h .. h + len(exact)) = exact` (overlapping hits included). For `range`: every start
   hit h is paired with the first end hit h_e ≥ h + len(exact) whose last line is at most `line(h) + RANGE_SPREAD ×
   (h2 − h1 + 1) − 1`, where `RANGE_SPREAD` = HOLE(F20-range-spread) (draft 2) ("within twice the captured span length",
   counted in lines; open point 20); unpaired start hits are dropped.
5. **One hit** → `moved` (or `fresh` when it lies at the hint).
6. **Several hits**, in order:
   1. **Context score.** For a hit with quote bytes [h, z) (for `range`, from the start quote's first byte to the end
      quote's last): `pa` = the length of the longest common suffix of `prefix`
      and `N′[max(0, h − len(prefix)) .. h)`, divided by `len(prefix)`; `sa` = the length of the longest common prefix
      of `suffix` and `N′[z .. z + len(suffix))`, divided by `len(suffix)`; an empty `prefix` or `suffix` gives 1.
      `ctx = (pa + sa) / 2`. A unique best with a margin ≥ `CONTEXT_MARGIN` = HOLE(F20-context-margin) (draft 1/10) over
      the next decides.
   2. **Window score** (§6.3). A unique best with a margin ≥ `WINDOW_MARGIN` = HOLE(F20-window-margin) (draft 15/100)
      decides. When the window step is Unavailable (§2.4), it decides nothing.
   3. **Occurrence**, if recorded: the hit whose 1-based index in offset order within the search region equals it.
   4. Otherwise `ambiguous`.
7. Nearest-to-hint is never a tie-break ([40 §4.5]).

### 6.3 Window alignment ([40 §2.7]; the LCS-tokenisation gap of [PLAN §3.3])

- **Tokens** are the u16 window hashes (§2.7.1); a window holds at most 2 × `WIN` tokens ("LCS over ≤ 32 tokens",
  [40 §2.7]).
- **Score.** For a stored window (B_s, A_s) and a candidate's window (B_c, A_c) computed around the candidate's quote
  span in t′: `wscore = (LCS(B_s, B_c) + LCS(A_s, A_c)) / (len(B_s) + len(A_s))`, where `LCS` is the length of the
  longest common subsequence of two token sequences. Before-tokens are aligned only with before-tokens and after-tokens
  only with after-tokens. With `len(B_s) + len(A_s) = 0` the score is 0 for every candidate.

### 6.4 Fuzzy quote ([40 §4.5] step 4)

- **Error budget.** `k = ⌊FUZZY_BUDGET × len(exact)⌋` with `FUZZY_BUDGET` = HOLE(F20-fuzzy-budget) (draft 1/4).
  Distances are Levenshtein distances over bytes with unit costs.
- **Regions**, in order: R1 = `[max(0, a − SPAN), min(len(N′), z + SPAN))` where [a, z) is the byte range in N′ of the
  hint lines that exist and `SPAN` = HOLE(F20-fuzzy-span) (draft 16,384 bytes; "±16 KB", open point 21), skipped when
  no hint line exists; R2 = the scope's range when the scope resolves uniquely; R3 = all of N′.
- **Candidates in a region R.** For each end offset e in R, `d(e)` = the least distance between `exact` and a substring
  `N′[s .. e)` with s ≥ R's start. E = {e : d(e) ≤ k}. Repeat: take the element of E least by (d(e), e); record it;
  remove from E every e′ with |e′ − e| < `len(exact)`. For each recorded e, s is the largest start with distance d(e).
  (Myers' bit-parallel matcher computes d(e); the model enumerates by definition, P11.)
- **Score of a candidate** [s, e): `q = (len(exact) − d) / len(exact)`; `ps = 1 − lev(prefix, N′[max(0, s −
  len(prefix)) .. s)) / len(prefix)`; `ss = 1 − lev(suffix, N′[e .. e + len(suffix))) / len(suffix)` (an empty prefix or
  suffix gives 1); `ws` = the window score of §6.3 around the candidate's lines, 0 without a window.
  `score = (w1·q + w2·ps + w3·ss + w4·ws) / (w1 + w2 + w3 + w4)` with (w1, w2, w3, w4) = HOLE(F20-fuzzy-weights)
  (draft (50, 20, 20, 10)).
- **Acceptance.** Candidates with `q < FUZZY_ACCEPT` = HOLE(F20-fuzzy-accept) (draft 3/4) are discarded. The first region
  that holds at least one candidate decides: its candidates are ordered by (score descending, s ascending); the first is
  accepted, as `edited`, when it is the only one or its score exceeds the second's by at least `FUZZY_MARGIN` =
  HOLE(F20-fuzzy-margin) (draft 2/100); otherwise the anchor is `ambiguous`.
- **Same-kind headers.** For a `symbol` or `heading` anchor whose scope did not resolve uniquely, only candidates that lie
  inside the header text of an item header of the same kind (a Rust item of the same keyword; a Markdown heading of the
  same level) count, and the margin is `HEADER_MARGIN` = HOLE(F20-header-margin) (draft 1/10) ([40 §4.5], [41 m8]).
  Finding item headers needs a scanner, so while §6.1's interim scanner rule holds this restriction does not apply: an
  imported `symbol` or `heading` anchor's header quote is matched as a quote, with every candidate and `FUZZY_MARGIN`.
- **`range` anchors.** The start quote is matched as above; for each start candidate the end quote is searched exactly,
  then fuzzily with its own k, after the start candidate and within the spread of §6.2 step 4; the best end candidate by
  (q descending, offset ascending) is taken, and a start candidate without one is discarded. The pair's q is the smaller
  of the two, ps comes from the start quote's prefix, ss from the end quote's suffix, and ws from the window around the
  pair's lines.

### 6.5 Scope only, `lines` anchors, watch, cross-file ([40 §4.5] steps 5–8)

- **Scope only.** A `symbol` or `heading` anchor whose scope resolves uniquely while steps 3–4 found nothing →
  `edited` (coarse). The step needs a scanner and does not run while §6.1's interim scanner rule holds.
- **`lines` anchors.** L = h2 − h1 + 1. For every line j with j + L − 1 ≤ the number of lines of t′, the window around
  [j, j + L − 1] is scored against the stored window (§6.3). The best j by (score descending, j ascending) is accepted when
  its score is at least `LINES_MIN` = HOLE(F20-lines-min) (draft 1/2) and exceeds the second best by at least
  `WINDOW_MARGIN`; then `XXH3-64(ST(j, j + L − 1)) = span_hash` gives `moved` (`fresh` when j = h1), and anything else
  `orphaned`. When the line-hash array is capped (§2.4), the anchor is `unverified (size)`. (`LINES_MIN` is this
  chapter's addition: [40] gives no minimum, and without one a low alignment next to trivial lines could match; open
  point 23.)
- **`text-unavailable` anchors** ([40 §5.7]) skip the quote steps: hint, then the window alignment of the `lines` rule
  over the hint length, then scope only, else `orphaned`; never `fresh` by quote.
- **Watch.** `header` watch: a changed body keeps `fresh`. `span` watch: a changed span is `edited`. A `file` anchor with
  `watch = span` is `edited` iff the current content's `oid` differs from `blob` (*false* membership, §2.3); a pair of
  different algorithms leaves the anchor `unverified`.
- **Binary current content**: every span anchor is `orphaned`; the file state decides the link.
- **Cross-file** (`--deep`, or a `split` file): the exact quote over the pieces, or over files changed since the anchor's
  `git` commit, gives a proposal to re-point the anchor; no constant.

### 6.6 Result order and subset-consistency (disposition of review S-12)

[40 §8.3.2] P11 and WP-77 require FL-1's resolver to be **subset-consistent** with the model's brute-force search: the
same result or a more conservative one, never a different target. The order is [40 §8.3.2]'s (review S-12); for the
functions of this chapter it reads ("r1 ⊒ r2": r1 is at least as conservative as r2):

- **Anchor results** (state, span): `moved` and `fresh` with one span are equal; r1 ⊒ r2 iff r1 equals r2; or r1's
  state is `ambiguous` or `orphaned` and r2 has a span; or r1 is `edited` with span S and r2 is `fresh` or `moved` with
  span S; or r1 is `unverified`. A result with a span is never ⊒ a result with a different span.
- **File results**: a proposal (`moved-needs-confirm`), `ambiguous`, `missing` or `unverified` is ⊒ any re-bind; a
  re-bind is ⊒ only the same re-bind. A re-bind to a path other than the model's fails P1 and P11.

[F13] (WP-16) cross-lists this order with P11 and P1.

## 7. The constant table

Every constant of resolver version 1, with the name WP-62's constant module uses:

| Name | Value | Unit | § | Source |
|---|---|---|---|---|
| `RESOLVER_VERSION` | 1 | — | 1.3 | [40 §2.6, §2.7] |
| `READ_RETRIES` | 1 | re-reads | 2.4 | this chapter (review A1P-05) |
| `FP_MIN_CHARS` | 3 (lines of ≤ 3 characters dropped) | characters | 2.6.1 | [40 §2.5] |
| `SKETCH_K` | 64 | values | 2.6.3 | [40 §2.5] |
| `SKETCH_BITS` | 32 | bits | 2.6.2 | [40 §2.5] |
| `TINY_LINES` | 5 | lines | 2.6.5 | [40 §4.4] |
| `TINY_BYTES` | 64 | bytes | 2.6.5 | [40 §4.4] |
| `WINDOW_BITS` | 16 | bits | 2.7.1 | [40 §2.7] |
| `WIN` (`WINDOW_LINES`) | HOLE(F20-window-lines), draft 16 | lines per side | 2.7.2 | [40 §2.7] |
| `WINNOW_K` | HOLE(F20-winnow-k), draft 5 | tokens | 2.9 | [10 §5.8b] |
| `WINNOW_W` | HOLE(F20-winnow-w), draft 4 | k-grams | 2.9 | [10 §5.8b] |
| `EXACT_LIMIT` | 65,536 | fingerprint lines or values per side | 2.10.5 | this chapter |
| trivial-line bytes | `WS` ∪ `{ } ( ) [ ] ; ,` | — | 2.5 | [40 §2.7] |
| never-candidate list | the 21 patterns and 2 contextual rules of §4.7 | — | 4.7 | [40 §4.3]; [80 §2.11.4] rule 5 |
| `LIST_MAX` | 3 | candidates or proposals | 4.1 | [40 §2.6, §4.4] |
| `SKEW` | HOLE(F20-clock-skew) | ns | 5.1 | review S-17 |
| NTFS `btime` class | HOLE(F20-btime-ntfs), draft `TunneledNotCopied` | — | 5.9 | [80 §2.11.1] |
| ChangeTime condition | HOLE(F20-ctime-rename), draft required | — | 5.9 | review S-16 |
| `REPLACED_MAX` | 29/100 (strict) | containment | 5.4.1 | [40 §4.4] |
| `REPLACED_MIN_LINES` | 5 | lines | 5.4.1 | [40 §4.4] |
| `PREFIX_MIN_NODES` | 2 | nodes | 5.10, 5.16 | [40 §4.3, §4.4] |
| `E6_MAX_COMMITS` | 2,000 | commits | 5.11.2 | [40 §4.3] |
| `E6_SLACK_MS` | 86,400,000 (1 day) | ms | 5.11.2 | [40 §4.3] |
| `SPLIT_MIN_PIECES` | 2 | files | 5.11.4 | [40 §4.4] |
| `SPLIT_NIO` | 4/5 | containment | 5.11.4 | [40 §4.4] |
| `SPLIT_SUM` | 3/5 | containment | 5.11.4 | [40 §4.4] |
| `GS_CHUNK` | 64 | bytes per span at most | 5.11.4 | git 2.54.0 `diffcore-delta.c` |
| `GS_HASHBASE` | 107,927 | span hash modulus | 5.11.4 | git 2.54.0 `diffcore-delta.c` |
| `GS_TEXT_PREFIX` | 8,000 | bytes tested for NUL | 5.11.4 | git 2.54.0 `buffer_is_binary` |
| `E6_STRONG` | 90 | git similarity index (%) | 5.11.4 | [40 §4.4] |
| `E6_WEAK` | 20 to below 50, with containment ≥ 4/5 | git similarity index (%) | 5.11.4 | [40 §4.4] |
| `E8_MIN` | 4/5, both directions | containment | 5.13 | [40 §4.3] |
| `STAGE1_TOP` | 10 | candidates | 5.14 | [40 §4.3] |
| `SIM_STRONG` | 1/2 | pair score | 5.15 | [40 §4.4] |
| `SIM_MARGIN` | 1/5 | pair score | 5.15 | [40 §4.4] |
| `SIM_WEAK` | 3/10 | pair score | 5.15 | [40 §4.4] |
| `MERGED_OIN` | 4/5 | containment | 5.15 | [40 §4.4] |
| `MERGED_NIO_MAX` | 1/2 (strict) | containment | 5.15 | [40 §4.4] |
| `QUIESCE_NS` | 50,000,000 (50 ms) | ns, monotonic | 5.17 | [40 §4.1] P10 |
| `QUOTE_LINES` | HOLE(F20-quote-lines), draft 4 | non-trivial lines | 6.1 | [40 §2.7] |
| `QUOTE_MAX` | HOLE(F20-quote-max), draft 128 | bytes | 6.1 | [40 §2.7] |
| `QUOTE_DEFAULT` | HOLE(F20-quote-default), draft 64 | bytes | 6.1 | [40 §2.7] |
| `CONTEXT` | HOLE(F20-context), draft 32 | bytes | 6.1 | [40 §2.7] |
| `CONTEXT_MAX` | HOLE(F20-context-max), draft 64 | bytes | 6.1 | [40 §2.7] |
| `CONTEXT_MARGIN` | HOLE(F20-context-margin), draft 1/10 | score | 6.2 | [40 §4.5] |
| `WINDOW_MARGIN` | HOLE(F20-window-margin), draft 15/100 | score | 6.2, 6.5 | [40 §4.5] |
| `RANGE_SPREAD` | HOLE(F20-range-spread), draft 2 | × hint lines | 6.2 | [40 §4.5] |
| `FUZZY_BUDGET` | HOLE(F20-fuzzy-budget), draft 1/4 | errors per quote byte | 6.4 | [40 §4.5] |
| `SPAN` | HOLE(F20-fuzzy-span), draft 16,384 | bytes each side | 6.4 | [40 §4.5] |
| fuzzy weights | HOLE(F20-fuzzy-weights), draft (50, 20, 20, 10) | — | 6.4 | [40 §4.5] |
| `FUZZY_ACCEPT` | HOLE(F20-fuzzy-accept), draft 3/4 | quote similarity | 6.4 | [40 §4.5] |
| `FUZZY_MARGIN` | HOLE(F20-fuzzy-margin), draft 2/100 | score | 6.4 | [40 §4.5] |
| `HEADER_MARGIN` | HOLE(F20-header-margin), draft 1/10 | score | 6.4 | [40 §4.5] |
| `LINES_MIN` | HOLE(F20-lines-min), draft 1/2 | window score | 6.5 | this chapter |

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [40] R-14 | complete: thresholds (§4.4, §4.5), `is_text`, `fold_v1`, the window-hash function, the never-candidate list, the 50 ms quiescence, the E6 window bounds with the 2,000-commit bound, the E3d identity rule and the path-reuse check, and the per-OS rules of [80 §2.11.4] (copy rule with `TunneledNotCopied`, no clone indicator and a unique creation time; twins, "spelling differs on disk" and the normalization rule; reads with ids; sorted candidates; the never-candidate additions and trash locations; `EXDEV`; Unix busy states; whole-id identity; denials as `Unknown`; the frontier's racy threshold) | §1–§7 |
| [60 §2.5] R4 reservations, row R-14 | complete, as [40] R-14 | §1–§7 |
| [60 §2.5] audit row "Resolver constants (R-14)" | complete: E3d exact only on a file-id, size + mtime or `oid` match; a present path with a changed file id → `ambiguous (path reused)`; the E6 bound as a constant; `fold_v1` at Unicode 17.0.0; the twin rule; the unique-creation-time copy rule; whole-`OsFileId` identity with the Linux `hgen`; denials as `Unknown` | §5.7, §5.4.2, §5.11.2, §3.1, §3.5, §5.9, §5.3, §4.8 |
| [80] X-F7 | P6 only: `fold_v1 = NFD(full_casefold(NFD(x)))` at Unicode 17.0.0. P1–P5 and P7–P12 are [OS/path]'s and [F08]'s | §3.1–§3.3 |
| [80] X-F8 | the R-14 half: rules 1–9 of [80 §2.11.4] and the frontier's racy threshold from a file-system timestamp. The tagged layouts (`OsFileId`, timestamps, `JOURNALCUR`, `DIRMAP`, the `TREES` additions, the `FSINTENT` holder) are [F11]'s | §3.5, §3.6, §4.1, §4.6–§4.8, §5.2, §5.3, §5.9, §5.12.1, §5.19, §5.20 |
| [40] R-8 and [50] F13 (`PATHIDX`) | the fold function of the key order only; the section is [F09]'s | §3.3 |
| [40] R-9 (fingerprint blob class) | the fingerprint value's bytes; where and how it is stored is [F10]'s and [F11]'s | §2.6.4 |
| [40] R-4 and R-10 (anchor record, selector block) | the contents of the `window`, `span_hash`, `hint`, quote, `end`, prefix, suffix and `occurrence` values at capture, and the window value's bytes; the record and block layouts are [F08]'s and [F07]'s. The per-kind anchor fixtures of `COVERAGE.md` row R-4 (R-FIX, WP-20) take these values from §2.7.3, §2.8 and §6.1 over a stated content, and capture no scope while §6.1's interim scanner rule holds | §2.7, §2.8, §6.1 |
| [40] R-12 (I-F10, I-F13) | the resolver's inputs and the constants that make the invariants hold; the invariant texts are [F13]'s and [F18]'s | §1.3, §5.9 |
| [40] R-18 (`FILEOBS`) | the meaning of the fields the resolver compares (stat quadruple, `verified_at`, `last_oid`, recorded state); the layout is [F11]'s | §5.2, §5.4, §5.9 |

## Holes

| id | what | decided by (measurement n / WP) | candidates | constraint the value must meet |
|---|---|---|---|---|
| F20-window-lines | `WIN`: non-trivial lines hashed on each side of an anchor's quote span | replay row 2 of [40 §8.3.4] (WP-76), filled by WP-81a | 16 (draft, [40 §2.7]); 8 | window tie-break agrees with full histogram-diff mapping on ≥ 99 % of duplicate-quote cases; `4 + 4 × WIN ≤ 68` ([40 §2.7]'s bound, so WIN ≤ 16) |
| F20-quote-lines | most non-trivial lines a `quote` span may have before it becomes a `range` | replay row 2 (WP-76) | 4 (draft, [40 §2.7]); 2; 6 | row 2: ≥ 96 % resolved, 0 silent wrong at the exact class |
| F20-quote-max | most bytes of a `quote` span, and of a header quote | replay row 2 (WP-76) | 128 (draft, [40 §2.7]); 96 | as above; [11 §2.7]: longer quotes need fuzzy matching more often |
| F20-quote-default | length of the start and end quotes of a `range` | replay row 2 (WP-76) | 64 (draft, [40 §2.7]); 32; 128 | as above; [11 §2.7] measured 32 B as ambiguous in 8 % of Rust cases and 256 B as fragile |
| F20-context | prefix and suffix length at capture | replay row 2 (WP-76) | 32 (draft, [40 §2.7], [11 §2.7]); 16 | as above |
| F20-context-max | widened prefix and suffix length | replay row 2 (WP-76) | 64 (draft, [40 §2.7]); 48 | ≥ F20-context; as above |
| F20-context-margin | margin of the context score that breaks a duplicate-quote tie | replay row 2 (WP-76) | 1/10 (draft, [40 §4.5], [11 §4.3]) | 0 silent wrong at the exact class |
| F20-window-margin | margin of the window score (duplicate quotes, `lines` anchors) | replay row 2 (WP-76) | 15/100 (draft, [40 §4.5]); 1/10; 1/5 | window tie-break ≥ 99 % agreement with the full diff; 0 silent wrong |
| F20-range-spread | how far after its start quote a range's end quote may lie, in multiples of the captured hint length | replay row 2 (WP-76) | 2 (draft, [40 §4.5]); 3 | as above |
| F20-fuzzy-budget | Myers error budget per quote byte | replay row 2 (WP-76) | 1/4 (draft, [40 §4.5], [11 §2.7]); 1/5 | ≤ 1/4 ([11 §2.7]: a `len/2` budget turned orphans into silent matches); 0 silent wrong |
| F20-fuzzy-accept | least quote similarity of a fuzzy match | replay row 2 (WP-76) | 3/4 (draft, [40 §4.5]); 4/5 | ≥ 1 − F20-fuzzy-budget (otherwise inert) |
| F20-fuzzy-span | bytes searched on each side of the hint before the scope and the whole file | replay row 2 (WP-76) | 16,384 (draft, [40 §4.5] "±16 KB", [11 §2.8]); 16,000 | the chosen value is part of the definition of the first region, so it changes results; row 2 targets |
| F20-fuzzy-weights | weights (quote, prefix, suffix, window) of the fuzzy score | replay row 2 (WP-76) | (50, 20, 20, 10) (draft, [40 §4.5]); (50, 20, 20, 2) ([11 §0.1], position weight) | non-negative integers, sum > 0; row 2 targets |
| F20-fuzzy-margin | top-2 margin of an accepted fuzzy match | replay row 2 (WP-76) | 2/100 (draft, [40 §4.5], [11 §4.3]) | 0 silent wrong at the exact class |
| F20-header-margin | top-2 margin for a same-kind header match when the scope did not resolve | replay row 2 (WP-76) | 1/10 (draft, [40 §4.5], [41 m8]) | a renamed item with a same-prefix sibling ends `ambiguous` ([41 m8]) |
| F20-lines-min | least window score for aligning a `lines` anchor | replay row 2 (WP-76) and P11's generated cases (WP-77) | 1/2 (draft, this chapter); 3/4 | 0 silent wrong for `lines` anchors; P11 subset-consistent |
| F20-winnow-k | winnowing k-gram length in tokens | WP-66 (stage-2 measurements on synthetic rename, reflow and identifier-rename sets, [10 §5.8b]'s method) with WP-76 | 5 (draft, [10 §5.8b]); 4; 6 | on [10 §5.8b]'s variants, the true file ranks first and the pair score (the maximum of lines and tokens) stays ≥ 1/2 for the reflow and identifier-rename variants; WP-66's recall@10 target holds |
| F20-winnow-w | winnowing window in k-grams | as F20-winnow-k | 4 (draft, [10 §5.8b]); 8 | as above; the guarantee threshold `K + W − 1` tokens |
| F20-btime-ntfs | `VolumeCaps.btime` class of NTFS volumes on Windows 11 | measurement 15 ([40 §8.3.6]: creation-time behaviour of `mv`, `cp`, `cp -p`, Claude Code's tools, Codex `apply_patch`, NTFS tunneling, on the project volume; WP-55), with the copy paths an agent machine runs added (pass 1, S1-31, lens S decision D-1): `robocopy /COPY:DAT`, PowerShell `Copy-Item`, Explorer copy and paste, archive extraction (`tar -x`, `Expand-Archive`), and `git checkout` of a moved file | `TunneledNotCopied` (draft, [80 §2.11.1]); `Unforgeable` (copy-rule line 2 never applies on Windows); `Absent` — some measured tool gives a copy its source's creation time, so creation times identify nothing and line 2 never applies on NTFS, as for `CopiedByClones` on macOS (the value [OS/project §4.3] gives ReFS) | `TunneledNotCopied` only if no measured tool gives a new file its source's creation time; if one does, `Absent` |
| F20-ctime-rename | whether copy-rule line 2 requires q's ChangeTime clearly after V | measurement 15 (a same-volume rename by `MoveFileExW` with and without `MOVEFILE_WRITE_THROUGH`, `mv`, `Move-Item`, `os.rename`, on the project volume; WP-55) | required (draft, review S-16; [80 §2.11.1] "ChangeTime, set by rename", [10 §5.5]); line 2 never `exact` on Windows | "required" only if every measured rename sets the moved file's ChangeTime to a value not before the rename's start minus the volume's granularity |
| F20-clock-skew | `SKEW`: the largest difference between a file timestamp and the `hlc` wall time of the same moment on one machine | measurement 15 (fresh-file timestamps against the process `hlc`, idle and loaded, on the project and system volumes; WP-55) and measurement 22 (clock step, sleep, hibernation rows; WP-52) | the smallest whole number of milliseconds ≥ the measured maximum plus the volume's recorded granularity; no draft (at M0 only WP-92 uses it, as a parameter) | ≥ the measured maximum; every rule of §5.1 stays on its conservative side |

## Open points for the review

1. **Conflict: [40 §4.4–§4.5] versus [60 §3.1] on who fixes R-14's values.** [40] states the anchor and file-level
   constants as "resolver v1 constants"; [60 §3.1] fixes "R4's resolver constants (R-14) and anchor layout (replay
   corpora)" at M0 exit; [F01 §2.5] makes a value a hole when [60 §3.1] lists it. Resolution (§1.4): holes for the values
   an M0 measurement can decide — the anchor constants that replay row 2 exercises, winnowing k and w, and the three
   measurement-15/22 facts — each with [40]'s value as draft; the file-level thresholds, E6 bounds, quiescence and
   pattern lists stay at [40]'s values, because the replay rows that exercise them (5–7) gate M6, not M0. The
   precedence rule ([40] for its own reservations) is honoured: every draft is [40]'s value.
2. **Resolved gap ([PLAN §3.3]): the window hash** is `low(XXH3-64(nl(l)), 16)` over the trimmed line (§2.7.1).
3. **Resolved gap: the sketch line hash** is `low(XXH3-64(f), 32)` over the collapsed fingerprint line (§2.6.2); the
   sketch is the bottom 64 of the set of these values.
4. **Resolved gap: "non-trivial line"** (§2.5). [40 §2.7] says "blank or contain only braces"; this chapter includes the
   bracket and separator bytes `{ } ( ) [ ] ; ,` so that `});` and `},` lines, which carry no information, neither start
   a quote nor fill a window. Alternative: `{` and `}` only. The replay (WP-76) may propose the alternative.
5. **Resolved gap: "≤ 3 characters"** (§2.6.1) counts characters as bytes outside `80`–`BF`, so a 2-character Cyrillic
   line (4 bytes) is dropped as [10]'s character-based probe dropped it.
6. **Resolved gap: `distinct` beyond 64 lines** (§2.6.3) is the KMV estimate, so a fingerprint needs constant memory and
   no key; exact below 65 values.
7. **Normalisation of window lines.** Window hashes use the trimmed line, like quotes, not the collapsed fingerprint
   line. Re-indentation is absorbed by trimming; interior whitespace changes change a hash.
8. **For WP-12 and WP-15: the window value** (§2.7.3). [40 §2.7] puts "the span's offset in the window" in a ≤ 68-byte
   value; [40 §5.7] writes the `.moi` `window` as "base64url of the u16 hashes". This chapter defines W with a 4-byte
   header (`n_before`, `n_after`); [F07 §8.2] carries W as one byte string, `lp(W)`, in the selector block (field 11)
   and [F14 §6.7] writes base64url(W) without padding ([F14 §2.6]), as this point asked. Closed (checked in pass 1,
   round 3).
9. **`replaced` and E8 always use sketch estimates** (§2.10.2), even when git holds the old blob, so a tree with and
   without the git object gives the same state (I-F10).
10. **Similarity never depends on `files.max-line-hashes`** (§2.10.5): `EXACT_LIMIT` = 65,536 is a resolver constant equal
    to the key's default. The key only bounds the line-hash array of the window steps, whose exhaustion is
    `unverified (size)` ([AR §5e.3]).
11. **The normalization rule applies to the last component** (§3.6). A path whose directory component differs by
    normalization falls through to E3d and the other sources. The rule also requires the NFD-equal entry to be unbound,
    so a node whose twin exists as a separate file on NTFS never resolves to that twin.
12. **Never-candidate matching** (§4.7) is on the basename only, ASCII case-insensitive, with `?` matching one byte, and
    uses one list on every OS. Case-insensitive matching and the basename-plus-suffix rule under `eqi` err towards
    excluding candidates, which can only yield `missing` or a proposal, never a wrong target.
13. **Hard links** (§5.3): a file with more than one link yields at most `strong` from E3 or E3d, because the path an id
    lookup returns for it is not determined. [40 §2.4] states only the exclusion of bound paths.
14. **`directory moved, file replaced`** (§5.7) is never applied automatically, also under `files.policy.auto = strong`.
15. **Disposition of review S-16** (§5.9): adopted here, as review a1-S assigns it to this chapter ("holes of chapter
    20 … listed there when WP-14b takes the dispositions"); [40 §4.3]'s copy rule does not state it yet. Copy-rule line 2
    also requires q's ChangeTime clearly after V, subject to HOLE(F20-ctime-rename). The condition only removes `exact`
    outcomes. [40]'s review log defers S-16 to this chapter with measurement 15 and asks that it account for V being a
    lower bound under A1P-04: it does not fully (the residue below), and closing it needs either a stored "last seen
    present" time that reads may not write (I-F5) or HOLE(F20-btime-ntfs) showing that no tool copies creation times on
    NTFS, which makes the residue unreachable. Residue: V is a lower bound of the true last verification (review A1P-04), so a
    creation-time-preserving copy made between V and the true last verification can still pass; it needs a tool that
    copies creation times on NTFS, which HOLE(F20-btime-ntfs) tests. Pass 1 (S1-31, lens S decision D-1): the ChangeTime
    condition is kept; the hole gains the candidate `Absent` (a measured tool copies creation times: line 2 never applies
    on NTFS) and measurement 15 tests the copy paths an agent machine runs (`robocopy /COPY:DAT`, `Copy-Item`, Explorer,
    archive extraction, `git checkout`). With the hole's constraint met, the residue is unreachable for every measured
    tool; a creation-time-copying tool outside the measured set remains a known risk, which the owner signs with the
    measurement's result (`reviews/owner-questions.md`).
16. **E6 for named roots** (§5.11): E6 applies only to root `project` of a git worktree. A named root that is a git
    worktree top-level of another repository could use that repository's history; resolver version 1 does not.
17. **For WP-13: the frontier's racy threshold** (§5.12.1). [80 §2.11.3] needs to know at the next settle that a
    recorded mtime was racy, but [80 §2.11.2]'s `DIRMAP` row has no such mark and `TREES` stores no file-system
    threshold. Resolution: a settle never records a racy mtime (git's smudging, applied by omission); no layout change.
18. **Budget cuts in the similarity search** (§5.14) give `unverified (budget)` with the proposals kept, and a proposal
    from a partial scan is never applied automatically.
19. **`QUOTE_DEFAULT` versus `QUOTE_MAX`** (§6.1). "`exact` ≤ 64 B by default (up to 128 B when needed)" is read as: a
    `quote` span of up to 128 B is quoted whole; `range` start and end quotes are 64 B; header quotes are cut at 128 B.
20. **Range spread in lines** (§6.2): the anchor stores its hint in lines, not the span's byte length, so "twice the
    captured span length" is counted in lines.
21. **"±16 KB"** (§6.4) is read as 16,384 bytes of N′ on each side of the hint lines.
22. **`unverified` details** — resolved with [F18] in pass 1 (A1-45). [40 §2.9] (as revised for review S-08) closes the
    `unverified` detail set at `budget`, `cloud-only`, `commit not in this repository`, `no tree`, `git` and `size`. This
    chapter maps its reason `unstable` (§2.4: a read whose passes disagreed twice; [40 §2.5] says only "unverified") to
    `budget`. Its reason `unreadable` — a denial on the stat of p or on a content read ([80 §2.11.4] rule 9, §4.8) — is
    [F18 §4.6]'s detail 59, which [F18] adds to the closed set with 60 and 61 for the owner's sign-off ([F18] open point
    12). The place details (codes 38–42) and the E3d detail "directory moved, file replaced" (21) are in [F18 §4.6].
23. **`LINES_MIN`** (§6.5) is a constant [40] does not name; it closes a silent-wrong path for `lines` anchors, whose span
    is trivial text by definition. It is a hole with draft 1/2.
24. **Disposition of review S-12** (§6.6): [40 §8.3.2] now states the order; §6.6 restates it for this chapter's
    results and adds only that an anchor's `unverified` is conservative, as [40] says for file results. [F13] (WP-16)
    cross-lists the order.
25. **Disposition of review S-13** (§1.3): the resolver's inputs are listed here; WP-14 restates I-F10.
26. **Disposition of review S-17** (§5.1): as [40 §4.3] "Clock domains" now states it — nanoseconds for every domain,
    one margin `SKEW` (a hole of this chapter), and the sides [40] fixes for copy-rule line 3, E7, the G4 window and
    `planned` binding. This chapter adds only the side of its own ChangeTime condition (clearly after) and applies E7's
    side to E8, which [40 §4.3] defines by the same "since T's last settle".
27. **Disposition of review S-18** (§2.3): as [40 §2.5] now states it — a per-root, `init`-fixed algorithm (`project`:
    the store's repository format at `init`; named roots and `abs`: SHA-1), and a pair of different algorithms is
    "content unknown". This chapter makes membership tests three-valued so that "unknown" is never read as "equal" or as
    "different". For WP-11 and WP-16: the `project` root's algorithm is an `init`-fixed parameter that [F04] must hold
    and [F17] must list; neither chapter lists it yet. (An earlier draft of this chapter compared contents per stored
    algorithm; it was withdrawn when [40] fixed the rule.) Pass 1 (A1-15, S1-28, P1-4): closed. The `InitParams` block
    has no spare byte, so [F04 §5.16] holds the value as `project_oid_algo` at slot offset 1072, [F17 §2.1] lists it
    beside the block under IP-1–IP-3, and [CFG §7.6] records it at `init`; §2.3 cites it.
28. **Disposition of review A1P-05** (§2.4): adopted as [40 §2.5] now states it (one handle, the normalised length, size
    and last-write time, one retry); this chapter adds a raw-bytes XXH3-64 comparison between the passes to the
    size and last-write checks, which a same-size overwrite within one timestamp tick would pass.
29. **Disposition of review A1P-06** (§5.17): the constant is stated here; the no-sleep rule of hook and server paths is
    [F16]'s.
30. **Scope scanners are not specified by any chapter.** The Rust, Markdown and TOML scanners ([40 §2.7.1]) decide the
    `scope` selector, header lines, item kinds and heading levels that §6 uses, and therefore resolution results, but no
    chapter fixes their grammar (WP-63 builds them from [40]). Pass 1 (A1-14, S1-4, P1-20): the grammar is required as a
    normative appendix of this chapter (item kinds per language, the exact name text, the numbering-stripping pattern,
    the TOML table-path form, item boundaries, behaviour on unparsable input, a fixture per construct) before the freeze
    and before WP-63 is accepted. Until it exists, §6.1's interim scanner rule keeps every hashed byte scanner-free: no
    scope is recorded (as [F08 §10.3.1] states), and the `symbol` and `heading` forms, whose header line, quote, hint and
    span hash a scanner would decide, are refused (the closure check's open point 2). The appendix is not written in pass 1:
    it is a grammar of three languages with fixtures, beyond a review round; its owner is R-SPEC-R with WP-63.
31. **Span anchors on binary content** (§6.1, §6.5) are refused at capture and resolve `orphaned`; [40] does not say.
    The refusal is [F19 §10.2] `anchor_spec`, case `binary` (exit 2), which §6.1 cites with the other capture cases
    (pass 1, round 3). Closed.
32. **A BOM at the start of a file** is removed from the anchor text (§2.5), matching the removal from quote input
    ([40 §2.7]), so a quote captured from line 1 and the same quote given by `--quote-file` agree.
33. **Several distinct `strong` targets are `ambiguous`** (§5.5 selection step 3). [40 §4.3] says "the best STRONG result
    becomes the proposal" and [40 §4.4] requires a unique strong candidate; this chapter keeps every strong target as a
    proposal, renders `ambiguous` when there are two or more, and applies policy `strong` only to a unique one.
34. **E5 composes every recorded entry** (§5.10) in the entries' (`hlc`, `from`, `to`) order, as merge composition does.
    A filter "only entries newer than F's observation" was considered and rejected: [40 §2.4] now defines
    `pathmove.hlc` as the writer's HLC at candidate computation and forbids comparing it with a commit header's `hlc`
    (reviews A-M2, S-20). An entry already reflected in F's path does not cover p, or maps p to a path whose content must
    still equal F's for an `exact` result.
35. **Arithmetic** (§1.2). All scores are exact rationals and no step uses floating point, so resolution is bit-identical
    across processes and in the model (P3). Stored or rendered scores (`FILEOBS` proposals, the `relink` score) are
    rounded by their owning chapters ([F11], [F18]; review A-M1 gives `relink` two decimals, half-even).
36. **E6's inexact pairs are scored with git's similarity index** (§5.11.4), because [40 §4.4] states their thresholds in
    git's percentages and [40 §2.2]'s `relink` records "git's per-commit similarity divided by 100". Pass 1 (A1-34): the
    index is now defined normatively in §5.11.4 (the span hash, the counts, the exact integer formula), with golden
    vectors checked against git 2.54.0, so no normative reference to git's source is needed in [F01 §2.2]; WP-74's
    differential against `git diff-tree -M20%` stays a conformance test. [40 §4.4] gives no class
    for a pair between 50 % and 89 %; this chapter applies its similarity row (≥ 0.5, margin ≥ 0.2, a directory or
    basename corroboration) with `gs / 100` as the score. Containments for the split, merged and weak tests stay this
    chapter's line measures, as [40 §4.4] asks ("old blob from git").
37. **The design is being revised while this chapter is written.** [40] carries uncommitted edits in the working tree
    that answer the A1 re-review (S-01 … S-20, A-M1, A-M2, A1P-01 … A1P-16). This chapter follows the text as it stood on
    2026-09-27: §2.3 (S-18), §2.4 (A1P-05), §5.1 (S-17), §5.10 (A-M2/S-20), §5.11.4 (A-M1's `git-pair`), §5.19 (A1P-01)
    and the closed `unverified` detail set (S-08). Pass 1 should re-check these sections against [40] as committed.
    Re-checked in pass 1, round 2, against [40] as committed (§2.5 S-18 and A1P-05, §4.3 "Clock domains" S-17, §2.4
    A-M2/S-20, §2.2 A-M1, §3.4 A1P-01, the §2.9 `unverified` row S-08): the sections agree; the one extension beyond
    [40]'s closed `unverified` set is detail 59 `unreadable`, which open point 22 and [F18] open point 12 carry to the
    owner. Closed.
38. **Representability before any OS call** (§4.9; pass 1, P1-15). A git-tracked path such as `x::$DATA` or `a:b` is
    legal on Linux but names an NTFS alternate data stream on Windows, and a `\?\` open bypasses Win32's name checks, so
    a stat could hash another file's content and a link could read `ok`. The cascade therefore tests every segment of
    every path with `representable_here` first and decides [F18 §4.6] detail 44 with no OS call. The matching check
    inside `ProjectFs` ([OS/path], [OS/project §2.3], [OS/fs §2.1]) is R-SPEC-P's; this chapter does not rely on it.
39. **Golden vectors for `gs`** (§5.11.4; pass 1, A1-34) were computed by an implementation of the §5.11.4 definition and
    agree with git 2.54.0 on them and on 60 random edited pairs of up to 65,000 bytes; WP-74 turns them into fixtures.
