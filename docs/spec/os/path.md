# OS layer: paths — the path-key canonical form (X-F7), rules P1–P12 and `os::path`

| Field | Value |
|---|---|
| Title | OS layer specification, part 2: stored paths, canonical roots, machine-local absolute paths, OS path conversion, the CLI boundary, representability and portability, the user-scope configuration location |
| Status | draft, pass 1 pending |
| Work package | WP-17b (role R-SPEC-P), part 2 of WP-17 ([PLAN §3.2] item 1) |
| Sources | [80 §2.10] (P1–P12, the CLI-boundary paragraph); [80 §2.11.1] (`case_rule`, `norm_insensitive`), [80 §2.11.4] rules 2 and 4; [80 §2.12] (user-scope config row, temporary files); [80 §3.1] X-F7, X-F9, X-F10, X-F11; [80 §8.0] (P12 added), [80 §8.1] m8, m9, m10; [40 §2.3] (`origin_path`), [40 §2.4] (roots, path rules I-F8, case and twins, reparse points); [40 §2.10] I-F8, I-F12; [40 §2.11] R-14, R-16; [AR §5a.1] (client-head key), [AR §5e.2]; [AR §13] (`files.portable-names`, the user scope); [60 §2.5] rows "Store layout", "Resolver constants (R-14)", "Cross-platform"; [X19 §9] (research input) |
| Reconciled with | [OS/README §1.3, §2.1] (this file carries X-F7, the name rules of X-F9, and the one `RelPath` type), [OS/README §4.2] (`canonical_root` and the path conversions belong to the `ProjectFs` surface), [OS/fs §2.1] (use-time checks), [F02 §7] (the user-scope location, which cites §10), [F20] (P6 `fold_v1`; the symlink target text of P8) |

---

## 1. Scope and placement

This file is normative for X-F7 (P1–P10 and P12) and for the path-shaped name rules P11 (a) and (b) that X-F9 freezes; P11
(c) is X-F10, whose home is [F02]. The rules are split over three crates by what they need:

| Part | Where it lives | Why |
|---|---|---|
| path value types and their syntax checks (`RelPath`, `AbsPath`, `CanonicalRoot`, `EntryName`) | `moirai-vfs` | every `ProjectFs` method takes them; they need no OS call |
| P3 (NFC for some untracked names), P5 (portability), P6 (`fold_v1`), and `representable(os, segment)` (§8.1) for every OS | `moirai-files` (FL-1, target-independent) | they need Unicode tables ([PLAN §6.2] R6); `representable` shares P5's device list and must be callable, and testable for all three OSes, from target-independent code |
| conversion between OS paths and stored paths, `canonical_root`, `canonical_abs`, the CLI boundary, the user-config location, and `representable_here(segment)` (§8.1), the build OS's use-time check | `moirai-os::path` (Windows built from M0) | they call the OS, or guard every OS call ([OS/project §2.3]) |

`fold_v1` itself is defined in [F20] (R-14); this file only states where it is used.

## 2. Path value types

### 2.1 `RelPath` — a root-relative path (P1, P4)

```
rel-path  = "" / segment *( "/" segment )
segment   = 1*seg-char                      ; and not exactly "." or ".."
seg-char  = any Unicode scalar value other than U+0000–U+001F, U+002F "/" and U+005C "\"
```

- The whole value is valid UTF-8 and is compared and hashed as **exact bytes**; no normalisation of any kind is applied by
  the type (I-F8).
- The empty value denotes the root directory itself. It is valid as an argument of an OS-layer call (for example
  `sync_dir` of the root) and never as a stored file path; the format's `path` value (R-1) requires a non-empty value
  ([F08]).
- There is no leading `/`, no trailing `/`, no empty segment, no `.` and no `..` segment (P1).
- A segment contains no `\` and no C0 control character (P4). A name that fails P4, or is not UTF-8, cannot become a
  `RelPath`; the OS layer reports it as `EntryName::Unrepresentable` (§2.4).
- The type sets no length limit. OS limits apply at use: Windows ≤ 255 UTF-16 code units per component and 32,767 in
  total with the `\\?\` prefix; Linux 255 bytes per component; macOS 255 UTF-8 bytes per component (§6).

This is the **one** `RelPath` type for store and project paths ([OS/README §2.1], [OS/fs §2.1]). Store names are a subset
(decimal numbers and fixed ASCII words, X-F10); [OS/fs §2.1] adds only use-time checks for store operations (`InvalidName`
for a segment Windows would store literally, such as one ending in `.`).

**The Rust form** (§11). `RelPath<'a>` is a `Copy` view over a validated `&'a str`, two words wide, and every seam takes it
**by value**: `rel: RelPath<'_>`, `dir: Option<RelPath<'_>>`, `name: RelPath<'_>`. An unsized `RelPath(str)` passed as
`&RelPath` cannot be built from a `&str` without `unsafe`, which `moirai-vfs` forbids ([OS/README §2.1]); the view keeps
the grammar, the byte-exact comparison and the absence of normalisation. The owned form is `RelPathBuf`; a caller holding
one passes `buf.as_rel_path()`, and a map keyed by `RelPathBuf` is queried with the borrowed text
(`RelPathBuf: Borrow<str>`).

