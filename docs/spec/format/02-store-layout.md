# 02 — Store layout

| | |
|---|---|
| Title | Store layout: placement, discovery, the pointer file, the store id, the contents of the store directory, store file names (X-F10) and the user-scope configuration locations (X-F11) |
| Chapter | [F02], `docs/spec/format/02-store-layout.md` |
| Status | draft, pass 1 pending |
| Work package | WP-10 (R-SPEC-F), [60 §3.1] item 1 |
| Sources | [AR §2.14] (discovery, placement CM6, `init` guard D4, `doctor store`); [AR §2.13] (scope); [AR §4.1] (files table and rules, file count CL3); [AR §4.9] (delta files, retirement, GC of `cs`, `tmp/` and `trash/`); [AR §4.10] (namespace durability, `restore` swap); [AR §7.1] (`init`, `backup`, `restore`, `doctor` flags; exit codes); [AR §11] #17 (one store per repository), #36, #37; [AR §13] (two scopes; `discovery.git-hint`; user-scope keys); [AR §14]; [80 §2.3.2] (durability class per protocol point), [80 §2.5] (sealed files), [80 §2.6] (environment guard), [80 §2.10] P1, P9, P11, P12, [80 §2.12] (temporary files, user-scope config, `config` rewrite), [80 §3.1] X-F10, X-F11, [80 §3.2] (pointer files), [80 §4.2] T5; [90 §2.1] (`MOIRAI_DIR`), [90 §2.2] (the MCP server's store discovery), [90 §4.5] (lazy open); [H21 §2.1] (the Codex MCP server's environment); [40 §3.5] (`trash/`); [60 §2.5] "Store layout" row and the Cross-platform row; [PLAN §3.2] WP-10, [PLAN §3.3] (pointer-file encoding) |
| Depends on | [F01]; cites [F03], [F04], [F05], [F09], [F10], [F11], [F14], [F15], [F16], [F17], [F18], [F19], [CFG], `[OS/fs]`, `[OS/env]`, `[OS/path]` |

## 1. Scope

This chapter specifies where a store lives, how every process finds it, the pointer file, the store id, which entries
the store directory holds and how each is named, and where the user-scope configuration file lives on each OS. The byte
layouts of the store's files are in their own chapters (§5.1 names each). The `config` syntax, precedence, unknown-key
rule and registry format — the remaining content of [60 §2.5]'s "Store layout" row — are [CFG]'s.

**Terms.**
- **Store directory**: the directory that holds one store's files (§5). A store is one store directory.
- **Repository, worktree, common dir**: git's terms. A *main worktree* has a `.git` directory; a *linked worktree* has a
  `.git` file. `<git-common-dir>` is the directory git shares among a repository's worktrees.
- **Store id**: the store's 16-byte identity (§4).

## 2. Placement

### 2.1 Where `moirai init` creates a store

