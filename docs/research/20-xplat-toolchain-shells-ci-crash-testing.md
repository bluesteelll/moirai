# 20 — Cross-platform toolchain: shells, Claude Code, distribution, CI and crash testing (Linux, macOS, Windows)

*Research for the owner requirement of 2026-09-26: moirai must run on macOS and Linux as well as Windows, with all three first-class. This lens covers the shells agents and humans type into, how Claude Code runs commands, hooks and the sandbox on each OS, how the binary is built, signed and installed, which CI runners exist and what they cost, and how crash and power-loss testing works on each OS. Storage primitives (flush, locks, mmap) and file identity are other lenses; they appear here only where CI or crash rigs must test them.*

*Date: 2026-09-26. Nothing in moirai was implemented and no repository was modified. Probes ran locally (probe scripts are not published). There is no Mac, and WSL2 is **not installed** on the owner's machine [M], so Linux and macOS facts come from primary documentation, source code (Claude Code 2.1.281's own bundled code, sandbox-runtime, zsh, Rust std, runner-images) and from probes of bash, POSIX-mode bash and dash under Git for Windows. Tags: **[M]** measured here, **[D]** documented by the vendor or a spec, **[S]** read in source code, **[C]** third-party claim, **[I]** my inference. Citations such as [AR §7.1], [16 §6.10], [50 §6.2], [60 §3.13] and [07 §4.2] point to the design of record and earlier reports.*

---

## 0. Executive summary

1. **Claude Code's Bash tool only ever runs bash or zsh, never fish or dash** [S]. It picks `CLAUDE_CODE_SHELL`, then `$SHELL` if that is bash or zsh, then searches for zsh and bash in `/bin`, `/usr/bin`, `/usr/local/bin` and `/opt/homebrew/bin`. On macOS the default login shell makes it zsh; on most Linux desktops it is bash. Every command runs as `<shell> -c '<source snapshot> && <glob-off> && eval '<command>' && pwd -P >| <file>'`. That is a **non-interactive** shell, so `#` at the start of a word is always a comment, in zsh too [S, zsh `lex.c`]. The glob-off step is `shopt -u extglob` for bash and `setopt NO_EXTENDED_GLOB NO_BARE_GLOB_QUAL` for zsh [S]. So the zsh `#`/`~`/`^` glob operators are **off for agents** even when the user's `.zshrc` turns them on. zsh's `NOMATCH` stays on, though: an unquoted `*`, `?` or `[` that matches no file aborts the whole command, and `~name` for an unknown user is an error [S, zsh `subst.c`]. In bash and dash the same characters silently become file names when a file matches [M].
2. **One set of CLI rules is safe on every shell.** Ids are bare integers. No argument token starts with `#`, `~`, `=`, `@`, `/` or `!`. Argv never carries glob characters, braces, redirections or `$`. Values with spaces use single quotes. Query text and bodies go through stdin from a *quoted* heredoc (`<<'EOF'`) in bash and zsh, which is byte-exact [M], through `-f PATH` in PowerShell, fish and `sh` scripts, or as MCP strings. This tightens [50 §6.2] in one place: `scope=#88` is safe for agents but fails in a human zsh that has `EXTENDED_GLOB` set, so the taught form becomes `scope=88`.
3. **The Claude Code sandbox on Linux and macOS changes three things moirai relies on.**
   - (a) **Writes.** Sandboxed commands may write the working directory, a per-user `TMPDIR` and, for linked worktrees, the main repository's shared `.git` except `hooks/` and `config` [D]. `<git-common-dir>/moirai/` is therefore writable in the normal cases. It is **not** writable when a session starts in a subdirectory of the main checkout [I]. A per-user state directory (`~/.local/state`, `~/Library/…`) is never writable from the sandbox.
   - (b) **Unix sockets.** On Linux a seccomp filter makes `socket(AF_UNIX)` fail with `EPERM` and cannot allow by path. On macOS each socket path must be allow-listed [D, S]. The optional leader's IPC therefore cannot be the only path; the CLI's direct mode must stay complete.
   - (c) **PIDs.** Linux sandboxing runs every command in its own **PID namespace** (`bwrap --unshare-pid --unshare-user --proc /proc`) [S], so a PID that a sandboxed `moirai` records is meaningless to any other process. On macOS, Seatbelt allows `signal` and `process-info*` only to targets in the same sandbox [S]. The design's "PID + start time" liveness check [AR §6.2] needs a namespace-aware holder id and a third answer, *unknown*, that never reclaims a lease.
4. **Hooks and MCP servers are not sandboxed**, and exec-form hooks resolve `command` through the Claude Code process's `PATH` [D]. The snapshot the Bash tool sources ends with `export PATH=<Claude process PATH>:<plugin bin dirs>` [S]. A Claude Desktop launched from the macOS Dock has `PATH=/usr/bin:/bin:/usr/sbin:/sbin` [C, anthropics/claude-code#44649]. So hooks and `.mcp.json` must name moirai by **absolute path**; `${CLAUDE_PLUGIN_DATA}/bin/moirai` is a stable one. The SessionStart hook should append `export PATH=…` to `CLAUDE_ENV_FILE` [D] so that the agent's bare `moirai` resolves however Claude Code was launched.
5. **Claude Code 2.1.281 already embeds a native-Windows sandbox helper (`srt-win`).** It runs commands as a separate local `srt-sandbox` user with a restricted token and ACL-based file rules [S]. The public docs still say native Windows is not supported [D]. If it ships, sandboxed moirai processes on Windows run as a different principal. A per-user pipe DACL, `%LOCALAPPDATA%` and `OpenProcess`-based liveness would all break. Designing for "the client may be a different, restricted principal" now keeps one semantics on all three OSes.
6. **Distribution.**
   - **Linux:** static `x86_64`/`aarch64-unknown-linux-musl` binaries. These are Tier 2 with host tools and ship musl 1.2.5 since Rust 1.93 [D]. musl's allocator is much slower: 513 ms against glibc's 56 ms in one 22-thread benchmark [C], and ripgrep ships jemalloc on musl for that reason [S]. Use mimalloc or jemalloc as the global allocator and let the M0 measurement pick.
   - **macOS:** one universal2 binary (arm64 + x86_64; x86_64-apple-darwin is Tier 2 now [D], and macOS 26 is the last Intel release [C]). For personal use an ad-hoc signature suffices. Downloads made by curl and gh are not quarantined [I]. Distribution to others needs a Developer ID signature, the hardened runtime and notarization. A bare binary's ticket cannot be stapled; Gatekeeper fetches it online [D].
   - **Windows:** SmartScreen reputation. EV certificates no longer help [C]. Artifact Signing costs $9.99/month and serves individuals only in the US and Canada [C].
   - Install to `~/.local/bin` on Linux and macOS and `%LOCALAPPDATA%\Programs\moirai` on Windows. Never keep writable per-user state on the command path.