### 2.2 `AbsPath` — a machine-local absolute path (P12)

```
abs-path        = win-drive-path / win-unc-path / unix-path
win-drive-path  = drive ":/" [ abs-rel ]           ; "X:/" alone is a drive root
drive           = %x41-5A                          ; A–Z, upper-case (P12)
win-unc-path    = "//" abs-seg "/" abs-seg [ "/" abs-rel ]   ; //server/share[/…]
unix-path       = "/" [ abs-rel ]                  ; the leading "/" is kept (P12)
abs-rel         = abs-seg *( "/" abs-seg )
abs-seg         = 1*abs-char                       ; and not exactly "." or ".."
abs-char        = any Unicode scalar value other than U+0000 and U+002F "/"
```

- `win-drive-path` and `win-unc-path` are produced only on Windows, `unix-path` only on Linux and macOS. No `\\?\`
  prefix is ever stored.
- An `AbsPath` is **machine-local** ([80 §2.10] P12): existence and `oid` checks only, never re-bound, never a candidate,
  never compared across machines or OSes. Its bytes are versioned, hashed and exported unchanged, so an `abs` artifact's
  derived uid is machine-specific by construction.

### 2.3 `CanonicalRoot` — a canonical top-level (P9)

A `CanonicalRoot` is a triple `{text: AbsPath, root_id: OsFileId, os: OsTag}`: the canonical text of §4, the
directory's `OsFileId` ([OS/project §3.1]; `kind` 0 where the volume has no trusted ids) and the OS tag of the process
that produced it ([OS/proc §2]).

### 2.4 `EntryName` — a name as an OS returned it

```rust
pub enum EntryName {
    /// The name is valid Unicode and passes P4: usable as a `RelPath` segment.
    Utf8(Box<str>),
    /// The name is not UTF-8 (Linux), has an unpaired surrogate (Windows), or contains "\" or a C0 control (P4).
    /// Holds the OS bytes: WTF-8 of the UTF-16 name on Windows, the raw bytes on Unix.
    Unrepresentable(Box<[u8]>),
}
```

An `Unrepresentable` name is never a candidate, is never stored, and renders with the escapes of §9. A path already in git
that contains such a component renders `unrepresentable path` ([F18], R-16).

The borrowed form, for enumerations that allocate nothing per entry ([OS/project §5.2]):

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EntryNameRef<'a> { Utf8(&'a str), Unrepresentable(&'a [u8]) }
impl EntryNameRef<'_> { pub fn to_owned(&self) -> EntryName; }
impl EntryName { pub fn as_entry_ref(&self) -> EntryNameRef<'_>; }
```

The two forms carry the same bytes and the same classification.

## 3. The rules P1–P12 (frozen, X-F7 and X-F9)