`moirai init [--here | --link STORE] [--default-branch main] [--force --shadow]` ([AR §7.1]) chooses the location by
where it runs ([AR §2.14], owner decision #17: one store per repository).

| Where `init` runs | Flag | Result |
|---|---|---|
| inside a git repository (any worktree) | none | creates `<git-common-dir>/moirai/`: outside every working tree, so `git add -A` never sweeps store files into a commit, and shared by every worktree of the repository through the git hint (§3.4) with no pointer files |
| the top-level directory of the main worktree | `--here` | creates `<main-worktree>/.moirai/` and adds `.moirai/` to `<git-common-dir>/info/exclude` (§2.3); linked worktrees find it through the hint's second probe (§3.4) |
| a linked worktree | `--here` | refused, naming `<git-common-dir>/moirai/`: a store per worktree is refused ([AR §2.14]) |
| a subdirectory of the main worktree | `--here` | refused, naming the main worktree's top-level (open point 11) |
| outside any git repository | none, or `--here` | creates `./.moirai/`; subdirectories reach it by the walk-up of §3.1 step 3, other directories through pointer files |
| any directory | `--link STORE` | creates no store; writes a pointer file `./.moirai` naming STORE (§3.3) |

- "Inside a git repository" means that the walk of §3.1 step 3, started in `init`'s working directory, finds a `.git`
  entry and that §3.4 resolves its common dir. A `.git` entry whose common dir does not resolve makes `init` refuse,
  naming the entry; `init` never treats a broken repository as "outside any repository".
- `init` creates a store only at the locations of this table. A `--store` flag or `MOIRAI_DIR` does not choose where
  `init` creates (open point 12).
- `init` refuses when the target path already exists as any entry.
- `--default-branch` sets the store-scope key `default-branch` ([AR §13]) in the new store's `config`.

### 2.2 The shadow guard (D4)

Before it creates anything, `init` runs discovery steps 3 and 4 (§3.1) from its working directory. If either step
finds an existing store, `init` refuses unless both `--force` and `--shadow` are given, and names the store it would
shadow. If either step finds an entry that is not a store (§3.2) or a stale or malformed pointer file (§3.3), `init`
refuses, and `--force --shadow` does not override that. The exit codes of these refusals are [F19]'s (open point 13).

### 2.3 The `info/exclude` line

`init --here` inside a repository makes `<git-common-dir>/info/exclude` contain the line `.moirai/`:
- if the file does not exist, `init` creates `<git-common-dir>/info/` if needed and a file containing the 9 bytes
  `.moirai/` LF;
- if the file already contains a line equal to `.moirai/` or `/.moirai/` (compared after removing a trailing CR),
  `init` changes nothing;
- otherwise `init` appends `.moirai/` LF, preceded by one LF if the file's last byte is not LF.

`init` never writes to a tracked `.gitignore` ([AR §2.14]).

### 2.4 What `init` creates

- Before it creates any store file, `init` runs the full environment probe at the target location ([80 §2.6],
  `[OS/env]`): classification, a durable write, `sync_dir` and the lock probe. A refused location refuses `init` with
  exit 7 naming the file system, and `init` removes whatever the probe created, the new directory included
  (`[OS/fs §4.2]` `remove_dir`).
- `HEAD` is created last of all (§5.5). The other initial entries proposed here are `LOCK` (§5.1), `config`, the first
  log extent `log.1` and the directory `tmp/`. [F16] fixes the initial set, the sequence and the durability class of
  each step, and [F04] the initial `HEAD` contents (open point 14).
- On Windows `init` sets the owner access-control entry on the store directory that files created under it inherit
  ([80 §2.6]); `create_root` of `[OS/fs §4.1]` does it.
- `init --link` writes only the pointer file (§3.3).

### 2.5 One kernel, admitted file systems

- A store is used only on a file system its OS's allow-list admits ([80 §2.6]); every process runs the cheap
  classification at every open. In M0–M11 only NTFS is admitted on Windows.
- A store is touched by one kernel only: byte-range locks do not cross a kernel boundary ([AR §4.1], [80 §2.6]).
- `trash/` is used only when the project file and the store are on the same volume (§5.4).

## 3. Discovery

### 3.1 The CLI's chain

Every CLI process, command hook and `moirai gc` child finds its store with this chain. The first step that decides
wins. Discovery writes nothing, and a miss never creates a store ([AR §2.14]).

| Step | Source | Rule |
|---|---|---|
| 1 | `--store DIR` | DIR must be a store directory (§3.2). If it is not, exit 7 naming DIR. Discovery does not continue to step 2 |
| 2 | `MOIRAI_DIR`, when set and not empty ([90 §2.1]) | as step 1. An empty value counts as unset |
| 3 | walk-up | for each directory D from the process's working directory up to the root of its path, D itself first: if `D/.moirai` exists, it decides (§3.2): a store directory is found; a valid pointer file finds its target; anything else is exit 7. A `.git` entry does not stop the walk |
| 4 | the git hint, when the user-scope key `discovery.git-hint` is true (default `true`; environment `MOIRAI_GIT_HINT`, [AR §13]) | §3.4 |
| 5 | none found | exit 7, naming every path steps 1–4 examined, the hinted store if there was one, and `moirai init --link <store>` ([AR §2.14]); the text is [F19]'s |

- A relative DIR (steps 1 and 2) is resolved against the working directory. On Windows, DIR may use `\` or `/` ([80
  §2.10], CLI boundary).
- During step 3 the walk records the nearest directory that contains an entry named `.git` (the first one met from the
  working directory upward). Step 4 uses it.
- A probe that fails because access is denied counts as "not found" at that level. The failure is listed in the exit-7
  text and in `doctor store` (§3.7).
- `discovery.git-hint` is a user-scope key because a store-scope key cannot govern the discovery that finds the store
  ([AR §2.14], [74 A10]). `MOIRAI_GIT_HINT` outranks the user file ([AR §13] precedence); its accepted spellings are
  [CFG]'s boolean syntax.
- The CLI's `--tree DIR` selects the tree a read uses ([AR §7.1], [90 §4.1]); it does not take part in store discovery
  (open point 10).

### 3.2 What a store directory is; how a `.moirai` entry is classified

- A **store directory** is a directory that contains an entry named `HEAD` that is a regular file. `init` creates `HEAD`
  last (§5.5), so a directory with `HEAD` is complete. `HEAD`'s name is durable only once `init`'s rename of
  `tmp/head.<nonce>` has been followed by `durable-name` on both parents, so a discovering process can open a store whose
  `HEAD` a crash would still lose; before it first acknowledges a durable effect through the store, every process runs
  `durable-name` on `tmp/` and on the store directory, once per opening, holding no role byte ([F16] P-88, "The window
  after step 6's rename"; spec sync 2b).
- Discovery examines `.moirai` and every probe path by the OS's ordinary lookup, which follows symbolic links.

| The entry is | Result |
|---|---|
| a directory that contains `HEAD` | a store: discovery ends with this store directory |
| a regular file | a pointer file: parsed and checked (§3.3). A valid pointer ends discovery with its target store. A malformed or stale pointer ends discovery with exit 7: it is reported, never followed ([AR §2.14]), and the walk does not continue |
| a directory without `HEAD`, or an entry of any other type | exit 7: "not a store (initialisation in progress, or damaged)", naming the path. The walk does not continue |

Stopping instead of continuing keeps a damaged or stale entry from silently selecting another store further up (open
point 8).

**An interrupted or running `restore` swap.** `restore` exchanges the store directory `P/<a>` with the restored copy
through `swap_dirs`, which on Windows (and wherever no atomic exchange exists) renames under the guard of the intent
file `P/<a>.swap`, keeping the old store at `P/<a>.swap-old` in between (`[OS/fs §4.9]`, [80 §2.3.2]). The probes of
steps 1, 2 and 4 (probes A and B of §3.4), and the same steps of §3.5, therefore also look for `P/<a>.swap` when
`P/<a>` is not a store directory:
- if `P/<a>` is a store directory, discovery proceeds normally, whether or not the intent exists (a reader of the old
  store is sent back to discovery by `HEAD.retired`, §3.6);
- if `P/<a>` is absent or not a store and `P/<a>.swap` exists, discovery retries the probe as [F16] specifies and, if the
  intent is still there, ends with exit 7 naming the intent file and `moirai doctor`, which completes or rolls back the
  swap (`swap_recover`, `[OS/fs §4.9.4]`).

Discovery never runs `swap_recover` itself (open point 17). The walk of step 3 does not look for `.moirai.swap` at
every level, which would double its probes on the open path ([AR §4.7]). A `.moirai` store inside a repository is still
covered, because the walk then reaches the hint, whose probe B checks `<main-worktree>/.moirai.swap`. The residual case
is a store outside any repository that is being swapped while a second store further up shadows it; the walk then
finds the upper store, and `doctor store` already reports that configuration as shadowing (§3.7).

### 3.3 The pointer file

A **pointer file** is a regular file named `.moirai` that names a store directory elsewhere. `moirai init --link STORE`
writes it ([AR §2.14]). Pointer files are machine-local, like bindings ([80 §3.2]): never versioned, exported or copied
into a store.

**Grammar** ([RFC 5234] ABNF, with [RFC 7405]'s `%s` for case-sensitive strings):

```abnf
pointer-file = [BOM] dir-line id-line
dir-line     = %s"moiraidir: " store-path EOL
id-line      = %s"store-id: " 32LHEX [EOL]
EOL          = LF / CR LF
BOM          = %xEF.BB.BF
LHEX         = DIGIT / %x61-66                 ; 0-9 a-f
store-path   = 1*PCHAR                         ; UTF-8; rules below
PCHAR        = %x20-7E / UTF8-NONASCII         ; no C0 control, no DEL
UTF8-NONASCII = <a UTF-8 encoded scalar value above U+007F, [RFC 3629]>
```

**Rules.**
1. **Reading** follows [F01 §6.7]: one leading byte-order mark is skipped, CR LF is accepted, and the line end after
   `id-line` may be missing. Nothing may follow `id-line` and its line end: an extra line, even an empty one, makes the
   file malformed. The file is at most 4,096 bytes; a larger file is malformed.
2. **Writing.** `init --link` writes no byte-order mark, ends both lines with LF, and writes nothing else.
3. **`store-path`** names the store directory itself, never another pointer file. It uses `/` as the separator.
   - An **absolute** path has the form of [80 §2.10] P12: on Windows `X:/…` (written with the drive letter
     upper-cased; either case is accepted on read) or `//server/share/…`; on Linux and macOS a leading `/`. A path
     beginning `//?/` or `\\?\` is malformed.
   - A **relative** path is resolved against the directory that contains the pointer file. It may contain `..`
     segments; `init --link` writes no empty and no `.` segment and no trailing `/`.
   - On Windows a `\` read in `store-path` is a separator. On Linux and macOS `\` is an ordinary name byte.
   - `init --link` writes a relative path when the pointer's directory and the store directory lie under the same root
     — the same drive letter or UNC share on Windows, always on Linux and macOS — computed lexically between their
     canonical absolute paths ([80 §2.10] P9); otherwise an absolute path ([80 §3.2]).
4. **`store-id`** is the target store's id (§4) in lower-case hexadecimal ([F01 §6.4]).
5. **Checking a pointer during discovery**, in order:
   1. the file must match the grammar and rule 1, else it is **malformed**;
   2. `store-path` is resolved; the result must be a store directory (§3.2) and not a pointer file, else the pointer is
      **stale**;
   3. the id recorded in the target's `HEAD` must equal `store-id`, else the pointer is **stale**.
   A malformed or stale pointer ends discovery with exit 7. The text names the pointer file, the resolved target, the
   recorded and the found store ids (where they exist), and the fix: remove the pointer file and run
   `moirai init --link <store>`.
6. **Creating.** `init --link` refuses when `./.moirai` already exists as any entry. It creates the file with
   create-new semantics, writes its bytes in one write, and makes it durable (`durable+meta` on the file,
   `durable-name` on its directory, [80 §2.3.1]) before it reports success (open point 1). A pointer file left torn by
   a crash is malformed, and discovery reports it. This durability point is a protocol point of [F16] with a seeded bug
   ([F16] P-99: success reported before `durable-name`; pass 1, S1-41).

*(Informative)* A pointer file in `D:/scratch/notes` for the store `D:/work/demo/.moirai` with the synthetic store id
`0123456789abcdef0123456789abcdef` is these 78 bytes:

```
moiraidir: ../../work/demo/.moirai␊
store-id: 0123456789abcdef0123456789abcdef␊
```

(`␊` marks the byte `0A`. The first line is 11 + 23 + 1 = 35 bytes, the second 10 + 32 + 1 = 43.)

### 3.4 The git hint

The hint reads git's files textually. It never runs `git` and never links a git library ([AR §2.14], [AR §5c]).

1. **Nearest `.git`.** G is the `.git` entry the walk of §3.1 step 3 recorded. With none, the hint misses.
2. **gitdir.** If G is a directory, `gitdir` = G. If G is a regular file of at most 4,096 bytes whose first line begins
   with the 8 bytes `gitdir: `, then `gitdir` is the rest of that line with trailing space, tab, CR and LF removed; a
   relative value is resolved against the directory that contains G. Otherwise the hint misses.
3. **Common dir.** If `<gitdir>/commondir` is a regular file of at most 4,096 bytes, the common dir is its first line
   with trailing space, tab, CR and LF removed, resolved against `gitdir` when relative. Otherwise the common dir is
   `gitdir`.
4. **Probe A: `<git-common-dir>/moirai`.** A store directory is found. An existing entry that is not a store directory
   is exit 7, as in §3.2. If nothing exists there, go on.
5. **Probe B: `<main-worktree>/.moirai`**, only when the common dir is a directory whose last path component is exactly
   `.git`; `<main-worktree>` is its parent. The entry is classified as in §3.2 (a store, a pointer file, or exit 7). If
   nothing exists there, or the condition does not hold, the hint misses.
6. A miss goes on to step 5 of §3.1.

- Paths read from `.git` and `commondir` must be valid UTF-8; otherwise the hint misses. On Windows both `/` and `\` are
  separators.
- The environment variables `GIT_DIR`, `GIT_COMMON_DIR` and `GIT_WORK_TREE` are not consulted (open point 9).
- *(Informative)* In a linked worktree the `.git` file names `<common>/worktrees/<name>`, whose `commondir` file holds
  `../..`, so probe A finds `<common>/moirai` from every worktree ([AR §2.14], CM6).

### 3.5 The MCP server's chain

`moirai mcp` discovers its store with the chain of [90 §2.2], which extends §3.1 with the call's tree ([90] is normative
for harness rules, [F01 §2.4]):

| Step | Source |
|---|---|
| 1 | `--store` in the server's argv (`moirai integrate` writes it where the harness expands a workspace variable) |
| 2 | `MOIRAI_DIR` in the server's environment |
| 3a | walk-up from the server's working directory, as §3.1 step 3 |
| 3b | walk-up from the call's `tree` parameter, or, without one, Codex's `sandboxCwd` |
| 4 | the git hint (§3.4) from the `.git` nearest to the server's working directory; if that misses, from the `.git` nearest to the call's tree (open point 10) |
| 5 | none found: the call's result is `isError`, with the text of §3.1 step 5 |

- **Lazy.** The handshake and `tools/list` touch no store file ([90 §2.2], [90 §4.5]). The first tool call runs the
  chain. The server then serves that store for its lifetime.
- A later call whose `tree` discovers a different store (steps 3b–4 from that tree) is answered with `isError` naming
  both stores ([90 §2.2]).
- Discovery never reads a harness variable such as `CLAUDE_PROJECT_DIR` ([90 §2.2]).

### 3.6 The result

- Discovery yields the store directory as its canonical absolute path (per component, on-disk names, [80 §2.10] P9)
  and the step that found it. The process then reads the store id from `HEAD`.
- Two discovery results name the same store when their canonical absolute paths are equal.
- A process that sees `HEAD.retired` set ([AR §4.2]) runs discovery again from its original inputs (`restore`'s swap,
  [AR §4.10], [72 m6]). A running `restore` sets `retired` before it creates its swap intent ([F16] P-85 steps 2–3), so
  when that discovery yields the same store again, `retired` still set and no swap intent `P/<a>.swap` beside it (§3.2),
  the process repeats the probe with [F16] P-86's delays; if the same state persists, a `restore` ended without clearing
  the flag ([F16] open point 10), and the process exits 7 `store_retired` ([F19 §10.2]) without using the store. Only
  `doctor` clears the flag.

### 3.7 `doctor store`

`moirai doctor store` reports ([AR §2.14]): every store reachable by every discovery step from the working directory
and from each registered directory binding (`HEADS`, [F11]); shadowing, meaning a walk-up hit that differs from the
hint's store; a store inside a working tree that no ignore rule excludes; a store on a network or OneDrive path; every
malformed or stale pointer file and every non-store `.moirai` entry it meets; every probe denied by access rights; and
two store directories that carry the same store id (a copied store).

## 4. The store id

- The store id is a `b16` ([F01 §5.6]): 16 bytes drawn from the operating system's cryptographically secure random
  source (`Entropy::fill_random`, [OS/README §4.6]) by `init`. `init` never uses the all-zero value (it draws again).
- It is fixed at `init` and constant for the store's life. `backup`, `restore` and `repair` keep it; `restore` and
  `repair` re-roll the epoch instead ([AR §4.2], G25). The epoch is a different value.
- It is recorded in `HEAD` among the parameters fixed at `init` ([F04], [F17 §2]; open point 2), and every process reads
  it from there.
- Its text form is 32 lower-case hexadecimal digits ([F01 §6.4]). It appears in pointer files (§3.3), in the image's side
  ref `refs/moirai/meta/<store-id>` and its `meta.moi`, in `refs/moirai/ops/<store-id>`, and in the alias map's origin
  store ([AR §5b.1], [AR §5b.6] step 5, [F14]).
- It is never part of a canonical form or of hashed image content ([AR §4.6], [AR §5b.5] rule 7).

## 5. The store directory

### 5.1 Entries

Every entry that moirai creates in a store directory is one of these. Names follow §6.

| Entry | Kind | Size and growth | Mapped | Layout |
|---|---|---|---|---|
| `HEAD` | file | 8,192 bytes: two 4 KiB slots; created by `init` and `restore`, never resized | never; read and written at offsets | [F04] |
| `LOCK` | file | 36,864 bytes (36 KiB); created only by `init` and `restore`; never deleted or resized; its lock bytes lie beyond the end of the file ([80] X-F1) | never | [F03] |
| `config` | text file | store-scope keys of [AR §13]; git-config syntax | never | [CFG] |
| `log.<n>` | file | a log extent of exactly `store.log-extent-bytes` ([F17]; the design default is 64 MiB), reading as zero beyond its tail ([80 §2.3.3]); at most `store.log-active-extents` extents ([F17]; 4 in [AR §4.1]) kept as active history before retirement | never; explicit I/O only | [F05] |
| `hist.<n>` | sealed file | a retired log extent as compressed frames with a commit index | read-only | [F10] |
| `seg.base.<G>` | sealed file | the base segment of generation G | read-only | [F09] |
| `seg.d<K>` | sealed file | a delta segment; at most 3 live before a tiered fold ([AR §4.9]) | read-only | [F09] |
| `seg.b<ref_id>.<K>` | sealed file | a promoted branch's segment and its `TOUCH` bitmap; one set per promoted branch | read-only | [F09] |
| `blobs.<n>` | sealed file | bodies, and R-9's fingerprint blobs ([40] R-9); one per delta checkpoint that sealed bodies, merged at folds and rollups | read-only, mapped on first body read | [F10] |
| `dict.<D>` | sealed file | the compression dictionary, present only when `HOLE(F02-dict-file)` says a dictionary exists | read-only | [F10] |
| `gitmap.<n>` | sealed file | pages of the git id map | read-only | [F10] |
| `cs.<n>` | sealed file | the changeset segment of one bulk commit ([AR §4.3]) | read-only, as one more delta layer | [F09] |
| `tmp/` | directory | temporary files (§5.3) | never | this chapter, [F16] |
| `trash/` | directory | files moved aside by `moirai file rm --trash` (§5.4) | never | this chapter, [F18] |

*(Informative)* The stable set is `HEAD`, `LOCK`, `config`, up to 4 log extents, one base segment, up to 3 delta segments
and one dictionary — about a dozen files — plus one `hist` file per retired extent, `blobs` files since the last fold,
`gitmap` pages, bulk-commit `cs` files until folded, promoted-branch segments, and the files pinned by branches and tags
([AR §4.1], CL3).

### 5.2 Rules

These restate [AR §4.1], [AR §4.10] and [80 §2.3.2, §2.5] for the directory as a whole. [F16] owns the protocol steps.

1. **Growth by new files.** A store grows by creating files. No file is truncated, and no file is renamed over while it
   can be mapped. No name is ever reused (§6.2). Reclamation writes a new file and switches `HEAD`.
2. **Sealed files** (`hist`, `seg.*`, `blobs`, `dict`, `gitmap`, `cs`):
   - each is created under its final name, written, made durable (`durable+meta`), sealed — read-only on disk
     (`FILE_ATTRIBUTE_READONLY`, mode `0444`) — and its name made durable (`durable-name` on the store directory),
     before the durable record that names it ([80 §2.3.2]);
   - the one exception is `cs.<n>`: it is built in `tmp/` (§5.3) and moved to its final name by `rename_noreplace`,
     followed by `durable-name` on both `tmp/` and the store directory, before the `Commit` record that names it
     ([AR §4.3], [AR §4.10]);
   - its header states its total length, which is checked against the file's size before it is mapped ([80 §2.5]).
3. **Deletion** happens only after the two-slot `HEAD` barrier, after 60 s of grace, and only when no pin references the
   file ([AR §4.1], [AR §4.2], [AR §4.9]). On Windows, store files are opened with
   `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, delete-pending files are tolerated, and GC clears the
   read-only attribute before deleting ([80 §2.5], [80 §2.12]).
4. **Temporary files** live only in `tmp/`, never in `%TEMP%` or `$TMPDIR` ([80 §2.12]).
5. **The commit path creates no file** other than a new log extent at rotation and a bulk commit's `cs.<n>`. There are
   no per-node and no per-commit files ([AR §4.10]).

### 5.3 `tmp/`

- `init` creates `tmp/`. A process that needs it and finds it missing creates it again; a directory that already exists
  is not an error.
- A temporary file that moirai creates is named `<word>.<nonce>` (§6.3). The nonce is a `u64` drawn from the OS's
  cryptographically secure random source (`Entropy::fill_random`, [OS/README §4.6]) and written in decimal. The file is created with create-new semantics; if the
  name exists, the process draws a new nonce.

| Word | Content | Lives until | Owner |
|---|---|---|---|
| `cs` | a bulk commit's changeset segment under construction | its `rename_noreplace` to `cs.<n>` (§5.2 rule 2) | [F09], [F16] |
| `config` | the next text of the store's `config` | its `rename_replace` onto `config`, then `durable-name` ([80 §2.3.2]) | [CFG], [F16] |
| `head` | the initial `HEAD` written by `init` | its `rename_noreplace` to `HEAD` (§5.5; open point 14) | [F16] |
| `sort` | a spill run of at most 1 MiB for the external sort of a producer whose entries exceed its write budget ([AR §4.1], [AR §4.6], [F07 §10.6]) | the end of the command that made it | [F16] |
| `probe` | a file of the full probe at `init` and `restore` ([OS/env §5]): the durable-write, lock and rename probes, each file named with its own nonce | the end of the probe | [OS/env] |
| `extent` | a spare log extent that maintenance prepares ahead of rotation ([F16] P-96): `create_extent`, `durable+meta`, then `rename_noreplace` onto `log.<n+1>` (pass 1, P1-7) | its rename to `log.<n+1>`, or its deletion when a rotation made that extent first | [F16], [OS/fs §4.5] |

One `tmp/` entry has a fixed name instead of `<word>.<nonce>` (pass 1, P1-32, S1-38, A1-40):

| Name | Content | Lives until | Owner |
|---|---|---|---|
| `settle.stamp` | a 1-byte file whose mtime a settle rewrites at its start: the racy threshold of the Linux and macOS frontier ([OS/project §5.9], [OS/clock §6], [F20 §5.12.1]) | the life of the store; a missing one is created again | [OS/project], [F20] |

- The orphan sweep may remove a `probe.<nonce>` left by a dead process, and may remove `settle.stamp`: a settle writes the
  stamp, creating it when absent, before it reads T0 ([F20 §5.12.1]), so removing it changes no result.

- Files of other names may appear in `tmp/`: a script may put its temporary query file there ([80 §4.2] T5). moirai
  never opens such a file as a store file.
- The orphan sweep removes temporaries left by dead processes ([AR §4.1], [AR §4.9]). The rule that decides when a
  temporary is an orphan is [F16]'s (open point 5).

### 5.4 `trash/`

- `moirai file rm --trash` moves each item of its `FsIntent` to `trash/<intent>/<i>` by `rename_noreplace`, followed by
  `durable-name` on both parents ([40 §3.5], [80 §2.3.2]).
- `<intent>` is the decimal form of the intent id of the `FsIntent` record ([F05], [F11]; open point 7). `<i>` is the
  zero-based position of the item in that record's item list, in decimal. The record keeps each item's original path.
- A moved directory keeps its content. The names below `trash/<intent>/<i>/` are project data, not store file names,
  and §6 does not apply to them.
- `--trash` refuses when the store is on another volume than the item ([40 §3.5]). `moirai gc` purges entries older than
  `gc.trash-expire` (14 days) ([AR §4.9]). Nothing under `trash/` is mapped or read as store data.

### 5.5 Completeness: `HEAD` last

`HEAD` exists in a store directory only when every other entry of the initial set (§2.4) is complete and durable.
Discovery relies on this (§3.2). Proposed mechanism for [F16] (open point 14): `init` writes the initial `HEAD` as
`tmp/head.<nonce>`, makes it durable (`durable+meta`), moves it to `HEAD` by `rename_noreplace`, and then makes both
names durable (`durable-name` on `tmp/` and on the store directory). `restore` builds the restored store completely in
its target directory before the swap ([AR §4.10], [F16]).

### 5.6 Foreign entries

An entry of the store directory whose name does not match the grammar of §6.3 is **foreign**. moirai never opens, maps,
renames or deletes a foreign entry; `doctor` lists it. Case matters: `Log.1` is foreign, even where the file system
would also find it as `log.1`, because moirai creates only the spellings of §6.3.

The orphan sweep ([AR §4.1], [AR §4.9]) removes only entries whose names match §6.3: unreferenced numbered files of the
store directory and moirai's own `tmp/` entries. The rule that decides when such an entry is an orphan is [F16]'s.

## 6. Store file names (X-F10)

### 6.1 The rule

Every name that moirai creates inside a store directory is built only from decimal numbers and the fixed ASCII words of
§6.3 (`HEAD`, `LOCK`, `config`, `log`, `seg`, `base`, …) joined by `.`, and from nothing else: never a ref name, a path,
a user string or hexadecimal text ([80] X-F10, [80 §2.10] P11 (c)). A promoted branch's segment is therefore `seg.b<ref_id>.<K>` with the decimal, never-reused
`ref_id` ([AR §4.1]). So a ref name such as `lane/x` never creates a subdirectory, two ref names equal under `fold_v1`
never collide on NTFS or APFS, and no store file name is a Windows device name or differs from another only by case or
normalisation.

### 6.2 Numbers

- Every number in a name is written in decimal ([F01 §6.5]): no leading zeros and no padding. `NNNN`, `G`, `K` and `D`
  in [AR §4.1] are notation for such a number, not a width (open point 3).
- A **file number** (`<n>`, `<G>`, `<K>`, `<D>`) is a `u32` from 1 to 4,294,967,295. 0 names no file ([F01 §5.8]).
- `<ref_id>` is the `u32` ref id ([F11]), from 0 to 4,294,967,295.
- Each name family (`log.`, `hist.`, `seg.base.`, `seg.d`, `seg.b<ref_id>.` per ref id, `blobs.`, `dict.`, `gitmap.`,
  `cs.`) has its own number space. Numbers in a family only increase, and a number is never used again, not even after
  its file was deleted ([80 §2.5] rule 3, [AR §4.9] "always a new file number, never a delete-pending name"). The
  persistent allocator that guarantees this is [F04]'s and [F16]'s (open point 4).

### 6.3 Grammar

([RFC 5234] ABNF, with [RFC 7405]'s `%s` for case-sensitive strings; a name matches only if its numbers are within
the ranges stated in the comments.)

```abnf
store-entry   = fixed-name / numbered-name / %s"tmp" / %s"trash"
fixed-name    = %s"HEAD" / %s"LOCK" / %s"config"
numbered-name = %s"log." fnum / %s"hist." fnum / %s"blobs." fnum / %s"gitmap." fnum
              / %s"cs." fnum / %s"dict." fnum
              / %s"seg.base." fnum / %s"seg.d" fnum / %s"seg.b" u32dec "." fnum
tmp-entry     = tmp-word "." u64dec / %s"settle.stamp"   ; an entry of tmp/
tmp-word      = %s"cs" / %s"config" / %s"head" / %s"sort" / %s"probe" / %s"extent"
trash-intent  = u64dec                        ; an entry of trash/
trash-item    = u32dec                        ; an entry of trash/<intent>/
fnum          = NZDIGIT *9DIGIT               ; 1 .. 4294967295
u32dec        = "0" / NZDIGIT *9DIGIT         ; 0 .. 4294967295
u64dec        = "0" / NZDIGIT *19DIGIT        ; 0 .. 18446744073709551615
NZDIGIT       = %x31-39
```

*(Informative)* Valid names: `HEAD`, `log.1`, `hist.12`, `seg.base.3`, `seg.d17`, `seg.b42.2`, `seg.b0.1`, `blobs.230`,
`dict.2`, `gitmap.1`, `cs.9`, `tmp/cs.5823400219`, `trash/5242880/0`. Foreign names: `log.0001` (leading zeros),
`log.0` (file number 0), `log.4294967296` (out of range), `Log.1`, `seg.dx`, `tmp/cs.01`.

### 6.4 Names moirai creates outside the store directory

- the pointer file `.moirai` (§3.3);
- the line `.moirai/` in `<git-common-dir>/info/exclude` (§2.3);
- the store directory itself, `moirai` or `.moirai` (§2.1);
- during a `restore` swap, in the store directory's parent: the intent file `<a>.swap` and the temporary directory name
  `<a>.swap-old`, where `<a>` is the store directory's own name (`moirai.swap`, `moirai.swap-old`; `.moirai.swap`,
  `.moirai.swap-old`). `[OS/fs §4.9]` owns the intent's bytes and the rename steps; both names are built from the store
  directory's name and fixed ASCII words, as §6.1 requires (open point 17);