7. **CI (2026).** GitHub-hosted standard runners exist for Linux x64 and arm64, Windows x64 (Server 2022/2025) and arm64 (Windows 11), and macOS arm64 (M1, 3 vCPU) and Intel (`macos-15-intel`, `macos-26-intel`) [D]. They are free for public repositories [D]. For private repositories the per-minute prices are Linux $0.006 (arm64 $0.005), Windows $0.010 and macOS $0.062 [D]. Linux runners expose `/dev/kvm` [D]; arm64 macOS runners cannot nest virtual machines [D]. **Hosted Windows images run with Defender real-time protection disabled and `C:\` and `D:\` excluded** [S, runner-images], so no hosted runner can measure the Defender-on budgets. [60] already treats hosted runners as a secondary signal, and this confirms it.
8. **Crash testing is a question of which window of loss each rig can see** (§7.1). A VM hard power-off with the host still running normally does not lose writes that the guest already issued to its virtual disk. With the usual QEMU, VirtualBox and Virtualization.framework cache modes they sit in the host's page cache or on the physical drive [D/I]; calibration must confirm this per VMM. VM rigs therefore catch missing `fsync`/`fdatasync`/`F_FULLFSYNC`/`NtFlushBuffersFileEx` calls (guest page-cache loss) and kernel-crash behaviour. They cannot catch missing device flushes or reordering between flushes. Only the `Vfs` simulator (GT1) sees those on every OS, plus Linux's **dm-log-writes** replay [D]. **LazyFS** (FUSE, v0.3.1, 2026-05) simulates page-cache loss on Linux thousands of times faster than VM reboots [D]. It is the cheapest way to catch the Unix-only bug class of a missing directory fsync after create or rename. macOS has no dm-* or FUSE-based equivalent; its OS-crash rig needs Apple-silicon hardware running Virtualization.framework guests with `synchronizationMode = .full` [D].
9. **Recommended crash matrix for one Windows 11 Home laptop** (§7.6):
   - the simulator and short kill loops on hosted runners for all three OSes;
   - nightly LazyFS, dm-log-writes and a QEMU/KVM Linux power-off loop on hosted Ubuntu x64 (free if the repository is public, about $2 per night if private);
   - the Windows VirtualBox power-off loop on the laptop, which currently has **no hypervisor running** so VirtualBox would run natively [M], or on a Windows 11 Pro test host with Hyper-V (`Stop-VM -TurnOff` [D]);
   - macOS OS-crash cycles on a Mac mini (about $599 once) or on rented cloud Macs (Scaleway M4 €0.22/h, AWS mac2.metal $0.65/h, each with a 24 h minimum [C]).
   Installing WSL2 on the laptop switches VirtualBox to the slower Hyper-V backend [C], so the Linux layers belong in CI.
10. **Platform abstraction (§8).** Keep one on-disk format and one protocol with compile-time per-OS modules behind a small seam. The pieces this lens owns:
    - a shell-independent transport contract (argv alphabet, stdin decoding, UTF-8/LF output, exit codes) tested through the real shells of each OS;
    - a `HolderId {os, boot_id, ns_id, pid, start}` with tri-state liveness, reserved in format v1;
    - one injected clock (libfaketime cannot hook static musl binaries [D], and Linux time namespaces do not cover `CLOCK_REALTIME` [D]);
    - a `TestHost` seam for kill, suspend and disk-full injection;
    - a cross-OS format gate: a store written on each OS must open and pass `doctor --verify` on the other two.

---

## 1. Environment, method and what is OS-specific today

| Item | Value |
|---|---|
| Owner machine | Windows 11 Home Single Language 10.0.26200; Ryzen 9 5900HS; `systeminfo` reports VM Monitor Mode Extensions yes, virtualization enabled in firmware, VBS not enabled; `HypervisorPresent = False` [M] |
| WSL | `wsl --status`: "The Windows Subsystem for Linux is not installed" [M]; no Linux probe was possible |
| VM software | VirtualBox, VMware Workstation and QEMU are not installed [M] |
| Shells available locally | Git for Windows: GNU bash 5.3.9, `bash --posix`, dash 0.5.13.2 (MSYS build) [M]. zsh and fish are not available; I read their behaviour from source and manuals |
| Claude Code | 2.1.281 inside Claude Desktop (`CLAUDE_CODE_ENTRYPOINT=claude-desktop`) [M]. Its bundled JavaScript was searched read-only for the shell wrapper, snapshot and sandbox code [S] |

**What in the current design depends on the OS, for this lens.**

| Mechanism in the design | Where | Windows today | Linux / macOS equivalent (this report) |
|---|---|---|---|
| Argv transport rules (bare ids, `-f`, heredoc, no `@file`) | [16 §6.10], [50 §6.2], [AR §7.1] | Git Bash + PowerShell 5.1, measured | bash/zsh under Claude Code's wrapper, dash for `sh -c` hooks, fish/zsh for humans (§2) |
| Hooks exec form, `moirai` on `PATH` | [07 §9.5], [AR §7.5], [40 §6.4] | `.exe` required for exec form | `sh -c` for shell form; `PATH` depends on how Claude Code was launched (§3.2) |
| Sandbox assumptions | [08 §2, §9], [AR §2.2] | "Bash sandbox not supported on native Windows" | Seatbelt/bubblewrap write, socket and PID rules (§3.3); `srt-win` coming on Windows (§3.4) |
| Lease liveness by `(pid, start time)`, LOCK holder `{pid, start_ms}` | [AR §4.1, §6.2] | `OpenProcess` + creation time | PID namespaces, Seatbelt `same-sandbox` (§3.3, §8) |
| Leader over a named pipe with per-user DACL | [AR §6.1] | `\\.\pipe\…`, `PIPE_REJECT_REMOTE_CLIENTS` | UDS blocked in the sandbox; `sun_path` 104/108 bytes (§3.3) |
| Signed binary at a stable path (Defender) | [AR §4.10], [05 §6.4] | Defender rescans rebuilt binaries | Gatekeeper/notarization, XProtect first-run scans, static musl (§4) |
| Test host, hosted Windows Server runners, OS-crash rig in VirtualBox/VMware, GT4 Windows kill loop, GT15 | [60 §3.13, M0 item 7, measurement 17] | NotMyFault, `TerminateProcess`, `NtSuspendProcess`, VHDX disk-full | runners for all three OSes, LazyFS, dm-log-writes, QEMU/KVM, Virtualization.framework (§5–§7) |

---

## 2. Shells and the CLI transport

### 2.1 Which shell runs an agent's command, and how [S]

From Claude Code 2.1.281's bundled code (excerpts saved in `cc_wrapper_excerpt.txt`):

- **Selection** (function `DIn`):
  1. `CLAUDE_CODE_SHELL`, accepted only if the path contains `bash` or `zsh` and is executable.
  2. Otherwise `$SHELL`, if it contains `bash` or `zsh`.
  3. Otherwise a search over `/bin`, `/usr/bin`, `/usr/local/bin` and `/opt/homebrew/bin` plus `which` results. It tries bash first if `$SHELL` names bash, zsh first otherwise.
  4. If nothing is found: "No suitable shell found. Claude CLI requires a Posix shell environment."
  **A fish or nushell user therefore gets zsh or bash**, never their own shell.
- **Snapshot.** At session start Claude Code runs `<shell> -c -l '<script>'`, which sources `~/.zshrc` or `~/.bashrc` with stdin from `/dev/null` and writes a snapshot. The snapshot holds the user's functions, the user's options (zsh: `setopt | sed 's/^/setopt /'`; bash: `set -o` lines that are on, plus `shopt -s expand_aliases`), the aliases, rg/find/grep shims, and finally **`export PATH=<Claude process PATH + plugin bin/ dirs>`**. Only on Windows is the PATH taken from `bash -lc 'echo $PATH'` instead.
- **Each command** runs as `<shell> -c "<cmdstring>"`, adding `-l` only when the snapshot failed. The command string is:

  ```
  source <snapshot> 2>/dev/null || true && [export TMPDIR=…] && setopt NO_EXTENDED_GLOB NO_BARE_GLOB_QUAL 2>/dev/null || true   # bash: shopt -u extglob
    && { \builtin unalias -- 'unsetenv'; … } && eval '<command>' && pwd -P >| <cwd-file>
  ```

  Under the sandbox Claude Code also sets `TMPDIR` (the sandbox temp directory), `CLAUDE_CODE_TMPDIR` and **`TMPPREFIX=<tmp>/zsh`**, so that zsh's heredoc temp files land in a writable place.
- **Measured on Windows** [M]: `$-` is `hmtBc`, which is non-interactive; `interactive_comments` is on and `extglob` off; **stdin is `/dev/null`**, so `cat` returns EOF in about 150 ms and stdin is not a TTY. moirai must never read stdin unless given `-`/`--stdin`, as [AR §7.1] already says.

### 2.2 Shell-by-shell behaviour of the characters moirai's syntax uses

zsh column: the agent's zsh as Claude Code runs it (non-interactive, `NO_EXTENDED_GLOB`, `NOMATCH` on). The "human zsh" notes cover an interactive zsh whose `.zshrc` may set `EXTENDED_GLOB`.

| Construct | bash / `bash --posix` (Linux, Git Bash) | dash (`/bin/sh` on Debian/Ubuntu; hook shell form) | zsh as the agent's shell (macOS default) | Human interactive zsh | fish (humans only) | PowerShell 5.1 / 7 |
|---|---|---|---|---|---|---|
| `#` at the start of a word | comment [M] | comment [M] | comment: `-c`/`eval` strings always allow comments [S, `lex.c` 678–681] | **literal** unless `INTERACTIVE_COMMENTS` is set (default off, marked `<K><S>`) [D] | comment | comment [M, 16] |
| `#` inside a word (`scope=#88`) | literal [M] | literal [M] | literal (`EXTENDED_GLOB` off) [S] | **glob operator** if `EXTENDED_GLOB` is set: no match gives the error "no matches found" [D] | literal | literal [M, 16] |
| `~name` at the start of a word | literal if no such user [M] | literal [M] | **error "no such user or named directory"** (with `NOMATCH`) [S, `subst.c` 791–793] | same error | expands if the user exists; avoid [I] | literal |
| `k=~x` | tilde-expanded like an assignment (non-POSIX bash) [D] | literal | literal (`MAGIC_EQUAL_SUBST` off) [D] | literal | — | literal |
| `main~5`, `main^2` | literal [M] | literal [M] | literal [S] | **glob operators** with `EXTENDED_GLOB` (exclusion, negation) [D] | literal | literal [M, 16] |
| `=40` at the start of a word | literal [M] | literal [M] | **error "40 not found"** (`EQUALS` on by default) [S, `subst.c` 724–726] | same | literal | literal |
| `*`, `?`, `[x]` unquoted | **replaced by matching file names**, literal only when nothing matches [M] | same [M] | **error "no matches found"** when nothing matches, replaced when something matches [D] | same | error "No matches for wildcard" [C] | literal |
| `{a,b}` | expands [M] | literal [M] | expands | expands | expands | script block [M, 16] |
| `<`, `>`, `\|` | redirect/pipe [M] | redirect [M] | redirect | redirect | redirect | pipe; `<` is an error in 5.1 [M, 16] |
| `$x`, backticks | expand | expand | expand | expand | `$x` expands; `(cmd)` is command substitution | `$x` expands |
| `'single quotes'` | literal [M] | literal [M] | literal | literal | literal | literal; inner `"` stripped by 5.1 [M, 16] |
| `<<'EOF'` quoted heredoc | **byte-exact** [M] | **corrupts UTF-8 in dash 0.5.13.x** (§2.4) [M] | byte-exact [D]; temp file under `TMPPREFIX` [S] | byte-exact | **no heredocs at all** | none; here-strings plus a BOM [M, 16] |

### 2.3 Probe: the argv forms of [50 §6.2] through bash, POSIX bash and dash [M]

Each form was fed through `<shell> -c '…; eval "$(cat form)"'`, the way Claude Code runs commands. The working directory held files named `scope88`, `a1b` and `x`. Full output is in `probe_argv_posix.txt`.