| # | Rule ([80 §2.10], normative) | Precision added here | Enforced by |
|---|---|---|---|
| P1 | A stored path is root-relative, `/`-separated, with no empty, `.` or `..` segment and no leading `/`, valid UTF-8 stored as exact bytes (I-F8) | the grammar of §2.1 | `RelPath` (`moirai-vfs`) |
| P2 | **Tracked file:** git's HEAD-tree spelling, bytes as git stores them, on every OS | the OS layer never rewrites a spelling; stat succeeding under another spelling on a case- or normalization-insensitive directory is reported as such ([OS/project §5.3]) | the link layer (M6) with the git object reader (M4) |
| P3 | **Untracked file:** the OS's enumerated spelling, except that on a normalization-insensitive volume (APFS, HFS+) the name is stored as `NFC(name)` when the repository has `core.precomposeUnicode = true`, or always when there is no git; on Windows and Linux names are never normalised | "normalization-insensitive volume" is `VolumeCaps` flag `norm_insensitive_always` ([OS/project §4.2]); a Linux casefold directory (`norm_follows_case`) does **not** trigger P3. NFC is the one git applies (open point 2) | `moirai-files` (port), reading `VolumeCaps` |
| P4 | **Refused at link time, on every OS:** a component containing `\` or a C0 control character; a name that is not UTF-8 (Linux); a name with an unpaired surrogate (Windows). Such a path already in git renders `unrepresentable path` and is never a candidate | `EntryName::Unrepresentable` (§2.4); the P4 refusal is exit 2 at the CLI ([F19]) | `RelPath`, `os::path` |
| P5 | **Portable by default:** `file mv`, `file add` and `link` warn about, and `file mv` refuses to **create**, a name some supported OS cannot hold (policy `files.portable-names = refuse \| warn`, `--allow-nonportable`) | the exact checks of §8.2 | `moirai-files` |
| P6 | `PATHIDX` fold `fold_v1(x) = NFD(full_casefold(NFD(x)))`, full case folding (statuses C and F), NFD, Unicode 17.0.0; used for collision detection, twin detection and index order only, never for identity | defined in [F20] | `moirai-files` |
| P7 | `origin_path` (the uid derivation input) follows P2 and P3 | — | the link layer |
| P8 | Symlinks: `lstat` semantics; a link names the link itself; a symlink's `oid` is taken over its target text, as git does | the target text per OS is [OS/project §5.6] | `ProjectFs` |
| P9 | **Canonical root, per component**, with the root's `OsFileId`; bindings, trees and root containment are looked up **by that id first and by spelling second**; two spellings with one root id are one tree, and a second binding is refused (I-F12) | the per-OS algorithm of §4 and the keys of §4.5 | `os::path::canonical_root` |
| P10 | Walks are relative (`openat`/`fstatat` on Unix; `\\?\` or handle-relative opens on Windows) | §6 | `os::path`, `os::project` |
| P11 | Names moirai writes into git or the store are portable by construction: (a) `schema/queries/<q>.moi`, `q` = 32 lower-case hex digits of BLAKE3-256 over the query name's bytes; (b) a ref-name segment is not a Windows device name (case-folded, before its first `.`) and does not end in `.lock`; a new ref name is NFC-normalised on input and refused (exit 2) when equal under `fold_v1` to a live ref name; (c) every store file name is built from decimal numbers and fixed ASCII words | (a): `q` is the hex of the first 16 bytes of the 32-byte digest, lower-case, over the query name's UTF-8 bytes as stored on the name line; (b): "Windows device name" is the list of §8.2 and "case-folded" is ASCII case folding; (c) is X-F10, specified in [F02] | [F14] (a), [F12] (b), [F02] (c) |
| P12 | **Root `abs`: a machine-local absolute path**: the OS-canonical absolute path with `/` separators, valid UTF-8, no empty, `.` or `..` segment; canonicalised per component as in P9 when it exists at record time, otherwise made absolute and lexically normalised; Windows `X:/…` with the drive letter upper-cased and no `\\?\` prefix, UNC as `//server/share/…`; Linux and macOS keep the leading `/` | the grammar of §2.2 and the algorithm of §5 | `os::path::canonical_abs` |

**At the CLI boundary** ([80 §2.10]): on Windows a path argument may use `\` or `/` and is converted to `/`; on Unix `\` in
an argument is a literal name character, so P4 refuses it rather than guessing; displays escape non-UTF-8 bytes as
`\xNN` (§7, §9).

## 4. The canonical root (P9)

### 4.1 Windows (built)

1. If the input is relative, make it absolute with `GetFullPathNameW` (after replacing `/` with `\`).
2. Open it: `CreateFileW(path, FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL,
   OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, NULL)`. Reparse points on the path (junctions, directory symlinks) and a
   `subst` drive are followed, so the result names the final directory.
3. `GetFinalPathNameByHandleW(h, …, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)`: every component in its on-disk spelling.
   The call fails on a volume mounted only in a folder (no drive letter) and on some virtual providers
   (`ERROR_PATH_NOT_FOUND`, `ERROR_INVALID_FUNCTION`, `ERROR_NOT_SUPPORTED`). `canonical_root` then returns the failure —
   `NotFound` for the first, `Unsupported` for the others, with `call` = `GetFinalPathNameByHandleW` — and invents no
   other spelling, because a guessed text could key a second tree for one directory (P9). The caller refuses the
   operation with exit 7 `no_canonical_path` ([F19 §10.2]), naming the directory and the OS error, and `doctor` names
   the same cause (pass 1, P1-37; the code, text and JSON keys are [F19]'s).
4. Rewrite the result: a leading `\\?\UNC\` becomes `//`; a leading `\\?\` is removed; every `\` becomes `/`; an ASCII
   lower-case drive letter at position 0 before `:` is upper-cased; a trailing `/` is removed except in a drive root
   `X:/`. The result must parse as `win-drive-path` or `win-unc-path` (§2.2) and be valid UTF-16 (else
   `Unrepresentable`).