- a backup directory's content ([F16]; open point 18);
- the image destination's files, which follow git's formats ([F14]).

## 7. User-scope configuration (X-F11)

### 7.1 Locations (frozen)

| OS | User-scope configuration file ([80 §2.12], [80] X-F11, [AR §4.1], [AR §13]) |
|---|---|
| Windows | `%APPDATA%\moirai\config` |
| Linux | `$XDG_CONFIG_HOME/moirai/config`, by default `~/.config/moirai/config` |
| macOS | as on Linux: `$XDG_CONFIG_HOME/moirai/config`, by default `~/.config/moirai/config` (not `~/Library/Application Support`) |

### 7.2 Resolving the location

Every process resolves the file by the same rule; `[OS/path §10]` implements it as `user_config_path()`.

**Windows.** If the environment variable `APPDATA` is set, not empty and an absolute path, the file is
`<APPDATA>\moirai\config`. Otherwise there is no user-scope file (rule 7.3.2).

**Linux and macOS.**
1. If `XDG_CONFIG_HOME` is set, not empty and an absolute path, the base is its value. A relative value is ignored, as
   the XDG base-directory specification requires.
2. Otherwise, if `HOME` is set, not empty and absolute, the base is `$HOME/.config`.
3. Otherwise the base is `<home>/.config`, with `<home>` from the user database entry of the effective user.
4. If none yields a directory, there is no user-scope file (rule 7.3.2).

