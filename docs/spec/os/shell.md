# OS layer: shell transport — rules T1–T10 (X-F12), argv, stdin and output bytes

| Field | Value |
|---|---|
| Title | OS layer specification, part 2: the CLI transport contract across shells — what moirai emits in argv, what it accepts, how stdin is decoded, the output bytes, hooks and MCP entries, shell hints, and the `os::term` contract |
| Status | draft, pass 1 pending |
| Work package | WP-17b (role R-SPEC-P), part 2 of WP-17 ([PLAN §3.2] item 1) |
| Sources | [80 §4] (which shells run moirai; §4.1 the character table; §4.2 T1–T10); [80 §2.1] (`os::term` row); [80 §2.10] (the CLI-boundary paragraph); [80 §2.12] rows "Console output", "Hook and MCP entry point", "Temporary files"; [80 §3.1] X-F12; [80 §8.1] m11, m15; [50 §6.1] (argv forms, query files), [50 §6.2] (Windows transport, rules 1–7), [50 §6.6] (exit codes); [AR §7.1] (CLI conventions, exit codes), [AR §7.5] (hooks by absolute path); [AR §8.2] item 7 (`${CLAUDE_PLUGIN_DATA}` substitution); [90 §3.7] (Codex rendering: `cmd.exe /C`, the `.mcp.json` entry), [90 §6.2] (bytes), [90 §10.1] row "Output contract" (ASCII in every rendered string), [90 §10.2] M8 (GT12 shells), [90 §10.5] P11; [60 §3.13] GT12 |
| Reconciled with | [OS/README §1.3, §3] (`os::term`; `term.md` is not a separate file, §10), [OS/path §7] (path arguments), [OS/proc §8] (the parent image for the `?` warning), [OS/fs §6.1] (`OsCode`), [LQ/lexical §2.1, §10.1] (BOM, UTF-8, the `?` predicate), [LQ/errors] (E003, E110, W09), [LQ/envelope §2.1–§2.2] (bytes, ASCII template bytes) |

---

## 1. Scope

This file is normative for X-F12: the argv alphabet, stdin decoding and output bytes that join the frozen output contract
([80 §3.1]). It fixes the transport — the bytes between a shell or harness and the `moirai` process. What the bytes mean
(the envelope, the error table, the texts) is [F19]'s and [LQ]'s; where a rule below names a text, [F19] or
[LQ/errors] owns its exact wording.

## 2. Which shells run moirai ([80 §4])

| Caller | Shell | Facts the rules rely on |
|---|---|---|
| Claude Code's Bash tool | bash or zsh only (`CLAUDE_CODE_SHELL`, else `$SHELL` if it is one of those, else a search): zsh on macOS, bash on Linux, Git Bash on Windows | non-interactive; a wrapper sources a snapshot, then turns off `extglob` (bash) or `EXTENDED_GLOB` and `BARE_GLOB_QUAL` (zsh); zsh's `NOMATCH` stays on; **stdin is `/dev/null`** |
| Claude Code's PowerShell tool | Windows PowerShell 5.1 or PowerShell 7 | a here-string piped to a native command arrives as UTF-8 with a BOM [M] |
| Codex agents on Windows | Windows PowerShell 5.1 with only `[Console]::OutputEncoding` set to UTF-8 ([50 §6.2]) | a default 5.1 pipe to a native command turns non-ASCII into `?` [M] |
| Hooks, shell form | `sh -c` (dash on Debian/Ubuntu, bash 3.2 POSIX mode on macOS), Git Bash on Windows | dash 0.5.13 corrupts UTF-8 inside heredocs [M] |
| Hooks, exec form | none | argv array passed as is |
| Codex command hooks on Windows | `cmd.exe /C "…"` ([90 §3.7]) | `%x%` expands even inside quotes; `^` is an escape character |
| Humans | also interactive zsh (possibly `EXTENDED_GLOB`), fish, PowerShell 7 on Unix, cmd | [80 §4.1]'s table |

## 3. Argv: what moirai emits (T1–T4)

Every argv form that moirai's skills, cards, hints, `--show-query` output, documentation and rendered hook entries
**emit** is built from the **teaching alphabet**:

```
emitted-token = flag / pair / ids / word / path-token / quoted-value
flag          = "--" lc-word [ "=" 1*safe-char ] / "-" ALPHA
pair          = key ( "=" / ":" ) *safe-char                     ; k=v, k:v ([50 §6.1])
key           = LCALPHA *( LCALPHA / DIGIT / "_" / "-" )
ids           = 1*DIGIT *( "," 1*DIGIT )                        ; bare ids and id lists (T1)
word          = first-char *safe-char                            ; verbs, revspecs, cursors, names
path-token    = a file path whose characters are safe-char or non-ASCII letters; it may start with "/" (T2)
quoted-value  = key "=" "'" *( any character except "'" ) "'"    ; POSIX shells, fish, PowerShell (T3)
              / key "=" DQUOTE *( any character except DQUOTE and "%" ) DQUOTE   ; cmd only (T3)
safe-char     = ALPHA / DIGIT / "-" / "_" / "." / "," / ":" / "=" / "/" / "+" / "~" / "^" / "@"
first-char    = ALPHA / DIGIT / "_" / "." / "+"
lc-word       = LCALPHA *( LCALPHA / DIGIT / "-" )
```

The rules this grammar enforces:

| # | Rule ([80 §4.2], normative) | How the grammar holds it |
|---|---|---|
| T1 | Ids in argv are bare integers (`show 40`, `scope=88`, `ids=40,41`); `#N` appears only in stdin, `-f` files and MCP strings | `ids`; `#` is not a `safe-char` |
| T2 | No token starts with `#`, `~`, `=`, `@` or `!`, and none starts with `-` unless it is a flag; a token that is not a file path never starts with `/` (Git Bash rewrites such tokens into Windows paths); the `~main` marker appears only in output | `first-char`; only `flag` starts with `-`, only `path-token` with `/` |
| T3 | No unquoted `*`, `?`, `[`, `]`, `{`, `}`, `(`, `)`, `<`, `>`, `\|`, `&`, `;`, `$`, backtick, `"`, `\` or `%`; values with spaces avoided; where one must be in argv, `'…'` in POSIX shells, fish and PowerShell, `"…"` in cmd | none of them is a `safe-char`; `quoted-value` is the only quoting |
| T4 | `main~5`, `main^2`, `a..b` and `a...b` are safe for agents on every OS; the human documentation quotes them in zsh with `EXTENDED_GLOB` and in cmd (`"main^2"`) | `~`, `^` and `.` are `safe-char` inside a token |

- Free text never goes in argv (§5). A glob-list value (`--refs`, `--applies-to`, `--path`) is emitted single-quoted or on
  stdin, never bare ([AR §7.1]).
- A token rendered into a Codex command hook (§7.2) additionally contains no `^`.

## 4. Argv: what moirai accepts

moirai **accepts** more than it emits; the grammar of §3 binds only emitters.

1. **Decoding.** Windows: the command line from `GetCommandLineW` is split by the Microsoft C runtime's rules, as Rust's
   `std::env::args_os` does; each argument must be valid UTF-16 and is converted to UTF-8. Unix: each argument's bytes must
   be valid UTF-8. An argument that fails is a usage error, exit 2 ([F19]).
2. **Ids.** Wherever a bare id is accepted, `#40` is accepted too, and `scope=#88` arrives intact in bash, dash, Git Bash,
   PowerShell and the agents' zsh ([50 §6.1], T1); the skills never teach it.
3. **Paths.** A path argument follows [OS/path §7]: on Windows `\` and `/` are both separators; on Unix `\` is a name
   character and P4 refuses it.
4. **No other rewriting.** moirai never un-escapes, re-splits or glob-expands an argument; what the process receives is
   what it parses.
5. **The Git Bash rewrite.** MSYS rewrites an argument that starts with `/` into a Windows path before moirai sees it; by
   T2 moirai never emits such a non-path token, and a revspec, key or cursor never starts with `/` ([50 §6.2] rule 5).

## 5. Stdin (T5, T6)

### 5.1 The forms (T5)

Free text — LQ queries, `TX` blocks, bodies, summaries, quote text — travels on stdin or in an `-f` file:

| Caller | Form |
|---|---|
| agents in bash or zsh, every OS | `moirai q - <<'EOF'` … `EOF`: a **quoted** heredoc delimiter; an unquoted `<<EOF` is never shown |
| PowerShell | `@'` … `'@ \| moirai q -`, the closing `'@` at column 0 |
| cmd | `moirai q - < FILE` |
| fish | `printf '%s\n' '…' \| moirai q -`, or `-f FILE` |
| `sh` scripts and shell-form hooks | `-f FILE` |
| a script's temporary query file | under the store's `tmp/` ([80 §2.12]); never `$TMPDIR` or `%TEMP%` |
| the Windows PowerShell tool, for queries with non-ASCII literals or after the `?` warning | `%TEMP%\moirai\q.lq` written with the Write tool, then `moirai q -f` ([50 §6.2] rule 2) |
| agents on Linux and macOS | the heredoc only; they never write query files (a sandboxed and an unsandboxed process see different `TMPDIR`s, and the Write tool cannot expand the variable, [81] m11) |
| MCP | JSON strings: immune to every shell rule |