5. `GetFileInformationByHandleEx(h, FileIdInfo)` gives the volume serial and `FILE_ID_128` for the root's `OsFileId`
   ([OS/project §3.1]); the kind follows the volume's file system ([OS/project §4.3]).
6. Close the handle. No handle outlives the call.

`D:\` and `d:\`, a junction into the tree and a `subst` of a local folder therefore all give one text and one root id; a
mapped network drive resolves to its UNC text ([80 §2.10] P9, [80 §2.6]).

### 4.2 Linux (port)

1. `open(path, O_PATH | O_DIRECTORY | O_CLOEXEC)`; `readlink("/proc/self/fd/<fd>")` gives the resolved absolute path
   (`realpath` semantics).
2. For each component whose parent directory has `FS_CASEFOLD_FL` (`ioctl(FS_IOC_GETFLAGS)`), replace the component by
   the entry name that `getdents64` of the parent returns for the component's inode (`d_ino` equal to the component's
   `statx` inode, confirmed by its handle digest, [OS/project §3.1]): the on-disk spelling.
3. The root's `OsFileId` from `statx` and `name_to_handle_at` on the opened directory.

### 4.3 macOS (port)

1. `open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC)`; `fcntl(F_GETPATH)` gives the firmlinked form (`/tmp/x` becomes
   `/private/tmp/x`).
2. Replace each component by its on-disk name from `getattrlist(ATTR_CMN_NAME)` on the path prefix up to that component
   (the method of Apple's `realpath(3)`).
3. The root's `OsFileId` from `getattrlist(ATTR_CMN_FILEID)` and `ATTR_VOL_UUID`.

### 4.4 Lookup by id first

Given a `CanonicalRoot` `c` and the stored rows of `TREES` or `HEADS` bindings ([F11]): if `c.root_id.kind ≠ 0` and some
row's root `OsFileId` is the same object as `c.root_id` (whole-id identity, [OS/project §3.2]), that row is the tree,
whatever its spelling; otherwise the row whose key equals `c`'s key (§4.5) is; otherwise the tree is new. Two rows with
one root id are one tree, and a second binding of it is refused (I-F12). A row written under another OS tag has an
uninterpretable id and is found by spelling only ([80] X1).

### 4.5 Keys derived from canonical paths

| Key | Value | Consumer |
|---|---|---|
| `TREES` key | `blake3_16(c.text bytes)` | [F11] `TREES` |
| `HEADS` directory-binding key and client-head key | `blake3_16(canonical directory text bytes)`, the directory canonicalised by §4 | [AR §5a.1], [F11] `HEADS` |
| `git.worktree` provenance string | `c.text` | [F06] commit header, image trailers ([F14]) |

`blake3_16` is the first 16 bytes of BLAKE3-256 ([F01]).

## 5. Machine-local absolute paths (P12)

`canonical_abs(path) → AbsPath`:

1. **The path exists:** canonicalise it as §4 (Windows: `CreateFileW` with `FILE_FLAG_BACKUP_SEMANTICS`, which opens files
   and directories; Linux: `open(O_PATH | O_CLOEXEC)`; macOS: `open(O_RDONLY | O_CLOEXEC)`), then steps 3–4 of §4.1 (or
   §4.2/§4.3 steps 1–2). Links on the path are followed; the result names the final object.
2. **It does not exist:** make it absolute against the process's current directory (Windows `GetFullPathNameW`; Unix: the
   canonical current directory by §4, then `/` and the argument), then normalise lexically: split on `/` (and `\` on
   Windows), drop empty and `.` segments, let `..` remove the previous segment (never the drive, the `//server/share`
   prefix or the leading `/`), and join with `/`; upper-case the drive letter.
3. The result must match §2.2; otherwise the value is refused (exit 2 at the CLI).

Lexical `..` may differ from the physical parent when a symlink precedes it; for a path that does not exist this is the
only possible reading, and the value is only ever existence-checked.

## 6. OS path construction and entry names (P10)