The file is `<base>/moirai/config`.

The file does not need to exist for the location to resolve. A harness that starts the MCP server with a reduced
environment can make the server resolve another file than the CLI (open point 20).

### 7.3 Rules

1. There is exactly one user-scope file per user and machine. It holds only user-scope keys ([AR §13]); its syntax is
   [CFG]'s, like the store's `config`.
2. A missing file, or no resolvable location, means every user-scope key takes its default. `doctor` reports a location
   that cannot be resolved.
3. The file is machine-local. It is never inside a store or a repository, never copied by `backup` or `restore`, and
   never exported: a store restored on another machine never carries it ([AR §13]).
4. It is per principal. A sandbox that runs commands as another local user (the future Windows `srt-win`) reads that
   user's file, so a named root may render `unmapped root` there, never a wrong answer ([80 §2.12]).
5. `moirai config set --user` creates the directory `moirai` under the base when it is missing. How the file is
   rewritten is [CFG]'s (open point 15).
6. Only the location is frozen. The user-scope keys — among them `discovery.git-hint`, `roots.<name>`, `files.main-tree`,
   `files.cloud` and `hooks.session-start.path-export` — are registered in [AR §13] and [CFG] and may change like every
   key; the key set is not frozen ([80] X-F11, [60 §2.5]).

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] "Store layout": directory contents | complete | §5 |
| [60 §2.5] "Store layout": pointer-file format (`moiraidir:`, `store-id:`) | complete, with discovery and the store id | §3.3, §3.1–§3.2, §4 |
| [60 §2.5] "Store layout": store file names from decimal numbers and fixed ASCII words | complete | §6 |
| [60 §2.5] "Store layout": the per-OS user-scope configuration locations | complete | §7 |
| [60 §2.5] "Store layout": the `config` syntax, precedence and unknown-key rule | not here: [CFG] (WP-18) | — |
| [60 §2.5] Cross-platform row: numeric store file names; per-OS user-scope configuration locations | complete for these two items | §6, §7 |
| [80] X-F10 | complete | §6 |
| [80] X-F11 | the frozen locations and their resolution; the registered keys are [CFG]'s | §7 |
| [40] R-9 | the `blobs.<n>` file that holds fingerprint blobs; the blob class is [F10]'s | §5.1 |
| [40] R-13 | the location of the user-scope file that holds `roots.<name>`, `files.main-tree` and `files.cloud`; the keys are [CFG]'s | §7 |
| [90 §10.1] codec (`dict.D` as a raw-content dictionary, a formatted zstd dictionary, or absent) | whether a `dict.<D>` file exists and its name; the dictionary form is [F10]'s | §5.1, Holes |