### 5.2 Decoding (T6, identical on every OS)

1. **Only on request.** stdin is read only when the argv has the operand `-` or the flag `--stdin`; otherwise it is never
   touched (the Bash tool's stdin is `/dev/null`).
2. **Read.** All bytes until end of input, **incrementally** (a 64 KiB buffer at a time), counting them: as soon as the
   count exceeds `input.max-bytes` ([CFG §10.5], 16 MiB by default) the read stops and the command exits 2 with
   [F19 §10.2]'s `usage` text for an over-long input, before anything is lexed or written (pass 1, P1-39; [LQ/lexical §8]).
   A `-f FILE` operand is read the same way. When stdin is a Windows console, the characters are read as UTF-16
   (`ReadConsoleW`), a Ctrl+Z (U+001A) ends the input, and the text is converted to UTF-8, the bound counting the UTF-8
   bytes; an unpaired surrogate is invalid. Otherwise the raw bytes are read.
3. **BOM.** If the bytes start with EF BB BF, those three bytes are removed, once.
4. **Validate.** The rest must be valid UTF-8 (no overlong forms, no surrogates, no value above U+10FFFF); otherwise
   exit 2 with E003 ([LQ/errors], [LQ/lexical §2.1, §10.1], [50 §6.2] rule 6). This turns dash's heredoc corruption into
   a loud error. Steps 3–4 are the LQ lexer's §2.1 rules applied to every stdin text, LQ or not.
5. **Nothing else.** CR, LF and every other byte are passed on unchanged; the consumer decides (the LQ lexer treats CR LF
   as a line break; a body is stored byte-exact).

### 5.3 The PowerShell `?` warning

If the parent process's image is `powershell` or `pwsh` ([OS/proc §8]) and the text satisfies the `?` predicate of
[LQ/lexical §10.1], moirai prints warning W09, which suggests `-f` ([LQ/errors], [50 §6.2] rule 6). It is never a silent
fix. The
parent image is diagnostics-grade, so a wrong guess costs one misplaced warning, nothing else.

## 6. Output bytes (T7)

1. **Encoding.** Every byte moirai writes to stdout and stderr is UTF-8. Lines end with LF (0x0A); moirai never writes CR.
   Every string moirai renders itself (headers, footers, errors, hints, notices) is ASCII ([90 §10.1]); user data
   (titles, bodies, paths) is UTF-8 and escaped by [F19]'s untrusted-text rule.
2. **No terminal control off a TTY.** No ANSI escape sequence (no byte 0x1B) is written to a stream that is not a
   terminal.
3. **Console versus pipe** ([80 §2.12]). On Windows, a stream attached to a console is written with `WriteConsoleW` after
   converting the UTF-8 to UTF-16 (a UTF-8 sequence split across two writes is completed before conversion); any other
   stream receives the raw UTF-8 bytes. Output never depends on the console code page or the locale.
4. **Broken pipe.** A write to stdout that fails because the reader went away (Windows `ERROR_BROKEN_PIPE` 109 or
   `ERROR_NO_DATA` 232; Unix `EPIPE`, `SIGPIPE` being ignored) ends the process quietly: no further output, nothing on
   stderr, exit code 0. A broken stderr is ignored.
5. **Byte-identical across OSes.** One set of golden files serves every OS. Before comparison, the golden harness replaces
   exactly three things in the actual output ([80 §4.2] T7, [81] m15):

| Placeholder | Replaces | How the harness finds it |
|---|---|---|
| `<ROOT>` | the fixture tree's root | every occurrence of the tree's canonical root text ([OS/path §4]) |
| `<OSERR>` | an OS error detail | the OS-error unit below, as a whole |
| `<OS-DETAIL>` | the OS-specific detail of an R4 state (for example the trash location's name) | the per-OS detail strings that [F18] lists |

   No other difference between OSes is allowed.
6. **The OS-error unit.** An OS error code is rendered only as the ASCII unit `os <code> <SYMBOL>`: `<code>` is the
   decimal value of the `OsCode` (an `i32`, [OS/fs §6.1]; Win32 codes and `errno` values are non-negative), `<SYMBOL>` its symbolic name from moirai's own table
   (Windows `ERROR_*` names, for example `ERROR_SHARING_VIOLATION`; Unix `errno` names, for example `EBUSY`), or `?` for a
   code the table lacks. `FormatMessageW` and `strerror` texts are never printed: they are localised and not ASCII
   (open point 2). Where two `errno` names share one value on an OS, the table prints one fixed name: the one that OS's
   `errno.h` defines first, not the alias defined in terms of it. So `EAGAIN`, never `EWOULDBLOCK`, and on Linux
   `EOPNOTSUPP` for 95, which `ENOTSUP` aliases there; macOS, where the two differ (45 and 102), prints each under its
   own name.

## 7. Hooks and MCP entries (T8)

### 7.1 The rule

Hooks and MCP server entries use the **exec form** — an argv array — with an **absolute path** to the binary:
`${CLAUDE_PLUGIN_DATA}/bin/moirai.exe` on Windows and `${CLAUDE_PLUGIN_DATA}/bin/moirai` on Unix, or the expanded absolute
path when measurement 7 shows that Claude Code does not substitute the variable in an exec-form hook `command` or in
`.mcp.json` (re-checked at M8; `moirai hooks install` then writes the expanded path) ([80 §2.12], [AR §8.2] item 7). No
shell sits on any Claude Code hook path or MCP entry. Hook input arrives as JSON on stdin; argv carries no free text.

### 7.2 The Codex exception ([90 §3.7], [80 §2.12])

- Codex runs **command hooks** under `cmd.exe /C "…"`. The rendered command string is `moirai hook <name> --client codex`
  (and fixed flags), built only from teaching-alphabet tokens (§3) with no `^`, no `%`, no inner `"` and none of
  `& | < > ( )`, so cmd's expansion and escaping cannot touch it.
- Codex's MCP entry is `"command": "moirai"` with an args array, resolved on `PATH` (Codex's plugin format; the app's CLI
  is not on `PATH`, [90 §3.7]).
- Both are recorded as the harness-defined exceptions to T8's "exec form, absolute path"; [90] governs them (open point 3).

## 8. Shell-problem hints (T9)

Error texts that name a shell problem say which one ([80 §4.2] T9). Texts are [F19]'s and [LQ/errors]'s, ASCII, ≤ 600 B:

| Situation | Who prints it | Hint (wording owned by [F19]) |
|---|---|---|
| a verb that needs an id receives none (an unquoted `#40` was a comment; a PowerShell `@x` splat vanished) | moirai | E110's hint `'#' starts a shell comment; write 40` ([LQ/errors]) |
| zsh aborted with "no matches found" before moirai ran | the skill and documentation (moirai never sees it) | `zsh: no matches found -> quote the value or use stdin` (ASCII `->`, open point 4) |
| PowerShell turned non-ASCII into `?` | moirai (§5.3) | W09 ([LQ/errors]) |
| stdin is not valid UTF-8 | moirai (§5.2) | E003 |

## 9. GT12 (T10)

GT12 runs T1–T9 through the real shells with Claude Code's wrapper ([60 §3.13] GT12, [80 §4.2] T10):

| Phase | Shells |
|---|---|
| M0–M11 (Windows) | Git Bash (Claude Code's wrapper), Windows PowerShell 5.1 and PowerShell 7, PowerShell 5.1 under Codex's command prefix ([90 §10.2] M8, probe P11), `cmd.exe /C` for Codex command hooks, and cmd for the human documentation's forms |
| port phase | `bash -c` on Ubuntu, `dash -c` for shell-form hooks, `zsh -c` on macOS with and without a user `EXTENDED_GLOB` set before the wrapper's reset, fish forms from the documentation, bash 3.2 `sh -c` |

## 10. The `os::term` contract

[80 §2.1] lists `os::term`: console versus pipe detection, UTF-8 output (`WriteConsoleW` on a Windows console), and a
broken pipe exiting quietly with 0. Its contract is §5.2 steps 1–3 and §6 items 1–4 above. On every target the Rust
standard library's stdio implements exactly that contract: `std::io::Stdout` and `Stderr` write a Windows console with
`WriteConsoleW` after completing split UTF-8 sequences and write other handles as raw bytes; `std::io::Stdin` reads a
Windows console with `ReadConsoleW`, converts to UTF-8, treats Ctrl+Z as end of input and rejects an unpaired surrogate;
a write to a closed pipe returns `ErrorKind::BrokenPipe` on every OS (the Rust runtime ignores `SIGPIPE`);
`std::io::IsTerminal` answers the TTY question. Target-independent code (`moirai-app`, M8) therefore implements T6 and T7
through `std::io` without an OS seam, and `moirai-os` carries **no `term` code** unless GT12 shows a deviation, in which
case the fix is added to `moirai-os::term` behind a trait in `moirai-vfs` (open point 1). The only OS reading the
transport needs is the parent image, which comes through `ProcHost` ([OS/proc §8]).

## Coverage

The rows of `COVERAGE.md` that cite this file ([F01 §2.7]).

| Item | Part covered here | Section |
|---|---|---|
| `60-AU-CrossPlatform` (the "Cross-platform" summary row) | X-F12, whose parts are the row `X-F12` | — |
| `X-F12` ([80] X-F12) | T1: the emitted argv alphabet; accepted argv; stdin forms and decoding (T5, T6); output bytes (T7); hooks and MCP entries (T8); shell-problem hints (T9); GT12 (T10); `os::term`. The LQ side is [LQ/lexical]'s, [LQ/errors]'s and [LQ/envelope]'s, the output contract [F19]'s | §3, §4, §5, §6, §7, §8, §9, §10 |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none: the `${CLAUDE_PLUGIN_DATA}` substitution (measurement 7) selects between two forms fixed in §7.1 and changes no byte of this contract | — | — | — |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [OS/README §1.3, §3] expects a `term.md` in part 2, which WP-17b's brief does not list | the `os::term` contract is §10 of this file; Rust's stdio implements it, so no seam and no `moirai-os` code exist unless GT12 finds a deviation; README §1.3 and §3 point `os::term` here | WP-17a |
| 2 | T7's `<OSERR>` substitution needs a delimited OS-error form, and localised system texts would break [90 §10.1]'s ASCII rule | the unit `os <code> <SYMBOL>` from moirai's own symbol table (§6 item 6); [OS/fs §6.1] and [F19] adopt it | WP-17a, WP-18 |
| 3 | T8 says "exec form with an absolute path, no shell on any hook path", while [90 §3.7] and [80 §2.12] render Codex command hooks under `cmd.exe /C` and the MCP entry as a bare `moirai` on `PATH` | precedence: [90] for its own rendering; T8 holds for Claude Code and every exec-capable harness; the Codex forms are constrained by §7.2 | R-REV-A |
| 4 | [80 §4.2] T9 writes the zsh hint with `→`; [90 §10.1] makes every rendered string ASCII | `->` (§8) | WP-18 |
| 5 | The teaching alphabet of §3 is stricter than T1–T4 require (for example it has no `!` or `#` inside a token) | stricter is compatible: it only restricts emitters; acceptance (§4) is unchanged | R-REV-A |
| 6 | Console stdin (a human typing into cmd) is not covered by [80 §4] | §5.2 step 2 fixes it (UTF-16 read, Ctrl+Z ends input), matching Rust's stdio | R-REV-P |
| 7 | §6 item 6 did not say which symbol is printed when two `errno` names share a value (WP-30) | **closed (spec sync 2a):** the name the OS's `errno.h` defines first: `EAGAIN` (not `EWOULDBLOCK`), Linux `EOPNOTSUPP` for 95 | — |