| | Windows (built) | Linux (port) | macOS (port) |
|---|---|---|---|
| Opening `root` + `rel` | the wide string `\\?\` + root text with `/`→`\` (UNC: `\\?\UNC\server\share\…`) + `\` + `rel` with `/`→`\`, UTF-8 → UTF-16; passed to `CreateFileW` and friends; no `MAX_PATH` limit, and no Win32 name normalisation (trailing dots and spaces, device names, `:` streams) happens, which is why `ProjectFs` tests every segment with §8.1 first ([OS/project §2.3]) | `openat2(root_fd, rel, {flags, resolve: RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS})` (Linux ≥ 5.6) for opens; `fstatat`/`statx(root_fd, rel, AT_SYMLINK_NOFOLLOW)` for stats | `openat(root_fd, rel, flags \| O_NOFOLLOW_ANY)`; `fstatat(root_fd, rel, AT_SYMLINK_NOFOLLOW)`; `getattrlistat` |
| Component limit | 255 UTF-16 code units (NTFS) | 255 bytes (`NAME_MAX`) | 255 UTF-8 bytes (APFS) |
| OS name → `EntryName` | UTF-16 → UTF-8 if valid and P4 holds, else `Unrepresentable(WTF-8 bytes)` | bytes → `Utf8` if valid UTF-8 and P4 holds, else `Unrepresentable(bytes)` | as Linux (APFS stores valid UTF-8 only) |

- A `\\?\` path follows reparse points on intermediate components. Enumeration-based walks never descend into an entry of
  kind `Symlink` or `Other`, `locate_id` results are containment-checked, and `read_for_hash` checks the opened object's
  final path ([OS/project §5.5]), so resolution never follows a link out of the root ([40 §2.4]).
- A Windows root is held as its text and root id, not as an open directory handle: an open handle on a directory makes
  renames of that directory's ancestors fail with error 5 ([40 §4.7]), so no project-tree handle outlives one operation
  ([OS/project §2.2]).

## 7. The CLI boundary

`cli_path(arg, cwd, tree) → RelPath` turns a path argument into a root-relative path:

1. **Decode.** Windows: the argument's UTF-16 must be valid (else exit 2); Unix: its bytes must be valid UTF-8 (else
   exit 2).
2. **Separators.** Windows: every `\` becomes `/`. Unix: a `\` stays a name character, so step 5 refuses it (P4).
3. **Absolute or relative.** Windows: `X:/…` and `//…` are absolute; a leading single `/` means the root of the current
   directory's drive; a drive-relative `X:rel` (a drive letter and `:` not followed by `/`; a bare `X:` is drive-relative
   too) and the device forms `//./…` and `//?/…` (from `\\.\…` and `\\?\…` after step 2) are refused with exit 2
   `bad_path` (`rule` `drive-relative` or `device`, [F19 §10.2]; `PathError::DriveRelative` or `PathError::DevicePath`,
   §11), because their meaning depends on a per-drive current directory or bypasses Win32 name handling (pass 1, P1-37);
   anything else is relative. Unix: a leading `/` is absolute.
4. **Join and normalise.** A relative argument is joined to the canonical current directory (§4 applied to `cwd`); the
   result is normalised lexically as in §5 step 2.
5. **Strip the root.** The tree's canonical text followed by `/` must be a byte prefix of the result (the tree root itself
   gives the empty `RelPath`); otherwise the argument is outside the tree (exit 2, or the verb's own refusal). The rest
   must be a valid `RelPath` (§2.1); a P4 failure is exit 2.

The user's spelling of the components after the current directory is kept; mapping it to git's spelling (P2) or to the
on-disk spelling is the link layer's job.

## 8. Representability and portability

### 8.1 Representable on this OS

`representable(os, segment) → bool` decides whether the OS `os` can hold a name, which is what renders `missing (not
representable on this OS)` for a tracked path ([80 §2.10] P5, [F18] R-16). It is a pure `moirai-files` function beside
P5 that uses P5's device list (§8.2), so target-independent code (FL-1's resolver, [F20 §4.9]) calls it with the OS tag
of the process ([OS/proc §2]) and its Linux and macOS rows are tested on Windows. `moirai-os::path::representable_here
(segment)` is `representable(<the build OS>, segment)` restated in `moirai-os`, which may not depend on `moirai-files`
([OS/README §2.2]). On Windows every `ProjectFs` method applies it to every segment of every path before any OS call
and returns `InvalidName` for a failing one ([OS/project §2.3]), and [F20 §4.9] applies `representable` before any
cascade step; `--allow-nonportable` never relaxes either (pass 1, P1-15). The two copies are one rule: each crate's tests
check the same cases, namely every device name of §8.2 bare, with an extension and with trailing spaces before the
extension, in two ASCII cases; a name ending in `.` and one ending in ` `; each reserved character; and a name of 255 and
one of 256 UTF-16 code units, one of them built from a supplementary-plane character (two units each).

| OS | A segment is representable iff |
|---|---|
| Windows | it is not a Windows device name (§8.2); does not end in `.` or ` `; contains none of `<`, `>`, `:`, `"`, `\|`, `?`, `*`; and is at most 255 UTF-16 code units |
| Linux | it is at most 255 bytes |
| macOS | it is at most 255 UTF-8 bytes |