## Holes

| id | what | decided by | candidates | constraint the value must meet |
|---|---|---|---|---|
| `F02-dict-file` | whether a store holds `dict.<D>` files | measurement 6 (WP-54), filled by WP-81a ([60 §3.1] "dictionary compression", [90 §11.3]) | absent (no dictionary); present, holding a raw-content dictionary behind the `MDIC` header; present, holding a formatted zstd dictionary behind that header ([80 §2.5] rule 4, [AR §4.1]) | must equal the dictionary form [F10] adopts for the same decision; the name `dict.<D>` and the rules of §5.2 and §6 hold in every case |

## Open points for the review

1. **Pointer-file encoding** (gap of [PLAN §3.3], WP-10). Resolved in §3.3: the exact grammar; the reading rule of
   [F01 §6.7] (a byte-order mark skipped, CR LF accepted); no extra lines, so that a future line such as [D §5c]'s
   `branch:` cannot be silently ignored by a format-1 reader; a 4,096-byte cap; relative paths resolved against the
   pointer's directory; no chaining of pointers; lower-case hex only; creation with create-new and a durable write.
   The durability of a pointer file is not in [80 §2.3.2]'s table; this chapter adds it so that a successful
   `init --link` survives a power loss.
2. **Where the store id lives.** [AR §2.14] compares a pointer's store id with the store's, but [AR §4.2]'s `HeadSlot`
   has no store-id field. [F17 §2.1] (WP-16c) defines the 32-byte `InitParams` block of `HEAD` with 16 reserved bytes,
   and its rule IP-1 already keeps the block across `restore` and `repair`. Proposal: WP-16c and WP-11 turn those 16
   bytes into a `store_id` (`b16`) field of `InitParams`, validated as non-zero under IP-2; [F04] places the block.
   This chapter fixes that the id is constant for the store's life and kept by `backup`, `restore` and `repair`.