| Typed | bash 5.3.9 | `bash --posix` | dash 0.5.13.2 |
|---|---|---|---|
| `show #40 41` | `<show>` | `<show>` | `<show>` |
| `show '#40' "#41"` | `<#40><#41>` | same | same |
| `q ready scope=#88` | `<scope=#88>` | same | same |
| `log range=main~5..main` · `show main^2` · `show main@{1}` · `show 12@c4471 :id` | intact | intact | intact |
| `show ~main` · `show at=~main` · `show =40` | intact | intact | intact |
| `find prio:<=1` | `=1: No such file or directory` | same | `cannot open =1` |
| `find a?b` · `find [x]` · `find scope*` | **`<a1b>` · `<x>` · `<scope88>`** (silent substitution) | same | same |
| `find text=a?b` (no file matches) | literal | literal | literal |
| `show {a,b}` | `<a><b>` | `<a><b>` | `<{a,b}>` |
| `find text='lease reclaim' kind=note` | `<text=lease reclaim><kind=note>` | same | same |
| `cat > f <<'EOF'` with `" ' # $ \` ~ ^ {} <= * ? [x]` and Cyrillic, wrapped in Claude's `eval '…'` quoting | **byte-exact (125 bytes)** | **byte-exact** | **corrupted** (next section) |
| Same body with an *unquoted* `<<EOF` delimiter | the backticks ran `cmd`, which hung waiting for input: unquoted heredocs execute text | — | — |

In zsh (derived from source and manual, not run): `show ~main` and `show =40` abort the command. `find a?b` substitutes when a file matches and aborts when none does. Everything else behaves as in bash.

### 2.4 A dash 0.5.13 bug that matters for `sh -c` and git hooks [M, C]

Under dash 0.5.13.2, a quoted heredoc wraps every multibyte UTF-8 character in the internal `CTLMBCHAR` markers `85 02 … 02 85`. `правило` becomes `85 02 d0 bf 02 85 85 02 d1 80 02 85 …` [M, `dash_ctlmbchar_probe.txt`]. Git's maintainers hit the same bug in April 2026. Herbert Xu posted a parser fix in May 2026; the bug was present on dash master when reported [C, ratatoskr.run/git/2026/04/8033555]. Debian stable and testing still ship dash 0.5.12-12, which predates the multibyte code; 0.5.13.5 reached Debian *experimental* on 2026-09-23 [D, tracker.debian.org]. macOS `/bin/sh` is bash 3.2 in POSIX mode by default, selectable through `/private/var/select/sh` [C].

Consequences:
- Claude's Bash tool never uses dash, so agent heredocs are safe.
- Shell-form hooks (`sh -c`), `#!/bin/sh` git hooks and `shell: sh` CI steps may run dash. moirai's own git hook blocks ([40 §4.7]) must not pass UTF-8 text through heredocs; exec form is the rule [07 §9.5].
- The corrupted bytes are **invalid UTF-8** (a lone `0x85`), so the existing rule "reject invalid UTF-8 on stdin, exit 2" [16 §6.10 rule 5] turns this silent corruption into a loud error. Keep that rule identical on all OSes.

### 2.5 Human shells differ from agent shells

- A human typing in interactive zsh without `INTERACTIVE_COMMENTS` passes `#40` literally. The agent's zsh drops it as a comment. The two disagree in opposite directions, so neither form can be taught as "works everywhere".
- Many zsh frameworks set `EXTENDED_GLOB`. For a human that breaks `scope=#88`, `main~5` and `main^2`. For agents it does not, because Claude re-disables the option after sourcing the snapshot [S].
- fish has no heredocs. The human documentation shows `moirai q -f FILE` or `printf '%s\n' '…' | moirai q -` for fish.
- PowerShell 7 on macOS and Linux passes native arguments in `Standard` mode, which preserves quotes [C], unlike 5.1 [07 §5.2]. `#`, `@`, `{}` and `$` keep their PowerShell meaning.

### 2.6 Conclusion: CLI rules safe on every shell (normative proposal)

| # | Rule | Replaces / tightens |
|---|---|---|
| T1 | **Ids in argv are bare integers** (`show 40`, `scope=88`, `ids=40,41`). `#N` appears only in stdin, `-f` files and MCP strings. moirai still *accepts* `#40` and `scope=#88` when they arrive; the skill never teaches them. | [50 §6.2] said `scope=#88` is safe [M on Windows]; it is not for a human zsh with `EXTENDED_GLOB` |
| T2 | No argv token **starts** with `#`, `~`, `=`, `@`, `/`, `!` or `-` (except real flags). The `~main` marker is output-only; revspec input never starts with `~`. | extends [16 §6.10 rule 6] (`/`) with `~`, `=`, `!` |
| T3 | Argv never contains unquoted `*`, `?`, `[`, `]`, `{`, `}`, `(`, `)`, `<`, `>`, `\|`, `&`, `;`, `$`, a backtick, `"` or `\`. Values with spaces use **single quotes**. | [50 §6.2 rule 3], now justified on all shells: silent file-name substitution (bash, dash) or aborts (zsh) |
| T4 | `main~5`, `main^2` and `a..b`/`a...b` are safe for agents on every OS. The human docs say to quote them in zsh with `EXTENDED_GLOB`. | new |
| T5 | Free text travels as **stdin from a quoted heredoc** (`moirai q - <<'EOF'`) in bash and zsh, which is every agent shell on Linux, macOS and Windows (Git Bash). `-f PATH` is the form for PowerShell, fish, `sh` scripts and hooks. MCP strings are immune. An unquoted `<<EOF` is never shown. | [50 §6.2 rule 2] unchanged, now cross-OS |
| T6 | stdin decoding is identical on every OS: strip one leading BOM; reject invalid UTF-8 with exit 2 (this catches dash `CTLMBCHAR` corruption); keep the `?`-heuristic warning for PowerShell; never read stdin without `-`/`--stdin`. | [16 §6.10 rule 5] plus the dash case |
| T7 | Output is UTF-8 with LF, has no ANSI off a TTY, and is **byte-identical across OSes** (shared golden files). A broken pipe on stdout exits quietly with status 0, as ripgrep does [S]. | new |
| T8 | Hooks and MCP use **exec form** (an argv array) with an absolute binary path (§3.2). No shell sits on any hook path. | [07 §9.5] made mandatory |

GT12 (contract and transport, [60 §3.13]) should run T1–T8 through the real shells of each OS, using Claude Code's exact wrapper:
- `bash -c` on Ubuntu;
- `zsh -c` with `setopt NO_EXTENDED_GLOB NO_BARE_GLOB_QUAL` on macOS, plus a variant with the user option `EXTENDED_GLOB` set *before* the reset;
- `dash -c` for the shell-form hook path;
- Git Bash, Windows PowerShell 5.1 and pwsh 7 on Windows.

---

## 3. Claude Code on macOS and Linux

### 3.1 Hooks

- **Shell form** (no `args`): "`sh -c` on macOS and Linux, Git Bash on Windows, or PowerShell when Git Bash isn't installed" [D, hooks]. On Debian and Ubuntu `sh` is dash (§2.4); on macOS it is bash 3.2 in POSIX mode [C].
- **Exec form** (`args` present): "Claude Code resolves `command` as an executable on `PATH` and spawns it directly with `args` as the argument vector. There is no shell" [D]. On Windows `command` must resolve to a real `.exe` [D].
- **Environment** [D]:
  - hooks inherit the Claude process environment, minus `OTEL_*` and minus the variables `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` strips;
  - hooks get `CLAUDE_PROJECT_DIR`, `CLAUDE_PLUGIN_ROOT`, `CLAUDE_PLUGIN_DATA` and `CLAUDE_PLUGIN_OPTION_<KEY>`;
  - `CLAUDECODE=1` is set in the Bash tool, hooks and stdio MCP servers;
  - `CLAUDE_CODE_CHILD_SESSION` separates tool and hook children from processes inside an MCP server;
  - `${user_config.KEY}` substitutes into MCP server configs and exec-form hook `args`, but not into shell-form commands;
  - "On macOS and Linux, command hooks run in their own session without a controlling terminal."
- **`CLAUDE_ENV_FILE`** exists for SessionStart, Setup, CwdChanged and FileChanged hooks. `export` lines appended to it apply to later Bash-tool commands [D].
- **Hooks and MCP servers run outside the sandbox.** The sandbox docs protect `.claude/hooks` and `.mcp.json` because "a command that could edit those files could … add a hook or MCP server that Claude Code runs outside the sandbox" [D]. The sandbox "applies only to Bash, PowerShell, and Monitor commands and their child processes" [D].

### 3.2 `PATH` and locating the binary

- The Bash tool's `PATH` is the Claude Code process `PATH` plus the plugin `bin/` directories [S, §2.1]. It is not the `PATH` the user's rc file builds.
- A Claude Desktop started from the macOS Dock sees `PATH=/usr/bin:/bin:/usr/sbin:/sbin` [C, #44649, closed as a duplicate]. Hooks likewise miss Homebrew tools [C, rtk-ai/rtk#685]. The Terminal-launched CLI inherits the shell's `PATH`.
- A plugin `bin/` directory is on the Bash tool's `PATH`, but "claude.ai and Cowork don't install a plugin that has this directory" [D].

**Recommendation** [I, verify at M0/M8]:
1. Hooks use exec form with `"command": "${CLAUDE_PLUGIN_DATA}/bin/moirai"`, a stable per-user path that survives plugin updates [D]. `moirai setup`, run by the owner, places the binary there: a symlink on Linux and macOS, a hardlink or copy on Windows, because exec form needs a real `.exe`. The `.mcp.json` entry uses the same path, or `${user_config.moirai_path}`.
2. The SessionStart hook appends `export PATH="<dir of moirai>:$PATH"` to `CLAUDE_ENV_FILE`. The agent's bare `moirai …` then works however Claude Code was started, without a plugin `bin/`.
3. `moirai doctor hooks` checks both paths from inside a hook and from a Bash-tool command.

### 3.3 The sandbox on macOS (Seatbelt) and Linux (bubblewrap)

| Aspect | macOS | Linux / WSL2 | Consequence for moirai |
|---|---|---|---|
| Enforcement | Seatbelt, built in [D] | bubblewrap + socat, optional seccomp filter; Ubuntu 24.04+ AppArmor needs a `bwrap` profile [D] | — |
| Writable by default | cwd subtree, `--add-dir` directories, per-user `TMPDIR`; `/tmp/claude` and a few conveniences [D, S] | same | a store under `<git-common-dir>/moirai/` is writable when the session cwd is the repo root; see the next row |
| Linked worktrees | "the sandbox also allows writes to the main repository's shared `.git` directory … Writes to `hooks/` and `config` inside that directory remain denied" [D] | same | writable from worktree sessions. **A session started in a subdirectory of the main checkout has `.git` outside its cwd, so writers get EPERM/EROFS** [I]. moirai should exit 7 and print the exact `sandbox.filesystem.allowWrite` entry (`//<abs>/.git/moirai`). Readers still work because they open read-only and take no locks |
| Protected paths | `.claude/*` settings, skills, agents, hooks; `.mcp.json`; shell rc files; `.git/hooks`, `.git/config`; top-level `HEAD`/`objects`/`refs`/`config` of the cwd ("would turn your working directory into a bare git repository") [D] | same; **Linux deletes** a top-level `HEAD`/`objects`/`refs` that appears during a command [D] | the store's own `HEAD` and `config` sit below `.git/moirai/` or `.moirai/`, never at the cwd top level. Keep it that way |
| Unix sockets | need `(allow system-socket (socket-domain AF_UNIX))` plus a path allow via `allowUnixSockets` [S] | seccomp makes `socket(AF_UNIX)` return `EPERM`; "seccomp cannot filter by path"; only `allowAllUnixSockets` lifts it [D, S] | the optional leader's UDS is unreachable from sandboxed CLI calls. The CLI's direct mode must be complete and correct on its own ([08 §9] already says so) |
| Network namespace | proxy | `--unshare-net` when the network is restricted [S] | abstract-namespace sockets are per network namespace; never use them |
| **PID namespace** | none | **`--unshare-pid --unshare-user --proc /proc`, `--new-session --die-with-parent`** for each command [S] | a PID recorded by a sandboxed `moirai` is namespace-local. Two concurrent sandboxed commands can both be PID 3. The lease and LOCK holder identity must carry a namespace id, and liveness must allow "unknown" (§8) |
| Seeing and signalling other processes | `(allow process-info* (target same-sandbox))`, `(allow signal (target same-sandbox))`; `sysctl-read` allows `kern.proc.pid.*` [S] | other processes are invisible [S] | macOS: `kill(pid, 0)` on an outside process may fail with `EPERM`, which must mean *alive*; read the start time through `sysctl(KERN_PROC_PID)`, which is allowed. Linux: evaluate liveness only when `ns_id` equals your own |
| Temp dir | `TMPDIR` points into the sandbox; zsh's `TMPPREFIX` is redirected [S] | same | temp files for segments and exports belong **inside the store directory**, never in `TMPDIR`: sandboxed and unsandboxed moirai processes see different `TMPDIR`s, and a rename across filesystems fails with `EXDEV` [I] |
| Escape hatches | `excludedCommands`, `dangerouslyDisableSandbox` retry, `allowUnsandboxedCommands: false` [D] | same, plus `enableWeakerNestedSandbox` for Docker | `moirai` should never need to be excluded; a writer store path in `allowWrite` is the documented fix |