(`/`, `\`, U+0000 and C0 controls are excluded by P1 and P4 before this check.)

### 8.2 Portable (P5)

`portable_issues(segment, siblings) → set of issues` reports each of the issues below. `siblings` are the names the
segment's directory holds **after** the operation, other than the segment itself (it is never its own sibling). A move's
source name is therefore not a sibling when source and destination share the directory: the case-only rename
`file mv a.md A.md` has no `fold-sibling` issue, because `a.md` no longer exists once `A.md` does.

| Issue | Condition |
|---|---|
| `device-name` | the segment's stem — the part before its first `.`, with trailing ASCII spaces removed — equals, ignoring ASCII case, one of `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0`–`COM9`, `LPT0`–`LPT9`, or `COM` or `LPT` followed by one of the superscript digits `¹`, `²`, `³` (U+00B9, U+00B2, U+00B3), which Windows also reserves (pass 1, A1-60) |
| `trailing-dot-or-space` | the segment ends in `.` (U+002E) or ` ` (U+0020) |
| `reserved-char` | the segment contains any of `<`, `>`, `:`, `"`, `\|`, `?`, `*`; the message cites the first such character of the segment |
| `too-long` | the segment is longer than 255 UTF-8 bytes |
| `fold-sibling` | some sibling differs from the segment but is equal to it under `fold_v1` ([F20]); the message cites the smallest such sibling in byte order ([F01 §6.6] path order) |

`file mv` refuses to create a name with any issue under `files.portable-names = refuse` (the default) unless
`--allow-nonportable` is given; `link`, `file add` and `file mv` under `warn` print one warning per issue ([AR §13]).
Paths already in git stay linkable.

## 9. Display of names

A name is displayed as its UTF-8 bytes; each byte of an ill-formed sequence (the bytes of an `Unrepresentable` name that do
not form valid UTF-8, including a Windows unpaired surrogate's WTF-8 bytes) is written as `\x` followed by two
lower-case hex digits. Further escaping of control characters and quotes inside rendered text is [F19]'s untrusted-text
rule.

## 10. The user-scope configuration file (X-F11)

The location is frozen ([80] X-F11, [80 §2.12]); its keys are [CFG]'s. `user_config_path() → Option<AbsPath>`:

| OS | Location | Resolution |
|---|---|---|
| Windows | `%APPDATA%\moirai\config` | the environment variable `APPDATA`; it must be a non-empty absolute path; the result is `canonical_abs` of `APPDATA` + `\moirai\config` (the file need not exist). `APPDATA` unset or relative → `None`: no user-scope configuration, and `doctor` warns |
| Linux, macOS | `$XDG_CONFIG_HOME/moirai/config`, default `~/.config/moirai/config` | `XDG_CONFIG_HOME` if set, non-empty and absolute (the XDG rule ignores a relative value); else `$HOME/.config`; `HOME` unset → the home directory of `getpwuid_r(geteuid())`; none → `None` |

The Windows resolution reads the environment rather than `SHGetKnownFolderPath`, which would load `shell32.dll` into every
process (open point 6).

## 11. The Rust surface

In `moirai-vfs` (pure):

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct RelPath<'a>(&'a str);    // a Copy view over validated text, passed by value; §2.1; one type for store and project paths
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct RelPathBuf(Box<str>);    // owned; its Eq, Ord and Hash are those of its text
pub struct AbsPath(Box<str>);       // §2.2
pub struct CanonicalRoot { pub text: AbsPath, pub root_id: OsFileId, pub os: OsTag }   // §2.3
pub enum EntryName { Utf8(Box<str>), Unrepresentable(Box<[u8]>) }                        // §2.4
pub enum EntryNameRef<'a> { Utf8(&'a str), Unrepresentable(&'a [u8]) }                   // §2.4, borrowed

impl<'a> RelPath<'a> {
    pub const ROOT: RelPath<'static>;                                  // the empty path
    pub const fn new(s: &'a str) -> Result<RelPath<'a>, PathError>;   // §2.1 grammar
    pub const fn as_str(&self) -> &'a str;
    pub fn segments(&self) -> impl Iterator<Item = &'a str>;
    pub fn parent(&self) -> Option<RelPath<'a>>;                       // None for the root
    pub fn file_name(&self) -> Option<&'a str>;
    pub fn join(&self, seg: &str) -> Result<RelPathBuf, PathError>;
    pub fn to_buf(&self) -> RelPathBuf;
}
impl RelPathBuf {
    pub fn new(s: &str) -> Result<RelPathBuf, PathError>;             // §2.1 grammar
    pub fn as_rel_path(&self) -> RelPath<'_>;
    pub fn as_str(&self) -> &str;
}
// RelPathBuf: Borrow<str> + AsRef<str> + PartialEq<RelPath<'_>>;  RelPath<'_>: AsRef<str> + PartialEq<RelPathBuf>;
// From<RelPath<'_>> for RelPathBuf;  From<&'a RelPathBuf> for RelPath<'a>.
impl AbsPath { pub fn new(s: &str) -> Result<AbsPath, PathError>; }                   // §2.2 grammar
pub fn display_name(bytes: &[u8]) -> String;                                          // §9

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PathError {
    Empty, BadSegment, DotSegment, Separator, Control, Backslash, NotUtf8, NotAbsolute, DriveRelative, DevicePath,
    OutsideRoot,
}
```

`Borrow<str>` is sound because `RelPathBuf`'s `Eq`, `Ord` and `Hash` are those of its text. Every seam takes the view by
value: [OS/fs §3] `rel: RelPath<'_>` and `dir: Option<RelPath<'_>>`, [OS/fs §2.4]
`GroupMember::Dir { dir: Option<RelPath<'a>> }`, [OS/map §3] `name: RelPath<'_>`, and [OS/project §2.1]
`At<'a, R> { root: &'a R, path: RelPath<'a> }`.

**`PathError`.** Each variant has one meaning; the last column is how the CLI reports it ([F19 §10.2]):

| Variant | Meaning | Reported as |
|---|---|---|
| `Empty` | an empty segment: a leading or trailing `/`, or a `//` inside a `RelPath` or inside an `AbsPath` after its prefix; or an empty `seg` passed to `join`. The empty `RelPath` itself is the root and valid | `bad_path`, P1 |
| `BadSegment` | a malformed `AbsPath` prefix (§2.2): a drive letter that is not upper-case `A`–`Z` or is not followed by `:/` (so `C:` and `C:x` are not `AbsPath` values), or a UNC path without a server or a share | exit 2 (§5 step 3) |
| `DotSegment` | a segment that is exactly `.` or `..` (P1) | `bad_path`, P1 |
| `Separator` | a `/` inside a single segment (the argument of `join`) | a programming error of the caller |
| `Control` | a C0 control character in a `RelPath` segment (P4), or U+0000 in an `AbsPath` | `bad_path`, P4 |
| `Backslash` | a `\` in a `RelPath` segment (P4) | `bad_path`, P4 |
| `NotUtf8` | input that is not valid Unicode: bytes that are not UTF-8 (Unix), or UTF-16 with an unpaired surrogate (Windows) (P4; §7 step 1) | `bad_path`, P4 |
| `NotAbsolute` | a value that must be an `AbsPath` matches none of §2.2's three forms; or a CLI argument could not be made absolute at §7 step 4 (the current directory has no canonical form, a leading `/` under a UNC current directory, or a `//` argument without a server or a share) | exit 2 (§5 step 3, §7) |
| `DriveRelative` | a Windows CLI argument `X:rel`, or a bare `X:` (§7 step 3) | `bad_path`, rule `drive-relative` |
| `DevicePath` | a Windows CLI argument in a device form `//./…` or `//?/…` (§7 step 3) | `bad_path`, rule `device` |
| `OutsideRoot` | a CLI argument whose normalised path is neither the tree root nor below it (§7 step 5) | exit 2, or the verb's own refusal |

In `moirai-os::path` (Windows built; reached by generic code through `ProjectFs`, [OS/project §2.1]):

```rust
pub fn canonical_root(dir: &std::path::Path) -> Result<CanonicalRoot, VfsError>;       // §4
pub fn canonical_abs(p: &std::path::Path) -> Result<AbsPath, VfsError>;                // §5
pub fn cli_path(arg: &std::ffi::OsStr, cwd: &std::path::Path, tree: &CanonicalRoot) -> Result<RelPathBuf, PathError>; // §7
pub fn representable_here(segment: &str) -> bool;                                      // §8.1: representable(<build OS>, segment)
pub fn user_config_path() -> Option<AbsPath>;                                          // §10
```

`portable_issues`, `representable(os: OsTag, segment: &str) -> bool` (§8.1) and `fold_v1` are `moirai-files` functions
([F20]).

## Coverage

The rows of `COVERAGE.md` that cite this file ([F01 §2.7]).

| Item | Part covered here | Section |
|---|---|---|
| `60-AR-Layout-userconf` (the per-OS user-scope configuration locations) | the lookup per OS; the locations are frozen by [F02 §7] | §10 |
| `60-AU-CrossPlatform` (the "Cross-platform" summary row) | X-F7 and X-F9 (a), whose parts are the rows `X-F7` and `X-F9` | — |
| `R-16` ([40] R-16: the frozen strings) | when a path is unrepresentable; representability. The strings are [F18 §4]'s | §2.4, §8.1 |
| `X-F7` ([80] X-F7) | the path value types `RelPath`, `AbsPath`, `CanonicalRoot`, `EntryName`; the rules P1–P12 (P3 NFC); canonical roots (P9); the `abs` form (P12); OS path construction and entry names (P10); the CLI boundary. `fold_v1` is [F20 §3.1]'s, the stored keys [F08] and [F11]'s | §2, §3, §4, §5, §6, §7 |
| `X-F9` ([80] X-F9) | P11; the query file name is [F14 §7.2.1]'s, the ref-name grammar [F12 §2]'s | §3 |
| `X-F11` ([80] X-F11) | the lookup; the locations are [F02 §7]'s, the keys [CFG]'s | §10 |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none: every rule here is fixed by [80 §2.10] or by this file's resolutions | — | — | — |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | One `RelPath` for store and project paths, or two? | resolved with [OS/README §2.1] and [OS/fs §2.1]: one type with §2.1's grammar, defined here; `os::fs` adds use-time checks only | — |
| 2 | P3 says `NFC(name)` "as git records it"; git on macOS precomposes with `iconv` from `UTF-8-MAC`, which is Apple's variant of NFC, not Unicode 17.0.0 NFC | P3's function is **git's precomposition** (the port reproduces it exactly and tests it against git on APFS); X-F7's intent (the same bytes as git) wins over the letter "NFC" | R-REV-P, port phase |
| 3 | P5's device list omits Windows' superscript-digit forms (`COM¹`–`COM³`, `LPT¹`–`LPT³`) and `CONIN$`/`CONOUT$` | **added (pass 1, A1-60)** to §8.2: P5's condition is "a name some supported OS cannot hold", and Windows reserves these names too, so the list is completed rather than changed. P11 (b) uses the same list; ref-name segments cannot contain `$` or superscripts anyway ([F12 §2]) | — |
| 4 | "with any extension" and the handling of spaces before the extension are not specified in P5 or P11 (b) | the stem is the part before the first `.` with trailing ASCII spaces removed (§8.2), matching Windows' Win32 name normalisation | R-REV-P, WP-12 |
| 5 | `\xNN` does not say upper or lower case | lower case (§9); [F19] adopts it with the output contract | WP-18 |
| 6 | X-F11 on Windows names `%APPDATA%`; the documented alternative `SHGetKnownFolderPath` loads `shell32.dll` | the environment variable, with `None` and a `doctor` warning when it is unset (§10) | R-REV-P |
| 7 | `\\?\` opens follow reparse points on intermediate components, while [40 §2.4] says resolution never follows a link out of the root | walks never descend into links; `locate_id` and `read_for_hash` check containment ([OS/project §5.5]); Linux and macOS use `RESOLVE_NO_SYMLINKS` and `O_NOFOLLOW_ANY` | R-REV-P |
| 8 | [OS/README §1.3] assigns X-F9 to this file | P11 (a) and (b) are stated here as rules with their byte precision; the image file name belongs to [F14] and the ref-name validator to [F12], which cite §3 | WP-12, WP-15 |
| 9 | `RelPath(str)` cannot be built from a `&str` without `unsafe`, which `moirai-vfs` forbids (WP-30 review) | **closed (spec sync 2a):** `RelPath<'a>(&'a str)` is a `Copy` view passed by value in every seam; `RelPathBuf: Borrow<str> + AsRef<str> + PartialEq<RelPath<'_>>` (§2.1, §11); [OS/fs §3], [OS/map §3] and [OS/project §2.1] follow | — |
| 10 | `PathError`'s variants had no stated meaning, and the drive-relative and device refusals of §7 step 3 had no variant of their own (WP-30, WP-33) | **closed (spec sync 2a):** §11's table gives every variant one meaning; `DriveRelative` (a bare `X:` included) and `DevicePath` are added | — |
| 11 | §8.2 did not say whether a move's source counts as a sibling, nor which sibling or character a message cites (WP-61 review) | **closed (spec sync 2a):** siblings are the directory's names after the operation; `fold-sibling` cites the smallest sibling in byte order, `reserved-char` the first reserved character | — |
| 12 | `representable_here` existed only in `moirai-os`, with its own copy of the device list: the Linux and macOS rows could not be tested on Windows and target-independent code could not call it (WP-61 review) | **closed (spec sync 2a):** a pure `representable(os, segment)` lives in `moirai-files` beside P5; `representable_here` is its build-OS restatement in `moirai-os`, and both crates test §8.1's common cases. [F18 §4.6] detail 44 and [F20] may cite `representable` at their next edit | R-SPEC-F, R-SPEC-R |