3. **Number spelling in names.** Resolved in §6.2: minimal decimal with no zero padding. [AR §4.1]'s `NNNN` is read as
   notation. The alternative, padding to at least 4 digits, would also give unique names; it was not chosen because
   one rule for every family is simpler for fixtures and the oracle.
4. **Never-reused file numbers need a persistent allocator.** [AR §4.2] lists `active_log` and the segment set but no
   per-family "next number". After GC deletes a family's highest-numbered file, a number derived from the directory
   listing could be used again, which [80 §2.5] rule 3 forbids. WP-11 and WP-16 should add the allocator — for
   example next-number counters in `HEAD` — or specify another rule that provably never reuses a number.
5. **Conflict: temporary names for segments.** [AR §4.1]'s rules say "segments under construction use a temp name",
   while [80 §2.3.2] and [AR §4.10] say sealed files are created under their final numbers and only `cs.<n>` is
   renamed. This chapter follows [80] (its durability reservation X-F5) and [AR §4.10]. Consequences for [F16]: the
   orphan sweep must recognise an unreferenced final-named file (the crash state "file written, record not") without
   deleting one that a live process is still writing, including a `cs.<n>` in the window between its rename and the
   append of its `Commit` record, and a live process's temporary in `tmp/`.
6. **`tmp/` word list** (§5.3). The words `cs`, `config` and `sort` come from the design; `head` is this chapter's
   proposal (point 14). The list is closed at the freeze; [F16] or `[OS/env]` may add a word before then. If the full
   environment probe of `init` and `restore` (`[OS/env]`) writes its test file inside the new store directory, that file
   needs a name from this grammar (proposal: a `tmp/` word `probe`), so that a crash during the probe leaves no foreign
   entry.