### 3.4 Native Windows sandbox on the horizon [S]

sandbox-runtime now has `src/sandbox/windows-sandbox-utils.ts`. It describes a Rust helper, `srt-win.exe`, that "provisions a dedicated `srt-sandbox` local user account", enforces egress with WFP filters keyed on that account's SID, spawns the target as a restricted-token child under `srt-sandbox`, and enforces file rules "via additive explicit ACEs for `<sb-SID>`". Strings from this helper are inside Claude Code 2.1.281's binary [S]. The published docs still say "Native Windows is not supported" [D]. If the helper is enabled:

- files created by a sandboxed `moirai` belong to another principal, so the store's ACL inheritance must let the owner's unsandboxed MCP server and hooks read and write them;
- the leader pipe's per-user DACL ([AR §6.1]) would reject sandboxed clients, which then fall back to direct mode;
- `OpenProcess` on the owner's processes returns `ACCESS_DENIED`, which must mean *alive*, the same rule as macOS `EPERM`;
- `%LOCALAPPDATA%` is the sandbox user's directory.

**Design stance:** on every OS, a client may be a restricted or namespaced principal.

### 3.5 Configuration paths per OS

| Item | macOS / Linux | Windows |
|---|---|---|
| User settings, skills, agents, plugins | `~/.claude/` (`settings.json`, `skills/`, `agents/`, `plugins/`); `CLAUDE_CONFIG_DIR` overrides [D] | `%USERPROFILE%\.claude` [D] |
| MCP servers (user/local scope), trust state | `~/.claude.json` [D] | same under `%USERPROFILE%` |
| Project | `.claude/settings.json`, `.claude/settings.local.json`, `.mcp.json` [D] | same |
| Plugin data (stable) | `~/.claude/plugins/data/<id>/`, deleted on uninstall unless `--keep-data` [D] | same |
| Managed settings | `/Library/Application Support/ClaudeCode/` (macOS), `/etc/claude-code/` (Linux) [D, via settings reference] | `C:\Program Files\ClaudeCode\` [D] |
| Claude Code's own install | native installer writes `~/.local/bin` and `~/.claude` [C] | Desktop: `%APPDATA%\Claude\claude-code\<ver>\claude.exe` [M] |

---

## 4. Distribution

### 4.1 Targets and Rust tiers [D, platform-support page, parsed 2026-09-26]

| Target | Tier | Minimum | Note |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | 1, host tools | kernel 3.2, glibc 2.17 | dynamic; will not run on Alpine and needs work on NixOS |
| `aarch64-unknown-linux-gnu` | 1, host tools | kernel 4.1, glibc 2.17 | |
| `x86_64-unknown-linux-musl` | 2, host tools | musl 1.2.5 (since Rust 1.93, 2026-01-22 [D]) | static by default; the recommended Linux artifact |
| `aarch64-unknown-linux-musl` | 2, host tools | musl 1.2.5 | same |
| `aarch64-apple-darwin` | 1, host tools | macOS 11 | |
| `x86_64-apple-darwin` | **2**, host tools | macOS 10.12 | macOS 26 is the last Intel release; Rosetta 2 is general-purpose through macOS 27 [C] |
| `x86_64-pc-windows-msvc` | 1, host tools | Windows 10 / Server 2016 | |
| `aarch64-pc-windows-msvc` | 1, host tools | — | a `windows-11-arm` runner exists (§5) |

### 4.2 Linux: static musl versus glibc; the allocator and RAM

- **Portability.** A static musl binary runs on any kernel-compatible distribution, including Alpine and NixOS, with no loader, no `GLIBC_2.x` symbol-version failures and no shared libraries in the RSS [I]. A glibc build can target an old glibc baseline (for example with cargo-zigbuild) but stays dynamic. Nothing in moirai needs NSS or `dlopen`: git transport is spawned [AR §4.10] and the home directory comes from `$HOME`. musl is therefore feasible [I].
- **Allocator speed.** musl's mallocng is slow. ripgrep's source says: "musl's allocator … appears to be substantially worse … even though ripgrep isn't particularly allocation heavy, musl's allocator appears to slow down ripgrep quite a bit. Therefore, when building with musl, we use jemalloc" [S]. One 22-thread benchmark measured glibc 56 ms, musl 513 ms, musl + jemalloc 67 ms and musl + mimalloc 57 ms, with a warning that "mimalloc uses more memory than both Glibc and jemalloc" [C].
- **Why moirai is less exposed.** moirai's CLI is single-threaded, and the MCP server has no threads [AR §6.1], so lock contention, musl's worst case, does not arise. Per-allocation cost still counts in pack rendering and query evaluation [I].
- **RAM.** mallocng returns memory eagerly, which gives low RSS. jemalloc and mimalloc keep freed pages longer. That matters for the long-lived MCP server, not for the millisecond-lived CLI [I]. The binary's text pages are shared between the 16 concurrent CLI processes whether it is linked statically or dynamically [I].
- **Recommendation.** Ship static musl for x86_64 and aarch64. Use **the same global allocator on all three OSes** (mimalloc is the [05 §15] default), so allocation behaviour and RAM numbers compare across platforms. M0 measures CLI peak RSS/PSS and MCP-server steady RSS for {glibc + system, musl + mallocng, musl + mimalloc, musl + jemalloc}, and the result is written down, not assumed.
- **Test-harness consequence.** libfaketime works through the dynamic loader and "cannot work with statically linked binaries" [D]. Linux time namespaces virtualise only `CLOCK_MONOTONIC` and `CLOCK_BOOTTIME`, not the wall clock [D]. GT4's "wall-clock steps of ±1 h" therefore has to go through moirai's injected clock ([60] M0 item 2 already injects one into the `Store` API). Test builds read a harness-controlled offset on every OS [I].

### 4.3 macOS: universal2, signing, notarization, Gatekeeper

- **Signing is mandatory on Apple silicon.** Every arm64 executable must carry a valid signature; an ad-hoc one is enough, and Apple's linker adds it automatically [C, Eclectic Light 2020 and 2026]. `lipo -create` of the arm64 and x86_64 slices, or any post-link edit, needs a re-sign (`codesign -s - --force`, or a Developer ID) [C].
- **Notarization for distribution to others.** It requires a Developer ID certificate and the hardened runtime [D]. The notary accepts zip, dmg and pkg. "Although tickets are created for standalone binaries, it's not currently possible to staple tickets to them"; Gatekeeper "finds the ticket online" [D].
- **Gatekeeper.** It applies to quarantined files, which is what browsers and quarantine-aware apps produce. Files fetched with `curl` or `gh`, or built locally, normally carry no `com.apple.quarantine` attribute and are not assessed at launch [I; verify on a Mac]. macOS 15 removed the Control-click override; users must go to System Settings → Privacy & Security → "Open Anyway" [D, Apple developer news].
- **First-run scans.** XProtect and syspolicyd scan new executables, which is the analogue of Defender [I]. The rule "a stable install path, never run from `target/`" [05 §6.4] carries over.
- **Recommendation.** Ship a universal2 binary while the Intel runners exist (the two slices double the file size, not the RAM [I]). For the owner's own machines, ad-hoc signing plus `gh release download` or a Homebrew tap formula is enough. Buy the Apple Developer Program ($99/year) only if moirai goes to other people (owner decision).

### 4.4 Windows: signing and SmartScreen

- SmartScreen reputation builds per publisher over time. Since August 2024 an EV certificate gives no advantage over OV [C, MS Learn via search].
- Azure **Artifact Signing** (formerly Trusted Signing) costs from $9.99/month. It serves organizations in the US, Canada, the EU and the UK, but **individuals only in the US and Canada** [C, MS FAQ]. Since March 2026 its rotation of intermediate CAs can re-trigger SmartScreen on every release [C, MS Q&A].
- SmartScreen checks files that carry the Mark of the Web, which browsers add. Locally built binaries and ones fetched with command-line tools usually carry none [I; verify with `Get-Item -Stream Zone.Identifier`].
- Defender scans regardless ([05 §6.4]).

### 4.5 Install paths and per-user directories

| | Binary | Hook/MCP entry point | Per-user writable state |
|---|---|---|---|
| Linux | `~/.local/bin/moirai` (Claude Code's installer uses the same directory [C]) | `${CLAUDE_PLUGIN_DATA}/bin/moirai`, a symlink | **none on the command path.** A sandboxed Bash command cannot write `~/.local/state` or `~/.cache`. Everything lives in the store; caches, if any, go inside the store |
| macOS | `~/.local/bin/moirai`, or `/opt/homebrew/bin` from a tap | same, a symlink | same (`~/Library/Application Support` is not writable from the sandbox) |
| Windows | `%LOCALAPPDATA%\Programs\moirai\moirai.exe`, per user with no admin, at a stable path for Defender | `${CLAUDE_PLUGIN_DATA}/bin/moirai.exe`, a hardlink or copy | same rule, so behaviour does not change when `srt-win` arrives |

---

## 5. CI

### 5.1 GitHub-hosted runners in 2026 [D, runner reference]

| Label(s) | OS / architecture | Private repo (vCPU / RAM / SSD) | Public repo |
|---|---|---|---|
| `ubuntu-24.04`, `ubuntu-22.04`, `ubuntu-26.04` | Linux x64 | 2 / 8 GB / 14 GB | 4 / 16 GB |
| `ubuntu-24.04-arm`, `ubuntu-22.04-arm`, `ubuntu-26.04-arm` | Linux arm64 | 2 / 8 GB | 4 / 16 GB |
| `ubuntu-slim` | Linux x64 | 1 / 5 GB | same |
| `windows-2025`, `windows-2022` | **Windows Server** x64 | 2 / 8 GB | 4 / 16 GB |
| `windows-11-arm` | **Windows 11** arm64 | 2 / 8 GB | 4 / 16 GB |
| `macos-14`, `macos-15`, `macos-26` | macOS arm64 (M1) | 3 / 7 GB / 14 GB | same |
| `macos-15-intel`, `macos-26-intel` | macOS x64 | 4 / 14 GB | same |

- Runners have passwordless sudo on Linux and macOS, and run as admin with UAC off on Windows [D].
- `/dev/kvm` is usable on the 2-vCPU Linux runners since 2024-04-02, after adding a udev rule [D, GitHub changelog].
- Nested virtualization is unsupported on arm64 macOS runners [D].
- A job may run at most 6 h [D, Actions limits].
- **Windows images disable Defender**: `DisableRealtimeMonitoring = $true`, `ExclusionPath = C:\, D:\`, passive mode [S, runner-images `Configure-WindowsDefender.ps1`]. No x64 Windows 11 image exists; `windows-11-arm` is Windows 11, but Defender is disabled there too [S]. A Defender-on measurement needs the dedicated test host or the owner's laptop, as [60 §5.1] already plans.

### 5.2 Cost [D unless tagged]

- **Per-minute prices** since the January 2026 repricing:

  | Runner | Price per minute |
  |---|---|
  | Linux 1-core | $0.002 |
  | Linux 2-core x64 | $0.006 |
  | Linux 2-core arm64 | $0.005 |
  | Windows x64 or arm64 | $0.010 |
  | macOS | $0.062 |
  | Linux 4/8/16-core | $0.012 / $0.022 / $0.042 |
  | macOS xlarge (M2 Pro) | $0.102 |

- **Included minutes:** Free 2,000/month; Pro 3,000; Team 3,000. They drain at 1× for Linux, 2× for Windows and 10× for macOS [C].
- **Free usage:** public repositories and self-hosted runners cost nothing. The announced $0.002/min charge for self-hosted runners was postponed indefinitely [C, GitHub resources page and changelog].

Illustrative monthly cost for a private repository (est., inputs shown):

| Workload | Assumption | Cost/month |
|---|---|---|
| PR checks on 5 runners (Linux x64 and arm64 10 min each, Windows x64 and arm64 15 min each, macOS arm64 15 min) | 60 PRs × ($0.06 + $0.05 + $0.15 + $0.15 + $0.93) | ≈ $80 |
| Nightly Linux x64: LazyFS + dm-log-writes + QEMU power-off loop | 5 h × $0.36/h × 30 | ≈ $54 |
| Nightly short kill loops: Linux arm64, Windows x64, macOS arm64, 1 h each | ($0.30 + $0.60 + $3.72) × 30 | ≈ $139 |
| **Total** | before included minutes (≈ $18 of Linux-equivalent) | **≈ $270** |

A public repository pays $0 for the same work. A self-hosted Mac mini removes the largest line: macOS minutes cost $3.72/h, so about $600 of hardware pays back in roughly 5 months of 1 h nightly runs [I].

### 5.3 Running the multi-process and kill-loop gates on each OS

One Rust harness with a per-OS `TestHost` implementation (§8):

| Action | Linux | macOS | Windows |
|---|---|---|---|
| Kill at a random point | `SIGKILL` | `SIGKILL` | `TerminateProcess` |
| Pause a holder (byte-0 holder, checkpoint holder, reader) | `SIGSTOP`/`SIGCONT`, or the cgroup freezer for a whole tree | `SIGSTOP`/`SIGCONT` | `NtSuspendProcess` |
| Lock release after death | immediate on process exit [I] | immediate [I] | may lag [AR §4.10] |
| Disk full | loop-mounted ext4, xfs or btrfs image (`truncate`, `mkfs`, `sudo mount -o loop`); tmpfs `size=` only for `ENOSPC` paths, since tmpfs has no durability | `hdiutil create -size 64m -fs APFS` then `hdiutil attach` [I] | small VHDX; `New-VHD` needs the Hyper-V module (absent on Home), so use `diskpart create vdisk` (admin) [I] |
| Wall-clock step | injected clock only (§4.2) | injected clock | injected clock (the same code path everywhere) |
| Acknowledgement channel | anonymous pipe (GT4) or socket (GT15) | same | same |
| Filesystems to cover | ext4 (default), xfs, btrfs [I] | APFS, which is **case-insensitive by default** | NTFS; ReFS/Dev Drive per [05] |

**Recommended CI plan.**
- *PR:* GT1 and GT2 on Linux x64 (the cheapest runner). GT12 transport tests on every OS/shell pair (§2.6). A 5–10 minute GT4 smoke run on ubuntu x64 and arm64, macos-26, windows-2025 and windows-11-arm. Cross-OS format identity (§8, G-X).
- *Nightly:*
  - GT3 on Linux;
  - LazyFS and dm-log-writes on ubuntu x64 (§7);
  - the Linux QEMU/KVM power-off loop, about 100 cycles per night within the 6 h limit;
  - GT4 long runs on self-hosted Windows (test host or laptop) and on the Mac if one exists;
  - GT15 Windows on the VM host.
- *Milestone exits:* owner's-machine measurements per [60 §5.1], plus macOS and Linux measurement floors (spawn through the zsh/bash wrapper and flush cost) on real hardware.

---

## 6. Where durability semantics differ, as far as rigs must test them

The storage lens owns the primitives. The test rigs have to exercise these OS differences [D, S]:

- **Durable flush.** `NtFlushBuffersFileEx(DATA_SYNC_ONLY)` on Windows, `fdatasync` on Linux, and `fcntl(F_FULLFSYNC)` on macOS. Rust std's `sync_data` and `sync_all` both use `F_FULLFSYNC` on Apple targets [S, `library/std/src/sys/fs/unix.rs`]. Plain `fsync` on macOS does not flush the drive's cache; that is exactly why Virtualization.framework's `.fsync` mode is only "best-effort" [D].
- **Directory durability.** On Linux and macOS a created or renamed file's directory entry is durable only after an `fsync` of the parent directory [I, POSIX practice]. Windows code never needed this. **It is a new, Unix-only bug class**, and LazyFS and dm-log-writes on Linux are the cheap way to catch it.
- **Lock semantics.** Windows byte-range locks versus POSIX `fcntl`/OFD locks versus BSD `flock` behave differently; the storage lens covers them. The kill loops must run on every OS, because lock-release and close-releases-lock semantics differ.

---

## 7. Crash and power-loss testing per OS

### 7.1 What each technique can observe

Windows of loss: **W1** unwritten process buffers; **W2** written but not flushed (the OS page cache); **W3** issued to the device but not flushed (the drive's volatile cache, or reordering between flushes); **W4** torn sector or page; **W5** flush error followed by retry ("fsyncgate").

| Technique | W1 | W2 | W3 | W4 | W5 | OS |
|---|---|---|---|---|---|---|
| GT1 `Vfs` crash enumeration (simulator) | ✔ | ✔ | ✔ | ✔ | ✔ | all (OS-independent) |
| GT4 process kill loop | ✔ | ✘: the page cache survives process death | ✘ | ✘ | ✘ | all |
| VM hard power-off, host alive (QEMU/KVM, VirtualBox, Hyper-V, VZ) | ✔ | ✔: guest RAM is lost | **✘**: data the guest already issued sits in the host page cache or on the physical drive and survives, in every cache mode [I from D] | rare | ✘ | Linux, Windows and macOS guests |
| Guest kernel panic (sysrq-c, NotMyFault) | ✔ | ✔ | ✘ | rare | ✘ | same |
| **LazyFS** `clear-cache` / `crash` / `torn-op` | ✔ | ✔ (its own page cache; metadata coverage to verify) | ✘ | ✔ (torn-op/torn-seq) | ✘ | Linux (FUSE) |
| **dm-log-writes** + `replay-log` to any entry or mark | ✔ | ✔ | **✔**: writes are logged only at the next `PREFLUSH`, so replaying to arbitrary points shows the state the drive could have held [D] | via custom replay | ✘ | Linux |
| **dm-flakey** `error_writes`/`drop_writes`/`corrupt_bio_byte` | — | drop after a point | — | corruption | **✔**: `error_writes` turns flushes into EIO [D] | Linux |
| Physical power cut (smart plug) | ✔ | ✔ | only if the drive lies about FLUSH | ✔ | ✘ | real hardware only |

**Consequence.** No VM rig on any OS, the planned Windows GT15 included, can observe W3 while the host stays up. The design already says "Loss of the drive's own volatile cache is covered only by the simulator" [60 §3.13 GT15]. This table shows that the same holds for *every* issued-but-unflushed write, not only a lying drive. **Measurement 17** ([60 §5.2]) should state which window its calibration proves: "an unflushed write is lost at least once" is satisfied by W2 alone. It should also record, as a limitation, that W3 cannot be observed, so nobody reads GT15 as covering flush placement. On Linux, dm-log-writes can cover W3 at the block layer.

### 7.2 Linux tools

| Tool | What it is | Status | Use for moirai |
|---|---|---|---|
| **dm-log-writes** | device-mapper target that logs every WRITE, FLUSH, FUA and DISCARD plus user marks. "WRITE requests are not actually logged until the next REQ_PREFLUSH request"; `replay-log --end-mark`, `--check fua`, `--fsck` [D] | in mainline; the userspace tool is `josefbacik/log-writes` (last push 2024-07) [M, API] | inside a KVM guest with a generic kernel. Run GT4-style workloads with marks at each acknowledgement, then replay to every flush and to random points between flushes; re-open and check acknowledged commits and `doctor --verify`. Covers ext4, xfs and btrfs |
| **dm-flakey** | up/down intervals; `drop_writes`, `error_writes`, `error_reads`, `corrupt_bio_byte`, `random_*_corrupt` [D] | mainline | W5 (a flush error aborts the process [AR §6.5]) and the corrupt-read paths |
| **LazyFS** | "a FUSE file system with an internal dedicated page cache that only flushes data if explicitly requested"; `lazyfs::clear-cache`, `::crash` with op/timing/path, `::torn-op`, `::torn-seq` [D] | v0.3.1, 2026-05-07; active [M, API]; VLDB 2024 paper | fastest W2 loop: no reboot, thousands of cycles per hour [I]. Mount the store directory on LazyFS; check `fdatasync` placement and **directory fsyncs**. Check that moirai's read-only `mmap` of sealed segments works over FUSE (`direct_io` limits shared mappings on older kernels) [I] |
| **CrashMonkey** (OSDI'18) | block-level record and replay with automatic workload generation, aimed at file-system bugs | last push 2022-10 [M, API] | a design reference only: GT1's "per-file prefixes plus one torn sector" already follows it [60 M0 item 3] |
| **ALICE** (OSDI'14) | application-level crash states from strace traces under abstract persistence models | last push 2018-02 [M, API] | a design reference (the persistence models inform GT1); not a runnable dependency |
| **Chipmunk** (EuroSys'23) | crash consistency for persistent-memory file systems | 2024-06 [M, API] | not applicable (no PM) |
| **QEMU/KVM power-off** | `cache=writeback` (default): data goes to the host page cache and guest flushes reach the host; `none` is `O_DIRECT` with flushes honoured; **`unsafe` sets `cache.no-flush`**, so flushes are ignored [D] | — | kill QEMU (SIGKILL or QMP `quit`) or panic the guest with `echo c > /proc/sysrq-trigger`. With the host alive, `unsafe` and `writeback` give **identical** results for a VM kill (the host page cache survives both), so `unsafe` is acceptable for speed on disposable overlays. Only a host crash differs, and then the image is discarded anyway [I] |

### 7.3 macOS

- **No dm-* equivalent and no in-kernel FUSE.** macFUSE needs a kernel extension or the File Provider path, and I found no LazyFS port [I]. W2 and W3 layers therefore come from the simulator plus the Linux runs of the shared Unix code.
- **OS-crash rig.** Virtualization.framework guests (tart, UTM) on Apple silicon.
  - `VZDiskImageSynchronizationMode.full` "ensures the data moves from the disk's internal cache to permanent storage"; `.fsync` has "the same guarantees as the fsync system call"; `.none` "doesn't guarantee data integrity if any error condition occurs, such as … panic, power loss" [D]. Use `.full`.
  - Power-off is a SIGKILL of the VM process.
  - A guest kernel panic needs SIP relaxed in the guest [C]. Start with hard power-off only.
  - GitHub's arm64 macOS runners cannot run nested VMs [D], so the rig needs a physical Mac or a bare-metal cloud Mac. The macOS licence allows two extra macOS VMs per Mac [C], which fits a two-guest loop.
- **What APFS promises.** Durability requires `F_FULLFSYNC`; `F_BARRIERFSYNC` gives ordering without a full flush [D, fcntl(2)]. A VM kill cannot tell `F_FULLFSYNC` from `fsync` apart (that is W3). Choosing the macOS primitive is therefore a documentation-plus-simulator decision, not a rig result [I].
- **Cloud Macs** [C]: Scaleway Mac mini M4 at €0.22/h with a 24 h minimum (≈ €160 for a continuous month); AWS `mac2.metal` (M1) at $0.65/h and `mac-m4.metal` at $1.23/h, billed per Dedicated Host with a 24 h minimum. Buying a Mac mini M4 costs about $599 once.

### 7.4 Windows

- **VirtualBox.** Flushes are ignored by default: "normally these requests are ignored for improved performance". Enable them with `VBoxManage setextradata <vm> "VBoxInternal/Devices/ahci/0/LUN#<n>/Config/IgnoreFlush" 0` [D, manual ch. 12]. Hard off is `VBoxManage controlvm <vm> poweroff`.
- **The owner's laptop currently runs no hypervisor** [M], so VirtualBox would use VT-x/AMD-V directly. Installing WSL2 turns on Virtual Machine Platform and the Hyper-V hypervisor; VirtualBox then runs through the Windows Hypervisor Platform in its degraded "green turtle" mode [C]. **Keep WSL2 off the rig machine.** Run the Linux layers on hosted runners instead.
- **VMware Workstation Pro.** Free for all uses since 2024-11-11 [D, VMware blog]. It coexists with Hyper-V through the Windows Hypervisor Platform [C]. Hard off is `vmrun stop <vmx> hard`. Its flush behaviour needs the same calibration [I].
- **Hyper-V** (Windows 11 Pro, not Home). `Stop-VM -TurnOff` "is equivalent to disconnecting the power from the virtual machine" [D]. If the dedicated test host [60 M0] runs **Windows 11 Pro**, Hyper-V with checkpoints is the most scriptable Windows rig, and the host doubles as a self-hosted runner [I].
- **Guest licensing.** A Windows 11 guest needs a licence or the 90-day Enterprise evaluation image. This is an owner logistics item.

### 7.5 Calibration refinement (edit to [60] measurement 17)

For each rig, record three results:

- (a) a deliberately unflushed write (W2) is lost at least once in ≥ 100 power-offs;
- (b) a flushed write is never lost;
- (c) **whether an issued-but-unflushed write (W3) is ever lost.** The expected result is "never", because the host keeps it. If a VMM loses in-flight writes on power-off (possible for VirtualBox with the host I/O cache off), the rig gains partial W3 coverage and that is recorded instead.

Result (c) documents what the rig cannot observe, so GT15 is not read as coverage of flush placement.

Run the same calibration on every rig: VirtualBox or Hyper-V (Windows), QEMU/KVM (Linux), VZ (macOS). On Linux, one dm-log-writes run shows the positive W3 case.

### 7.6 Recommended crash-test matrix for a one-laptop owner

| Layer | Linux | macOS | Windows | Where | Cost |
|---|---|---|---|---|---|
| L0 simulator (GT1, GT3), OS-independent, W1–W5 | ✔ | ✔ | ✔ | hosted Linux; every PR and nightly | ≈ $0 public / low private |
| L1 kill loops (GT4) | ✔ `SIGKILL`/`SIGSTOP` | ✔ | ✔ `TerminateProcess`/`NtSuspendProcess` | PR smoke on hosted runners for all three OSes; nightly long runs on self-hosted Windows and Mac | included above |
| L2 page-cache loss without reboots (W2) | **LazyFS** | — | — | hosted ubuntu x64, nightly | part of ≈ $54/month private |
| L3 block-level flush placement (W3, W5) | **dm-log-writes** + **dm-flakey** in a KVM guest; ext4, xfs, btrfs | — | — | hosted ubuntu x64 with `/dev/kvm`, nightly | same |
| L4 OS crash (GT15) | QEMU/KVM guest: SIGKILL, QMP quit, sysrq-c | VZ guest (`.full`) on a Mac mini or cloud Mac: SIGKILL of the VM | VirtualBox (laptop, no WSL2) or Hyper-V (Windows 11 Pro test host), NotMyFault | Linux: hosted nightly (about 100 cycles per night). Windows: laptop windows or test host. macOS: Mac | Linux ≈ $0–54/month; Mac mini ≈ $599 once, or Scaleway ≈ €5.3/day; Windows $0 on the laptop, or test host plus a Pro licence |
| L5 physical power cut | optional | optional | optional | smart plug on a mini-PC | not required: the design assumes FLUSH is honoured |

**Gate volumes (proposal; owner decision).**
- Keep [60]'s GT15 volumes for Windows: ≥ 1,000 cycles at M1 exit and ≥ 5,000 cumulative.
- Linux is cheap in CI, so run the same volumes plus LazyFS and dm-log-writes as mandatory nightly gates from M1.
- For macOS, run ≥ 1,000 cycles at M1 exit if a Mac is available. Otherwise use a rented cloud Mac at each milestone exit, with the simulator carrying W2–W5.

---

## 8. Platform abstraction: one on-disk format, one protocol semantics

**Principles** [I]:
1. **One format, byte-identical.** All integers little-endian. Store paths are relative with `/`. No OS-dependent field widths. Holder and lease records carry an `os` tag plus fields that every OS can fill (below).
2. **One protocol.** The commit, lock, publish and recovery protocol and its acknowledgement semantics are identical everywhere. OS differences live only in leaf primitives (flush, directory sync, locking a byte, read-only mapping, process identity, the clock), selected with compile-time `cfg` modules, not a runtime trait object (no dynamic dispatch on the commit path).
3. **The weakest client defines the contract.** A client may be sandboxed: in another PID or user namespace (Linux), confined to the same sandbox (macOS), a different principal (Windows `srt-win`), or unable to open sockets. Direct mode is complete; the leader is only an optimisation; liveness has a third answer.

**The seams this lens adds or changes:**

```rust
// OS-specific leaf, compiled per target; everything above it is shared.
pub struct HolderId { os: u8, boot_id: [u8; 16], ns_id: u64, pid: u32, start: u64 }
pub enum Liveness { Alive, Dead, Unknown }            // Unknown never reclaims: TTL decides
pub trait Clock { fn now_ms(&self) -> i64; }          // one injected clock; test offset on all OSes
pub trait TestHost {                                  // test builds only
    fn kill(&self, p: &Child); fn suspend(&self, p: &Child); fn resume(&self, p: &Child);
    fn small_volume(&self, bytes: u64) -> PathBuf;    // loop image / hdiutil / diskpart
}
```

| OS | `boot_id` | `ns_id` | start time | Liveness mapping |
|---|---|---|---|---|
| Linux | `/proc/sys/kernel/random/boot_id` | inode of `/proc/self/ns/pid` | `/proc/<pid>/stat` field 22 | `ns_id` or `boot_id` differs → `Unknown`; process gone → `Dead`; start time differs → `Dead` (PID reuse) |
| macOS | `kern.boottime` if readable, else zero | 0 | `sysctl(KERN_PROC_PID).p_starttime` (allowed under Seatbelt [S]) | `kill(pid, 0)` returning `EPERM` → `Alive`; unreadable → `Unknown` |
| Windows | boot time derived from the system uptime | session id | `GetProcessTimes` creation time | `OpenProcess` returning `ACCESS_DENIED` → `Alive` (`srt-win` principal) |

- **`HolderId` goes into format v1 now.** It is part of lease records and of the LOCK holder diagnostics at offset 2048 [AR §4.1], and [60 M0] freezes the format before any byte is written. Adding it later would be a format-v2 change [I].
- **Transport contract** (§2.6 T1–T8) is OS-independent and has one set of golden files.
- **Store placement.** Writes only under `<git-common-dir>/moirai/`, or `.moirai/` outside a repository. Temp files inside the store. No per-user writable state on the command path. Refuse network filesystems, **including WSL2's `/mnt/<drive>` (9P/drvfs) and `\\wsl$`/`\\wsl.localhost` paths**: POSIX locks taken inside WSL do not exclude a Windows moirai process on the same store [I; verify]. Also refuse cloud-sync roots on macOS (iCloud's `~/Library/Mobile Documents`, File Provider folders) [I].
- **New gate G-X (cross-OS format identity), from M1, on every PR.** Each runner writes a seeded store and uploads it as an artifact. Each of the other runners opens it, replays the model's expectations and runs `doctor --verify` and `--fsck`. The canonical commit ids must be byte-identical across OSes.

---

## 9. Proposed edits to the design of record

| Document | Edit |
|---|---|
| [AR §7.1] | Replace the argv conventions with §2.6 T1–T8. Say "`#N` only in stdin, files and MCP" and "the skill never teaches `scope=#88`". Add the dash `CTLMBCHAR` case to the stdin rule. |
| [AR §4.1, §4.10] | Replace "Unix builds swap `fdatasync`, `fcntl` byte locks and `mmap`" with a per-OS primitive table owned by the storage lens. Add directory fsync on Unix. Refuse 9P/drvfs and `\\wsl$` paths and macOS cloud roots. Temp files inside the store. |
| [AR §6.2] | Liveness becomes tri-state on `HolderId` (§8). `Unknown` defers to TTL. The format reserves `HolderId` in lease records and LOCK diagnostics. |
| [AR §6.1] | The leader is an optimisation only. Sandboxed Linux and macOS clients and Windows `srt-win` clients cannot reach it. UDS rendezvous connects relative to the store directory because of `sun_path` limits (104 on macOS, 108 on Linux) [I]. |
| [AR §7.5], [07 §9.5], [40 §6.4] | Hooks and MCP: exec form with `${CLAUDE_PLUGIN_DATA}/bin/moirai[.exe]`. SessionStart writes `PATH` to `CLAUDE_ENV_FILE`. No plugin `bin/`. `moirai doctor hooks` checks resolution from both the hook and the Bash tool. |
| [60 M0 item 7] | Infrastructure covers all three OSes: hosted runners (Linux x64/arm64, macOS arm64 and Intel, Windows x64/arm64); Linux crash layers (LazyFS, dm-log-writes, dm-flakey, QEMU/KVM loop); a macOS hardware decision; the Windows rig choice (VirtualBox on Home vs Hyper-V on Pro). |
| [60 measurements 11, 17] | Measurement 11: spawn floors per OS, including through Claude's zsh/bash wrapper with a realistic snapshot, plus flush floors (`fdatasync`, `F_FULLFSYNC`, `NtFlushBuffersFileEx`). Measurement 17: calibration results (a)(b)(c) per rig (§7.5). Add an allocator × libc RAM/speed measurement (§4.2). |
| [60 §3.13] | GT4 per OS via `TestHost`. GT12 over every shell pair (§2.6). GT15 per OS (§7.6). New gates: **LazyFS W2 loop** and **dm-log-writes W3 replay** (Linux, nightly from M1), **G-X cross-OS format identity** (PR from M1). |
| [60 risk register] | Add the Linux PID namespace, macOS Dock `PATH`, Windows `srt-win` principal, dash 0.5.13, and "hosted Windows runners have Defender disabled". |

---

## 10. Risks

| Risk | Likelihood / impact | Mitigation |
|---|---|---|
| A lease is reclaimed from a live holder because a sandboxed Linux PID was compared across namespaces | high if unaddressed / severe (double assignment) | `HolderId` with `ns_id`; `Unknown` never reclaims (§8) |
| Hooks or MCP fail to start on a Dock-launched macOS Claude Desktop because `moirai` is not on `PATH` | high / medium (silent fail-open hooks) | absolute exec-form path; `CLAUDE_ENV_FILE` `PATH` export; `doctor hooks` |
| A writer cannot write the store under the sandbox when the session cwd is below the repo root | medium / medium | exit 7 with the exact `allowWrite` entry; readers unaffected |
| `srt-win` ships and sandboxed Windows clients become another principal | medium (the code is already bundled) / high | the §8 principles; ACL inheritance test; pipe DACL fallback to direct mode |
| A VM crash rig is over-trusted for flush placement (W3) | medium / severe | calibration (c); dm-log-writes on Linux; the simulator for W3 on all OSes |
| The musl allocator makes packs or queries slow; mimalloc raises MCP RSS | medium / medium | the M0 allocator × libc measurement; one allocator on all OSes |
| zsh `NOMATCH` aborts agent commands that contain `?`, `*` or `[` | medium / low | T3; the error names the cause (the zsh message is clear) |
| dash `CTLMBCHAR` corrupts UTF-8 in `sh` hooks or CI steps | low (Debian stable ships 0.5.12) / medium | exec form; invalid-UTF-8 rejection |
| The macOS OS-crash gate cannot run for lack of hardware | medium / medium | cloud Mac at milestone exits; the simulator carries W2–W5 |
| Hosted Windows runners (Defender off) give falsely good budget numbers | high / low (known) | budgets gated only on the test host and the owner's machine [60 §5.1] |