7. **Trash naming** (§5.4). [40 §3.5] fixes `trash/<intent>/` and the rename; the item names are not specified. This
   chapter uses the decimal intent id and the item's zero-based position, so that no project name becomes a store name.
   The intent id's definition is [F05]'s and [F11]'s. Proposal: the log sequence number of the `FsIntent` record, which
   is unique in the store.
8. **Discovery stops on a damaged or stale entry** (§3.1–§3.2). [AR §2.14] says a stale pointer is "reported, not
   followed" without saying whether the walk continues. This chapter stops with exit 7, because continuing could select
   a different store silently (X5). Explicit selections (`--store`, `MOIRAI_DIR`) never fall through to later steps.
9. **Git environment variables.** The hint does not consult `GIT_DIR`, `GIT_COMMON_DIR` or `GIT_WORK_TREE`: [AR §2.14]
   defines the hint as a textual read of `.git`, and a harness may set or strip those variables per process.
10. **Which `.git` the MCP server's hint uses, and the CLI's `--tree`.** [90 §2.2] orders the working-directory walk
    before the tree walk and puts the hint last, without saying whose `.git` it reads. This chapter tries the working
    directory's first, then the tree's. For the CLI, [AR §2.14] does not include `--tree` in discovery, and this
    chapter keeps that. Pass 1 should confirm both, or give the CLI the MCP server's step 3b.
11. **`init --here` inside a repository only at the main worktree's top level.** [AR §2.14] says "in the current
    directory" and also that the hint's second probe finds the store, which is true only at the top level. This
    chapter refuses `--here` in a subdirectory of the main worktree.
12. **`init` does not create at `--store` or `MOIRAI_DIR`.** The design gives `init` no location flag other than
    `--here` and `--link`. This keeps an inherited `MOIRAI_DIR` from creating a store in an unexpected place.
13. **Exit codes of `init`'s placement refusals** (§2.1–§2.2): the shadow guard, `--here` in a linked worktree or a
    subdirectory, a broken `.git`, an existing target. Proposal for [F19]: exit 6 (precondition), keeping exit 7 for
    the environment guard's refused locations and for discovery misses.
14. **`HEAD` last at `init`** (§5.5). Discovery's test "a directory containing `HEAD`" is safe only if `HEAD` appears
    after every other initial entry is durable. The mechanism is proposed for [F16] (WP-16).
15. **Rewriting the user-scope file.** [80 §2.3.2] specifies the store `config` rewrite only. Proposal for [CFG]
    (WP-18): the same steps, with the temporary file in the user's `moirai` directory, named `config.<nonce>` per §5.3's
    nonce rule, followed by `rename_replace` onto `config` and `durable-name` on that directory.
16. **A missing store `config`** (§5.1). Proposal for [CFG]: every store-scope key takes its default, and `doctor`
    reports the missing file. `init` always creates the file.
17. **`restore`'s names beside the store and discovery during a swap** (§3.2, §6.4). `[OS/fs §4.9]` (WP-17) fixed the
    intent file `<a>.swap` and the temporary name `<a>.swap-old` in the store's parent; this chapter lists them.
    Conflict between spec files: `[OS/fs §4.9.4]` says store discovery runs `swap_recover` when it finds `<a>.swap`.
    This chapter keeps discovery free of writes. Discovery runs in lock-free readers, which cannot tell a crashed swap
    from one still in progress under the writer and maintenance bytes, and recovering a running swap would undo it
    halfway. Discovery therefore retries and then exits 7 pointing at `moirai doctor`, as [F15]'s OP-11 proposes for
    [F16]. The review should settle the rule in `[OS/fs]`, [F16] and here together. **Pass 1 (P1-13, S1-26, A1-18):
    settled as here**: discovery never runs `swap_recover` ([F16] P-86, whose seeded bug is exactly that); only `doctor` and a
    restarted `restore` do. **Closed** (round 1): `[OS/fs §4.9.4]` states the same (only `doctor`, and `restore` for its
    own failed swap).
18. **The backup directory's layout** is left to [F16] and M1. Pass 1 should confirm that a backup directory is not
    itself a discoverable store, for example because its `HEAD` is written last or under another name.
19. **Unknown values of `MOIRAI_GIT_HINT`** follow [CFG]'s rule for an invalid value (the default applies and `doctor`
    reports it).
20. **The same user-scope file in every process.** A Codex MCP server receives only a whitelist of environment
    variables: on Unix `HOME` but not `XDG_CONFIG_HOME`, on Windows the "core" variables ([H21 §2.1]). Where the owner
    sets `XDG_CONFIG_HOME`, or where `APPDATA` is not among the forwarded variables, the server would read a different
    user-scope file than the CLI, and `discovery.git-hint` or `roots.<name>` would differ between them. Proposal for
    [90 §3.7]'s Codex rendering (M9): `moirai integrate codex` adds `APPDATA` and `XDG_CONFIG_HOME` to the server
    entry's `env_vars`, and `doctor agents` compares the server's resolved path with the CLI's. A Windows fallback to
    the Roaming AppData known folder was considered and not adopted, following `[OS/path]`'s reason (it would load
    `shell32.dll` into every process) and X-F11's literal `%APPDATA%`.
21. **A store left `retired`** (§3.6; [F16] open point 10, recorded in pass 1, round 2). `restore` makes `retired`
    durable before it creates its swap intent and clears it after a failed swap ([F16] P-85); a crash between those
    steps leaves the live store marked `retired` with no intent. Discovery then finds the same store again, so a process
    repeats the probe with [F16] P-86's delays (covering a running `restore` between its steps 2 and 3) and then exits 7
    `store_retired` ([F19 §10.2]) instead of looping; only `doctor` clears the flag.