---

## 11. Open questions for the owner

1. **Which architectures are first-class?** x64 and arm64 on all three OSes, or arm64-only on macOS given that macOS 26 is the last Intel release [C]?
2. **Public or private repository?** Public makes all standard hosted runners free, macOS included. Private costs about $270/month at the §5.2 volumes, or needs self-hosted runners.
3. **macOS hardware.** Buy a Mac mini (≈ $599) as a self-hosted runner and VZ crash rig, rent cloud Macs only at milestone exits, or accept a lower macOS OS-crash volume?
4. **Dedicated Windows test host.** Windows 11 Pro (Hyper-V, `Stop-VM -TurnOff`) or Home (VirtualBox)? Should the laptop stay free of WSL2 while it hosts the VirtualBox rig?
5. **Code signing.** Pay for the Apple Developer Program ($99/year) and Artifact Signing ($9.99/month; individuals only in the US and Canada), or keep moirai personal (ad-hoc signing and build-from-source)?
6. **Linux scope.** Which distributions and filesystems are certified: ext4 only, or ext4 + xfs + btrfs? Does Alpine or NixOS matter (static musl covers them)?
7. **WSL2 as a supported environment.** Claude Code's sandbox on Windows currently means running Claude Code inside WSL2. If the owner might do that, is a store on a WSL-side ext4 checkout supported, and is a store on `/mnt/d` refused (recommended)?
8. **Gate parity.** Should Linux and macOS carry the same GT15 cycle volumes as Windows at M1 exit and at release, or reduced volumes with the simulator as the primary gate?
9. **Human shells.** Does the owner use zsh with `EXTENDED_GLOB`, or fish, on a Mac or Linux box? This decides how much the human documentation must spell out.

---

## 12. Sources

**Claude Code**
- Hooks reference (exec vs shell form, `sh -c`, env, `CLAUDE_ENV_FILE`, no TTY): https://code.claude.com/docs/en/hooks
- Sandboxing (write rules, worktrees, protected paths, Seatbelt/bubblewrap, Unix sockets, escape hatch): https://code.claude.com/docs/en/sandboxing
- Settings reference (`allowUnixSockets`, `allowAllUnixSockets`, `allowWrite`, managed paths): https://code.claude.com/docs/en/settings-reference · Settings: https://code.claude.com/docs/en/settings
- Plugin manifest reference (`CLAUDE_PLUGIN_DATA`, `bin/`, `${user_config.*}`): https://code.claude.com/docs/en/plugins-reference
- Environment variables (`CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`): https://code.claude.com/docs/en/env-vars
- Claude Code 2.1.281 bundled code, read locally: shell selection, wrapper, snapshot, sandbox helpers [S] (excerpts in the probe folder)
- Issues: https://github.com/anthropics/claude-code/issues/44649 · https://github.com/anthropics/claude-code/issues/42248 · https://github.com/rtk-ai/rtk/issues/685
- sandbox-runtime source (`linux-sandbox-utils.ts`: `--unshare-pid`, `--proc`; `macos-sandbox-utils.ts`: `same-sandbox`, `system-socket`; `windows-sandbox-utils.ts`: `srt-sandbox` user): https://github.com/anthropic-experimental/sandbox-runtime

**Shells**
- zsh options (`NOMATCH`, `INTERACTIVE_COMMENTS`, `EXTENDED_GLOB`, `EQUALS`, `BARE_GLOB_QUAL`): https://zsh.sourceforge.io/Doc/Release/Options.html
- zsh expansion: https://zsh.sourceforge.io/Doc/Release/Expansion.html
- zsh source `Src/lex.c` (comment rule) and `Src/subst.c` (`~name`, `=cmd` errors): https://github.com/zsh-users/zsh
- dash 0.5.13 `CTLMBCHAR` heredoc bug (git mailing list, 2026-04/05): https://ratatoskr.run/git/2026/04/8033555/t · Debian dash versions: https://tracker.debian.org/pkg/dash
- macOS `/bin/sh`: https://scriptingosx.com/2020/06/about-bash-zsh-sh-and-dash-in-macos-catalina-and-beyond/

**Rust, allocators, time**
- Platform support: https://doc.rust-lang.org/nightly/rustc/platform-support.html
- musl 1.2.5: https://blog.rust-lang.org/2025/12/05/Updating-musl-1.2.5 · Rust 1.93: https://blog.rust-lang.org/2026/01/22/Rust-1.93.0/
- Rust std `fsync`/`datasync` on Apple (`F_FULLFSYNC`): https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs
- ripgrep jemalloc-on-musl rationale: https://github.com/BurntSushi/ripgrep/blob/master/crates/core/main.rs · musl allocator benchmark: https://raniz.se/blog/2025/rust-musl-malloc/
- libfaketime README (no static binaries): https://github.com/wolfcw/libfaketime · time_namespaces(7): https://man7.org/linux/man-pages/man7/time_namespaces.7.html

**Apple**
- Notarizing macOS software: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- Customizing the notarization workflow (no stapling for standalone binaries): https://developer.apple.com/documentation/security/customizing-the-notarization-workflow
- Sequoia runtime protection changes: https://developer.apple.com/news/?id=saqachfa
- `VZDiskImageSynchronizationMode`: https://developer.apple.com/documentation/virtualization/vzdiskimagesynchronizationmode
- Apple-silicon signing: https://eclecticlight.co/2020/08/22/apple-silicon-macs-will-require-signed-code/ · https://eclecticlight.co/2026/01/17/whats-happening-with-code-signing-and-future-macos/
- Intel and Rosetta timeline: https://appleinsider.com/articles/25/06/10/macos-27-will-be-the-last-operating-system-to-fully-support-rosetta-2 · https://www.macrumors.com/2025/06/10/apple-to-phase-out-rosetta-2/
- tart: https://github.com/cirruslabs/tart · UTM: https://github.com/utmapp/UTM

**Microsoft**
- SmartScreen reputation: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
- Artifact Signing FAQ: https://learn.microsoft.com/en-us/azure/artifact-signing/faq · product: https://azure.microsoft.com/en-us/products/artifact-signing · CA-rotation Q&A: https://learn.microsoft.com/en-us/answers/questions/5954185/artifact-signing-users-see-smartscreen-on-each-rel
- `Stop-VM -TurnOff`: https://learn.microsoft.com/en-us/powershell/module/hyper-v/stop-vm

**GitHub Actions**
- Hosted runners: https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- Runner pricing: https://docs.github.com/en/billing/reference/actions-runner-pricing · billing concepts: https://docs.github.com/en/billing/concepts/product-billing/github-actions
- 2026 pricing changes: https://resources.github.com/actions/2026-pricing-changes-for-github-actions/ · https://github.blog/changelog/2025-12-16-coming-soon-simpler-pricing-and-a-better-experience-for-github-actions/ · multipliers [C]: https://cicdpipelinecost.com/github-actions-pricing
- KVM on 2-vCPU Linux runners: https://github.blog/changelog/2024-04-02-github-actions-hardware-accelerated-android-virtualization-now-available/ · https://determinate.systems/blog/kvm-on-github-actions/
- runner-images Defender configuration: https://github.com/actions/runner-images/blob/main/images/windows/scripts/build/Configure-WindowsDefender.ps1

**Crash testing**
- dm-log-writes: https://docs.kernel.org/admin-guide/device-mapper/log-writes.html · dm-flakey: https://docs.kernel.org/admin-guide/device-mapper/dm-flakey.html · sysrq: https://docs.kernel.org/admin-guide/sysrq.html
- LazyFS: https://github.com/dsrhaslab/lazyfs · CrashMonkey: https://github.com/utsaslab/crashmonkey · ALICE: https://github.com/madthanu/alice · Chipmunk: https://github.com/utsaslab/chipmunk · log-writes tool: https://github.com/josefbacik/log-writes
- QEMU `-drive cache=`: https://www.qemu.org/docs/master/system/invocation.html
- VirtualBox `IgnoreFlush`: https://www.virtualbox.org/manual/ch12.html · Hyper-V coexistence [C]: https://www.kicksecure.com/wiki/VirtualBox/Green_Turtle_Issue
- VMware free for all: https://blogs.vmware.com/cloud-foundation/2024/11/11/vmware-fusion-and-workstation-are-now-free-for-all-users/
- Cloud Macs: https://www.scaleway.com/en/pricing/apple-silicon/ · https://aws.amazon.com/ec2/instance-types/mac/ · https://instances.vantage.sh/aws/ec2/mac2.metal

**Local probes** (not published): `probe_argv_posix.txt` (§2.3), `dash_ctlmbchar_probe.txt` (§2.4), `cc_wrapper_excerpt.txt` (§2.1), `forms.txt`, `payload.txt`, and the downloaded sources (`srt_*.ts`, `zsh_lex.c`, `zsh_subst.c`, `rust_fs_unix.rs`, `rg_main.rs`, `hooks.md`).
