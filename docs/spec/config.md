# config — Configuration

| | |
|---|---|
| Title | Configuration: the two configuration files, the git-config-style syntax, the value types, precedence and resolution, the unknown-key rule and the configuration diagnostics, how configuration is written, `HEAD.config_gen`, the registry format, and the complete key registry of [AR §13] |
| Chapter | [CFG], `docs/spec/config.md` |
| Status | draft, pass 1 pending |
| Work package | WP-18a, the configuration part of WP-18 ([PLAN §3.2] item 1), author role R-SPEC-F |
| Sources | [AR §13] (every paragraph and table: "Syntax", "Two scopes", "Precedence", "Typed registry", "Reload classes", the `hooks install` paragraph, "Sweep plan", the eleven key tables, "Policy data", "Never a key"); [AR §4.1] row `config`; [AR §4.2] `config_gen`; [AR §2.14] (discovery, `discovery.git-hint`); [AR §5b.8] (destinations); [AR §6.2], [AR §6.4], [AR §6.5], [AR §6.6]; [AR §7.1] (the `config` verb, `init`, exit codes); [AR §7.3]; [AR §7.4]; [AR §7.5] (hooks, `hooks.transport`); [74 §5.1]–[74 §5.4] (A10); [60 §2.5] rows "Store layout", "Store parameters", the audit rows "`HEAD`" and "Configuration", the "Cross-platform" row; [60 §3.1] "Decisions fixed at M0 exit" and the exit bullet "the configuration registry of [AR §13] specified"; [60 §3.14] (the 25 former owner questions); [60 §4.2], [60 §4.3], [60 §4.4] items 1 and 8; [40 §2.11] R-13 and R-14; [40 §2.4] (roots); [40 §5.1], [40 §5.3] (designated tree, expected ref); [40 §9.2]; [50 §3.10] item 10; [50 §5.8]; [50 §5.10]; [50 §6.5]; [80 §2.3.2] row "Store `config` rewrite"; [80 §2.12] rows "User-scope config", "Store `config` rewrite", "Hook and MCP entry point"; [80 §3.1] X-F11; [80 §3.2] row "Config syntax (git-config)"; [90 §2.1] (the `MOIRAI_*` names and their forbidden substrings); [90 §4.1] (context order, detection); [90 §5.2]; [90 §6.1]–[90 §6.5]; [90 §8.2]; [90 §10.5] P3, P6, P7; [90 §10.8]; [PLAN §3.2] WP-18, WP-90, WP-94; [PLAN §7] E8 |
| Depends on | [F01] (text, hexadecimal, reading rule of hand-editable files §6.7, holes §2.5); [F02] (the store `config` file §5.1, `tmp/` names §5.3, user-scope locations §7); [F17] (meaning, range, decision point and production value of every store parameter); cites [F04], [F05], [F08], [F12], [F14], [F15], [F16], [F18], [F19], [F20], [API], [LQ/std], [LQ/envelope], [LQ/errors], [OS/fs], [OS/path], [OS/mem], [OS/lock], [RULES/merge-table], [RULES/policy-keys] |

## 1. Scope

This chapter specifies moirai's configuration system. Every operational policy of moirai is a configuration key with a
documented default ([AR §13]); nothing in this chapter is an owner decision.

**Frozen at M0** ([AR §13], [60 §2.5] rows "Store layout" and "Configuration"): the syntax (§3), the value types (§4), the
precedence and resolution rules (§5), the unknown-key rule and its diagnostics (§6), `HEAD.config_gen` (§8) and the registry
format (§9). **Not frozen:** the key set, each key's type range and default, scope and reload class (§10). Later milestones add
keys without a format change; an older binary reports a newer key as unknown and otherwise ignores it (§6.1).

This chapter does not specify:
- the meaning, valid range and production value of a **store parameter** — [F17] is normative for those, and this chapter
  only registers the key ([F17 §1.2]);
- the locations of the two files — [F02 §5.1] (the store file) and [F02 §7] (the user file, frozen by [80] X-F11);
- the byte offset of `config_gen` in the `HEAD` slot — [F04];
- the durability-class sequence of the protocol points that rewrite a configuration file — [F16], which this chapter feeds
  (§7.4, §7.5);
- the numbering and exit mapping of error codes — [F19] and [LQ/errors]; §6.2 proposes the configuration diagnostics.

**Terms.**
- **Key**: a dotted name (`files.policy.auto`) that names one configuration value. A **key pattern** is a key with
  parameter segments written `<name>` (`roots.<name>`); a key that fills every parameter is an **instance** of its pattern.
- **Scope**: `store` or `user`, the one file a key is read from (§2.2).
- **Source**: where a value comes from — a flag, an environment variable, a file, the value recorded at creation, or the
  built-in default (§5.1).
- **Effective value**: the value a process uses after resolution (§5).
- **Registry**: the typed table of every key pattern that the binary carries (§9) and that §10 lists.
- **Policy data**: schema rows, versioned per branch, which [AR §13] lists beside the keys; they are not keys (§2.4, §10.13).

## 2. Files and scopes

### 2.1 The two configuration files

| File | Location | Holds | Written by |
|---|---|---|---|
| store file | `<store>/config` ([F02 §5.1]) | store-scope keys only | `init` (§7.6), `moirai config set\|unset` (§7.4), a hand edit |
| user file | `%APPDATA%\moirai\config` on Windows; `$XDG_CONFIG_HOME/moirai/config`, by default `~/.config/moirai/config`, on Linux and macOS; resolved by `user_config_path()` ([F02 §7], [OS/path §10]) | user-scope keys, unqualified or store-qualified (§2.3) | `moirai config set\|unset --user` (§7.5), `init` for one entry (§7.6), a hand edit |

- Both files use the syntax of §3 and the reading rule of [F01 §6.7] (§3.1).
- There is no third file: no system-wide file, no per-worktree file, no include directive (an `[include]` section is an
  unknown key, §6.1).
- The user-scope directory also holds `integrations`, the record of `moirai integrate` writes ([90 §3.5]). It is not a
  configuration file and this chapter does not read it.

### 2.2 Scopes

Each key has exactly one scope ([AR §13]):

- **store** — every key that changes versioned writes or behaviour shared by the processes of one store, so all writers of a
  store behave alike;
- **user** — machine-local paths and volumes, security policy for data leaving the machine, harness installation, and
  discovery. A store restored on another machine never carries them ([F02 §7.3] rule 3). A store-scope key cannot govern the
  discovery that finds the store ([AR §2.14]), so `discovery.git-hint` is a user key.

A store key found in the user file, or a user key found in the store file, is ignored and reported (`CFG06`, §6.2), with one
exception: a store key marked **user-lower** in the registry (`pack.cli.max-bytes`) may also appear in the user file, where it
can only lower the effective value (§5.1, "User-lower").

### 2.3 Store-qualified user entries

One user file serves every store of a user on a machine, but some user keys are per project: the main worktree
(`files.main-tree`), named roots (`roots.<name>`), an image destination's path. The user file therefore accepts
**store-qualified entries**:

```
stores.<store-id>.<key>
```

- `<store-id>` is the 32 lower-case hexadecimal digits of a store id ([F02 §4], [F01 §6.4]).
- `<key>` must be an instance of a user-scope key whose registry row is marked **q** (qualifiable), or of a user-lower
  store key (§2.2), whose qualified entry §5.1 reads first (spec sync 2b). `discovery.git-hint` is not qualifiable: it is
  read before a store is known.
- A qualified entry applies only to the store with that id, and only in a process that has discovered that store. For that
  store it outranks the unqualified entry of the same key (§5.1). Entries qualified for other stores are ignored silently.
- `stores.` followed by anything else, or a qualified store key that is not user-lower, is an unknown key (`CFG04`).
- No registry key begins with the segment `stores` (registry invariant RG-4, §9.2).

*(Informative)* `stores.0123456789abcdef0123456789abcdef.files.main-tree = D:/work/demo-trunk` names the trunk worktree of one
store, and `roots.notes = D:/notes` names the `notes` root for every store of the user.

### 2.4 What is not configuration

- **Policy data** — role policy, delete policies, per-kind merge auto-policies — are schema rows versioned per branch
  ([AR §7.3], [AR §13]). §10.13 lists them with their defaults. A configuration file line naming one is ignored and reported
  (`CFG11`); `moirai config set` refuses it (exit 2) and names the schema write that changes it ([F08]).
- **Caller context** — `MOIRAI_DIR` (store selection, [F02 §3.1]), `MOIRAI_BRANCH`, `MOIRAI_LEASE`, `MOIRAI_AGENT`,
  `MOIRAI_ROLE`, `MOIRAI_RUN` and `MOIRAI_MODEL` ([90 §2.1], [90 §4.1]) — are not configuration: they carry who calls and where,
  are resolved by [90 §4.1]'s order, and never appear in a configuration file. The environment variables that are
  configuration sources are exactly those §10.12 lists.
- **Never a key** — the values [AR §13] "Never a key" and [80] X-F11 exclude (§10.15).

### 2.5 Missing and unresolvable files

- A missing store file means every store key takes its non-file value (§5.1); `doctor` reports it (`CFG14`). `init` always
  creates the file (§7.6), and moirai never deletes or truncates it (every rewrite is a replace-rename, §7.4). This answers
  [F02] open point 16.
- A missing user file, or a user location that does not resolve ([F02 §7.2]), means every user key takes its non-file value;
  `doctor` reports an unresolvable location (`CFG13`).

## 3. Syntax (frozen)

### 3.1 File-level reading

A configuration file is read by the rule of [F01 §6.7]: one leading byte-order mark is skipped, a line ends with LF or CR LF,
and the last line may lack a line end. In addition:

- a file larger than 262,144 bytes is **malformed**;
- a CR not followed by LF, a NUL byte, or bytes that are not valid UTF-8, anywhere in the file, make the file **malformed**
  ([F01 §6.7]).

A malformed file is treated as empty: every key of its scope takes its non-file value, and the process reports `CFG01` on
every command (§6.3). Line-level errors (§3.5) never make the whole file malformed.

moirai writes a configuration file in UTF-8 without a byte-order mark, with LF line ends, the last line included
([F01 §6.7], [80 §3.2]).

### 3.2 Grammar

[RFC 5234] ABNF, with [RFC 7405]'s `%s` for case-sensitive strings. The grammar applies to each line after the file-level
reading of §3.1 has removed the byte-order mark and the line end. `UTF8-NONASCII` is [F02 §3.3]'s: a UTF-8 encoded scalar value
above U+007F.

```abnf
line          = *WSP [ ( header / entry ) *WSP ] [ comment ]
header        = "[" *WSP key-name *WSP "]"
entry         = key-name [ *WSP "=" *WSP value ]
key-name      = segment *( "." segment )
segment       = seg-first *63seg-char                 ; 1 to 64 bytes
seg-first     = ALPHA / DIGIT
seg-char      = ALPHA / DIGIT / "-" / "_"
comment       = ( "#" / ";" ) *comment-char
comment-char  = %x09 / %x20-7E / UTF8-NONASCII
value         = *( plain-char / quoted )               ; trailing WSP outside quotes is removed (§3.4)
plain-char    = WSP / %x21 / %x24-3A / %x3C-7E / UTF8-NONASCII
                                                       ; any printable byte except DQUOTE, "#" and ";"
quoted        = DQUOTE *( q-char / q-escape ) DQUOTE
q-char        = %x09 / %x20-21 / %x23-5B / %x5D-7E / UTF8-NONASCII
q-escape      = "\" ( "\" / DQUOTE )
WSP           = %x20 / %x09
```

- A line is at most 4,096 bytes, its line end excluded. A longer line is a malformed line (§3.5).
- A byte in 0x00–0x08, 0x0A–0x1F or 0x7F anywhere on a line makes the line malformed (NUL and a lone CR already make the file
  malformed, §3.1).

### 3.3 Key names

1. **Full key.** An entry before the first header of the file is a **top-level entry**; its `key-name` is the full key
   (`default-branch = main`). An entry after a header `[P]` has the full key `P.N`, where `N` is the entry's `key-name`
   (`[files]` + `policy.auto` → `files.policy.auto`). Headers do not nest: each header replaces the previous one.
2. **Case.** ASCII letters in a `key-name`, in headers and entries alike, are folded to lower case before any other rule
   applies, as git folds section and variable names. The canonical spelling of every key is lower-case; a key written
   otherwise is reported as a notice (`CFG15`). Consequently every parameter segment (a root name, a destination name, a role,
   a model family) is a lower-case word (§4.2).
3. **Limits.** A full key has at most 16 segments and 255 bytes. A longer key makes its line malformed.
4. **Repeated headers.** The same header may appear several times; its entries join one another.
5. **Matching.** A full key matches a registry pattern when both have the same number of segments, every fixed segment is
   equal, and every parameter segment is a valid value of its parameter's vocabulary (§4.2). By registry invariant RG-1 (§9.2)
   at most one pattern matches. A full key that matches no pattern, and is not a store-qualified entry (§2.3), is unknown
   (§6.1).

### 3.4 Values

1. **Delimiting.** The value starts after `=` and the white space that follows it. Outside double quotes, a `#` or `;` starts
   a comment that ends the value, and white space at the end of the value is removed. White space inside the value is kept
   byte for byte.
2. **Quoting.** A double-quoted part keeps every byte between its quotes, including leading or trailing white space, `#` and
   `;`. Several plain and quoted parts concatenate (`a"b c"d` is `ab cd`).
3. **Escapes** exist only inside quotes: `\\` is one `\` and `\"` is one `"`. Any other backslash inside quotes makes the line
   malformed. Outside quotes a backslash is an ordinary byte, so a hand-written Windows path `D:\notes` needs no escaping
   (open point 2).
4. **No continuation lines.** A value ends at its line end.
5. **Empty and absent values.** `key =` has the empty value. An entry with no `=` has no value: it means `true` for a `bool`
   key and is an invalid value (`CFG05`) for every other type.
6. **After delimiting** the value is checked against the key's type (§4). A value that fails is an invalid value (§5.2).

### 3.5 Malformed lines and headers

A line whose first byte other than white space is `[` is a **header line**; every other non-blank line that is not a comment
is an **entry line**.

| Case | Consequence | Report |
|---|---|---|
| an entry line that does not match the grammar or its limits (§3.2–§3.4) | the line is ignored | `CFG02` |
| a header line that does not match `header` (for example `[files] policy.auto = x`, or `[Files` without `]`) | the line and every entry line after it up to the next valid header are ignored, so its entries cannot fall under the previous header | `CFG03` |
| a malformed file (§3.1) | the whole file is ignored | `CFG01` |

A malformed line never changes how the other lines of the file are read. `moirai config set` keeps malformed lines byte for
byte when it rewrites a file (§7.3), but refuses a malformed file (§7.1).

### 3.6 Repeated keys

A key that occurs more than once in one file takes the value of its last valid occurrence (git's rule for single-valued
variables). Every earlier occurrence is reported (`CFG07`). No key is multi-valued; list types are written as one value (§4.1).

### 3.7 Example *(informative)*

```
# moirai store configuration
default-branch = main
[files]
	policy.auto = exact          ; the default, written out
	ignore = target/, node_modules/, build/
[store.checkpoint]
	ops = 4096
[image.dest.default]
	refs = "main,tags/*,lane/*"
```

The full keys are `default-branch`, `files.policy.auto`, `files.ignore`, `store.checkpoint.ops` and
`image.dest.default.refs`.

## 4. Value types (frozen)

### 4.1 The type vocabulary

A registry row names one type from this table, with its parameters (a range `[lo..hi]`, an enumeration, a vocabulary). Words
are compared after ASCII case folding; the canonical form is what `moirai config set` writes and what `config list` and the
`--json` `config` key print.

| Type | Accepted text | Canonical form | Notes |
|---|---|---|---|
| `bool` | `true`, `yes`, `on`, `1`; `false`, `no`, `off`, `0`; an entry with no `=` is `true`; the empty value is `false` | `true` or `false` | git's boolean spellings |
| `int[lo..hi]` | decimal digits, optionally followed by `e` or `E` and one or two digits (`2e6` = 2,000,000); a `-` sign only if `lo` < 0 | decimal digits, no leading zeros | the value must be an integer in `[lo, hi]`; `hi` ≤ 2^63 − 1 |
| `size[lo..hi]` | an `int` in bytes, optionally followed, with no white space, by one unit: `B` (×1); `k`, `K` or `KiB` (×1,024); `m`, `M` or `MiB` (×1,048,576); `g`, `G` or `GiB` (×1,073,741,824) | if the value is a non-zero multiple of 1,024, the number followed by the largest of `GiB`, `MiB`, `KiB` that divides it exactly (`64MiB`, `15625KiB`); else plain decimal (`24000`) | bytes; git's `k`/`m`/`g` suffixes are binary |
| `duration[lo..hi]` | decimal digits followed, with no white space, by one unit: `ms`, `s`, `m` (minutes), `h`, `d` | the number followed by the largest of `d`, `h`, `m`, `s`, `ms` that divides the value exactly (`15m`, `90d`, `2s`) | held in milliseconds; a bare number is invalid. This grammar differs from LQ's duration literal ([LQ/lexical §5.6]: `s m h d w`, no `ms`) on purpose: configuration needs millisecond waits and no weeks, query text the reverse. The two never meet as text; the Store API passes durations as integer milliseconds and accepts LQ's form ([API §5.1]; pass 1, A1-55) |
| `percent[lo..hi]` | decimal digits, optionally followed by `%` | decimal digits | an integer percentage |
| `enum(a\|b\|…)` | one listed word | the word | |
| `set(a\|b\|…)` | listed words separated by `,`, each trimmed of white space; the empty value is the empty set | the members in the listed order, joined by `,` | a repeated member is invalid |
| `words` | open-vocabulary words (§4.2) separated by `,`, each trimmed | sorted bytewise, joined by `,` | a repeated word is invalid; role sets use it |
| `word` | one open-vocabulary word (§4.2) | the word | |
| `path` | an absolute path: on Windows `X:` followed by `/` or `\` and the rest, or a UNC path `//server/share/…` or `\\server\share\…`; on Linux and macOS a leading `/`; no empty (except a trailing one), `.` or `..` segment; valid UTF-8 without C0 controls | [80 §2.10] P12's machine-local form: `/` separators, an upper-case drive letter, no trailing `/` | `moirai config set` makes a relative argument absolute against the working directory before it validates it (the CLI boundary of [OS/path]); a path of another OS's form is invalid on this OS |
| `glob-list` | items separated by `,`, each trimmed; an empty item is invalid; the empty value is the empty list | the items in written order, joined by `,` | the pattern language is the key's (§10); order is kept because gitignore patterns may negate earlier ones; an item cannot contain `,` |
| `url-list` | items separated by `,`, each trimmed, non-empty, without white space | written order, joined by `,` | the matching rule is [F14]'s |
| `ref` | a moirai ref name ([F12]) | as written, after [80] X-F9's NFC normalisation of ref-name input | |
| `git-ref` | a git branch name, short (`main`) or full (`refs/heads/main`), valid by git's ref-name rules | as written | |
| `family` | a `word`, or the reserved word `unknown` | as written | §4.3 |

A value's canonical form is at most 4,000 bytes; `moirai config set` refuses a longer one (exit 2).

### 4.2 Parameter vocabularies

Every parameter segment of a key pattern, and every `word`, is an **open-vocabulary word**: 1 to 64 bytes of lower-case ASCII
letters, digits, `-` and `_`, beginning with a letter or a digit (the `segment` production of §3.2 after case folding).

| Parameter | Vocabulary | Reserved values |
|---|---|---|
| `<name>` in `roots.<name>` | a root name: exactly [F08 §5.4.1]'s grammar, 1–64 bytes of `[a-z0-9_-]` **starting with a letter** ([40 §2.4]; pass 1, S1-39, [F08] open point 38). A `roots.<name>` whose name starts with a digit, `_` or `-` matches no pattern and is an unknown key (§6.1) | `project`, `abs` (built-in roots; `roots.project` and `roots.abs` are unknown keys) |
| `<name>` in `image.dest.<name>.*` | a destination name | none; `default` is the destination used without `--to` (§10.10) |
| `<role>` | a role name ([AR §7.3]); roles are schema data, so any word is accepted, and a role no schema defines has no effect | none |
| `<client>` | `claude`, `codex`, `generic` (Tier B names only if owner decision #45 builds them, [90 §6.4]) | — |
| `<kind>` | a node kind name ([F08]) | none |
| `<family>` | a model family name (§4.3) | `default`, `unknown` |
| `<profile>` | `gated`, `compatible`, `unknown` ([90 §8.2]) | — |
| `<store-id>` in `stores.<store-id>.…` | 32 lower-case hexadecimal digits | — |

### 4.3 Model family names

`<family>` in `lq.model-profile.<family>` and the value of `lq.model-profile.default.<client>` name a model family ([90 §8.2]).
A family name is derived from the model identifier a run node, lease, marker, `MOIRAI_MODEL`, `--model` or hook declares
([90 §4.1] Model row): ASCII lower-case it, replace every byte outside `a`–`z`, `0`–`9`, `-` and `_` by `-`, and cut it to its
first 64 bytes. *(Informative: `claude-opus-5-5` stays as it is; `GPT-5.6-Luna` becomes `gpt-5-6-luna`.)* The word `unknown`
as a value of `lq.model-profile.default.<client>` means "no measured family": the session's profile is `unknown`.

## 5. Resolution and precedence (frozen)

### 5.1 Sources and their order

For each key, a process takes the **first valid value** from this list ([AR §13] "Precedence"):

1. **A command-line flag** that the registry names for the key (§10.12), for that one command. For the MCP server, a tool-call
   parameter that carries the key's value (`budget`) is a flag for that call, and a flag in the server's own argv (`--client`,
   `--tools`) is a flag for the server's lifetime.
2. **An environment variable** that the registry names for the key (§10.12). An empty value counts as unset.
3. **The key's scope file:**
   - a store key: the store file;
   - a user key: the store-qualified entry for the discovered store (§2.3), then the unqualified entry.
4. **The value recorded at creation**, for keys of reload class `init` (§5.4): `HEAD`'s `InitParams` block ([F17 §2]) or the
   destination's record ([F14]). Once the store or destination exists, the recorded value is the only source of such a key:
   rules 1–3 and 5 no longer apply to it. At creation, rules 1–3 and 5 give the value that is recorded (`init --set`,
   `image export --create --object-format`, the store file, the default).
5. **The built-in default** of the registry (§10), which may be a rule (§9.1).

Two additions:

- **User-lower.** For a key marked user-lower (§2.2), a valid value in the user file (qualified first, then unqualified) is
  applied after rules 1–5 as `min(resolved value, user value)`.
- **Derived defaults.** A default written as another key (`mcp.result-max-bytes.claude` defaults to the value of
  `mcp.result-max-bytes`) uses that key's effective value.

The effective values of one process form one **snapshot**, taken when §5.4 says; a value never changes inside a command or an
MCP request.

### 5.2 Invalid values

- A **flag** with an invalid value refuses the command with exit 2 (a usage error, [AR §7.1]); flags are never skipped.
- An invalid **environment variable** is skipped: resolution continues with rule 3. It is reported on the command (`CFG12`,
  §6.3), because a later `doctor` run does not see the command's environment (open point 4).
- An invalid **file value** is skipped: resolution continues with rule 4 or 5, which is the default [AR §13] names ("an invalid
  value at read time falls back to the default"). It is reported by `config check` and `doctor` (`CFG05`).
- An unknown key is §6.1's.

### 5.3 Cross-key constraints

Some keys constrain one another: [F17 §3] C-1 to C-4 among store parameters, and the constraints of §10 (K-1 in §10.8). After
every key is resolved:

1. each constraint is evaluated on the effective values;
2. for each constraint that fails, every **tunable** key it names whose effective value is not its fallback takes its
   fallback, and `CFG09` is reported. The **fallback** of a tunable key is min(its default, the largest value that satisfies
   every constraint with the recorded `init` values and the other keys' effective values) (pass 1, P1-12). When every `init`
   key has its default the fallback is the default, which is [F17 §3]'s "a violating tunable value falls back to its
   production value"; a store created with other `init` values (the test profile's 64 KiB extent, [F17 §12]) gets the
   largest value its extent admits instead, so the fallback never breaks C-1;
3. registry invariant RG-2 (§9.2) guarantees that the defaults satisfy every constraint with the default `init` values, and
   §7.6 refuses an `init` whose values leave some tunable no admissible value, so one pass suffices.

`init` keys never fall back: their values are recorded and fixed ([F17 §2]). `moirai config set` refuses a value that would
make a constraint fail with the other effective values (exit 2, naming the constraint).

### 5.4 When values are read: the reload classes

| Class | CLI, hook or `moirai gc` process | MCP server | A later change takes effect |
|---|---|---|---|
| `hot` | read once, at the start of the process, after discovery (§5.5) | read at its first store open; re-read at the start of a request, and before maintenance between requests, when `HEAD.config_gen` differs from the value at its last read (§8.3), and at every later store open | at the next command; in the MCP server at its next request after `config_gen` changes, or its next store open |
| `restart` | as `hot` | read at server start only | when the MCP server restarts |
| `init` | read only when the store or the image destination is created; the value is recorded in `HEAD`'s `InitParams` ([F17 §2]) or in the destination ([F14]) | the recorded value | never: a different value needs a new store (filled by an image import, [F17 §2.2] IP-1) or a new destination ([AR §5b.8]) |
| `install` | read by `moirai integrate` and `moirai hooks install`, which render it into the harness's files | not read | at the next `integrate`/`hooks install`; `doctor hooks` and `integrate --check` report a difference between the rendered files and the effective value ([AR §13]) |

- A file value of an `init` key is never applied: `config check` and `doctor` report it as ignored when it differs from the
  recorded value (`CFG08`), and `moirai config set` refuses to change it once the store or destination exists (exit 2,
  [F17 §2.2] IP-4).
- A hand edit reaches a running MCP server only at its next store open, the next `config_gen` change or its restart:
  checking the files' identity per request would add syscalls to every request ([AR §13]: "zero extra syscalls per request";
  the stat at open is the design's). `moirai config set` or `unset` of any key, even to its current value, bumps
  `config_gen` (§7.4) and is the supported way to make running servers re-read (open point 10).

### 5.5 The reading sequence of a process

1. Resolve the user file's location ([F02 §7.2]) and read the user file (§3.1).
2. Resolve `discovery.git-hint` from rules 1, 2, the unqualified user entry and 5 of §5.1, and run discovery ([F02 §3]).
3. Read the store id from `HEAD` ([F02 §4]); select the store-qualified user entries for that id.
4. Read the store file.
5. Resolve every key the process uses (§5.1–§5.3). A process that finds no store (a `config` or `integrate` command run
   outside a store) resolves user keys only; store keys then have their defaults.

Reading and parsing both files costs two opens and two reads per process start ([60 §3.9]'s M8 open-count budget counts them).

### 5.6 The `config` key of `--json` results

A `--json v1` result carries the additive envelope key `config` ([AR §13]; [LQ/envelope §7.2] additivity) when at least one
key that the command read had a non-default effective value:

```
"config":[{"key":<string>,"value":<string>,"source":<string>}...]
```

- One object per such key instance, sorted bytewise by `key`.
- `value` is the canonical form (§4.1); `source` is one of `flag`, `env`, `store`, `user`, `user-qualified`, `init`.
- "Read" means that the command's code path consulted the key: the registry accessor records every key it returns, so the
  list is exact, not a guess.
- The key is absent when the list would be empty, so a default configuration adds no bytes.

## 6. The unknown-key rule and diagnostics (frozen)

### 6.1 The unknown-key rule

- **In a file** (store or user, hand-edited), an unknown key — a full key that matches no registry pattern (§3.3 rule 5) and
  is not a valid store-qualified entry — is a **warning**, never an error: the entry is ignored, the rest of the file applies,
  the command's exit code is unchanged (exit 0 for a successful command), and `moirai config check` and `doctor` list it
  (`CFG04`) ([AR §13]).
- **At `moirai config set` or `unset`** an unknown key is refused with exit 2 ([AR §13]: "`moirai config set` validates").
- **In the environment**, variables beginning `MOIRAI_` that neither §10.12 nor §2.4 names are ignored; `doctor` lists them
  as a notice (`CFG17`).
- A key that a later version adds is therefore readable by every earlier version without a format change: the earlier
  version warns and ignores it.

### 6.2 Configuration diagnostics

Each diagnostic has a code, a severity and one ASCII message line of at most 200 bytes; `<…>` are substitutions. In `--json`
they use [LQ/errors §4.3]'s object form (`{"code","name","message","detail"}`); in text they render as `<code>: <message>`,
continuation lines indented seven spaces, after the LQ warnings and notices of the footer ([LQ/errors §4.2]). They never
change an exit code ([LQ/errors §4.4]). [F19 §10.6] owns code numbering across the specification and freezes these numbers
and names (open point 21; pass 1, A1-39, A1-62).

| Code | Name | Severity | Message |
|---|---|---|---|
| CFG01 | `config_file_malformed` | warning | `config: <file> is malformed (<reason>); its <scope> keys use their defaults` — `<reason>` is `invalid UTF-8 at byte <n>`, `NUL at byte <n>`, `CR without LF at byte <n>` or `larger than 262144 bytes` |
| CFG02 | `config_line_malformed` | warning | `config: <file>:<line>: malformed line ignored` |
| CFG03 | `config_header_malformed` | warning | `config: <file>:<line>: malformed section header; lines <line>-<last> ignored` |
| CFG04 | `config_unknown_key` | warning | `config: <file>:<line>: unknown key <key> ignored` |
| CFG05 | `config_invalid_value` | warning | `config: <file>:<line>: invalid <type> for <key>; <source> value <value> used` |
| CFG06 | `config_wrong_scope` | warning | `config: <file>:<line>: <key> is a <scope> key; ignored here` (for a user-lower key: `…; above the store value, no effect`) |
| CFG07 | `config_repeated_key` | notice | `config: <file>:<line>: <key> is set again at line <m>; the last one applies` |
| CFG08 | `config_init_key_ignored` | warning | `config: <key> is fixed at creation (<value>); the file value is ignored` |
| CFG09 | `config_constraint` | warning | `config: <constraint> fails; <keys> use their defaults` |
| CFG10 | `config_retired_key` | warning | `config: <file>:<line>: <key> was removed` or `…: <key> is now <new key>` (§6.4) |
| CFG11 | `config_policy_data` | warning | `config: <file>:<line>: <name> is policy data, set by a schema write; ignored` |
| CFG12 | `config_env_invalid` | warning | `config: <VAR>=<value> is not a valid <type>; ignored` |
| CFG13 | `config_user_location` | warning | `config: no user configuration location (<reason>); user keys use their defaults` |
| CFG14 | `config_store_file_missing` | warning | `config: <store>/config is missing; store keys use their defaults` |
| CFG15 | `config_noncanonical` | notice | `config: <file>:<line>: <key> is written <spelling>` |
| CFG16 | `config_reload_not_signalled` | warning | `config: <file> updated, but running MCP servers of this store were not told (<reason>); they re-read at their next store open or restart` |
| CFG17 | `config_unknown_env` | notice | `config: <VAR> is not a moirai variable; ignored` |

### 6.3 Where diagnostics appear

| Diagnostic | Every command that reads the file or variable | `moirai config check` | `doctor` |
|---|---|---|---|
| CFG01, CFG12 | yes: one warning in the result (`warnings` in `--json`; stderr in text) | yes | yes |
| CFG16 | only on the `config set`/`unset` that could not signal | — | — |
| every other code | no | yes | yes |

The per-command reporting of CFG01 and CFG12 extends [AR §13], which lists warnings only in `config check` and `doctor`: a
whole file silently ignored, or a variable only the failing command's environment holds, would otherwise weaken behaviour
without trace ([80 §1] X5; open point 3).

### 6.4 Retired and reserved names

A retired name is never reused with another meaning. A retired name in a file is ignored and reported with `CFG10` instead of
`CFG04`; `config set` refuses it (exit 2) and names its successor.

| Name | Status | Successor or reason |
|---|---|---|
| `pack.cyrillic-weight` | removed ([90 §6.2]) | budgets are UTF-8 bytes |
| `pack.cli.max-chars` | renamed | `pack.cli.max-bytes` |
| `pack.mcp.max` | renamed | `pack.mcp.max-bytes` |
| `mcp.result-max-chars`, `mcp.result-max-chars.<client>` | renamed | `mcp.result-max-bytes`, `.<client>` |
| `output.nonzero-exit-max-chars`, `.<client>` | renamed | `output.nonzero-exit-max-bytes`, `.<client>` |
| `query.budget.default.chars` | renamed | `query.budget.default.bytes` |
| `tx.wmem-max` | renamed (pass 1, P1-11) | `query.caps.<role>.wmem`, with `query.budget.default.wmem` as the request (§10.5) |
| `files.usn` | removed with E2 ([74 A13], [40] R-13) | a later E2 build brings `files.journal` |
| `files.hooks.nudge` | removed with the move nudge ([74 A17]) | — |
| `files.budget.window` | never a key | the E6 window bound is an R-14 constant ([F20]) |
| `image.anchor-text` | renamed | `image.dest.<name>.anchor-text` |
| `policy.unleased-root-role` | withdrawn ([90 §10.8]) | unleased callers get the `general-purpose` row |
| `files.journal`, `hooks.post-tool-batch.delta`, `hooks.native-tasks-mirror` | reserved for features not built ([60 §3.14] #41, #18) | an unknown key until the feature is built; the name keeps its meaning |

## 7. Writing configuration

### 7.1 The `moirai config` verb

`moirai config get|set|unset KEY [VALUE] [--user|--store] | list [--effective|--defaults] | check` ([AR §7.1]).

| Form | Does | Exit codes |
|---|---|---|
| `config get KEY` | prints the effective value (§5) in canonical form; a key whose effective value is `none` prints nothing | 0; 3 when the effective value is `none`; 2 for an unknown key |
| `config get KEY --store` / `--user` | prints the value in that file (for `--user`, the store-qualified entry if the process discovered a store and one exists, else the unqualified entry) | 0; 3 when the file does not set the key; 2 for an unknown key |
| `config set KEY VALUE [--store\|--user]` | validates (§7.2) and writes (§7.3–§7.5); without a scope flag, the key's own scope | 0 (with `CFG16` if the reload was not signalled); 2 for an invalid request; 7 when the store file cannot be written or changed concurrently too often (§7.4) |
| `config unset KEY [--store\|--user]` | removes every occurrence of the key from that file | as `set`; 0 when the key was not set |
| `config list` | every entry set in the two files, one line each: `<key> = <canonical value> [<source>]`, where `<source>` is `store`, `user` or `user-qualified`; sorted by key, then source | 0 |
| `config list --effective` | every key instance the process can resolve — every non-parameterised key, and every instance of a pattern that some source sets — one line each: `<key> = <canonical value> [<source>]` with `<source>` from §5.6 plus `default` | 0 |
| `config list --defaults` | the registry (§9.3) | 0 |
| `config check` | every diagnostic of §6.2 for both files, the environment and the constraints, one line each in the order file, line; exits 0 whatever it finds ([AR §13]: warnings are exit 0) | 0 |

- `--json v1` gives the `data` shapes of §9.3 (`--defaults`) and `{"key","value","source"}` objects (`get`, `list`,
  `--effective`); `check` gives [LQ/errors §4.3] objects. The envelope and header are [F19]'s.
- `config set` of a store key and `config unset` of a store key need a discovered store; without one they exit 7 with
  [F02 §3.1] step 5's text. `--user` works without a store.

### 7.2 Validation by `config set`

`moirai config set` refuses with exit 2, writing nothing, when:
1. the key is unknown (§6.1), retired (§6.4) or policy data (§2.4);
2. the scope flag names the other scope, except `--user` for a user-lower key;
3. the value is invalid for the key's type and range (§4), or longer than 4,000 bytes, or contains LF;
4. the key is an `init` key of a store or destination that exists (§5.4; [F17 §2.2] IP-4);
5. the value would make a constraint fail (§5.3);
6. the target file is malformed (§3.1): the owner repairs it by hand first; `config check` names the reason.

A refusal names the rule, the key and, for rules 3 and 5, the valid range or the constraint, in one ASCII line. Its
[F19 §10.2] code is `config_key` for rules 1 and 2 and `config_value` for rules 3, 4 and 5 (an `init` key of an existing
store included). `moirai config unset` is checked by rules 1, 2, 4, 5 and 6, rule 5 on the value that applies without the
entry: an unset that would make a constraint fail is refused as a set is (spec sync 2b).

### 7.3 Textual edit rules

`config set` and `unset` edit the file as text, keeping every other line byte for byte (comments, blank lines, malformed lines
and their order). Let `K` be the full key, `P` its segments but the last, and `L` its last segment. For a store-qualified user
entry, `K` is the whole `stores.<id>.<key>` name.

- **Replace.** If `K` occurs (§3.3), its last occurrence is replaced by the line `<indent><name> = <v>`, where `<indent>` is
  that line's leading white space, `<name>` its `key-name` text as written, and `<v>` the canonical value, quoted when §7.3.1
  requires; a trailing comment on that line is dropped. Every earlier occurrence of `K` is deleted.
- **Append.** Otherwise:
  1. if `K` has one segment, the entry `L = <v>` is inserted as a top-level entry directly before the first header line, or at
     the end of a file that has no header;
  2. else, if a header `[P]` exists, the entry `TAB L = <v>` is inserted after the last entry that follows the last `[P]`
     header, or directly after that header if it has none;
  3. else the lines `[P]` and `TAB L = <v>` are appended at the end of the file.
- **Unset** deletes every entry line of `K`. Headers are kept, even when left empty (git's behaviour).
- **Line ends.** The rewritten file has LF line ends and no byte-order mark (§3.1): CR LF endings of kept lines become LF.

#### 7.3.1 Quoting on write

`<v>` is written inside double quotes, with `\` and `"` escaped (§3.4), when it is empty, begins or ends with white space, or
contains `#`, `;`, `"` or `\`; otherwise it is written as is. A `path` value has no `\` on write, because its canonical form uses
`/`.

### 7.4 Rewriting the store file, and the `config_gen` bump

Steps, in order. The durability classes are [80 §2.3.2]'s row "Store `config` rewrite"; [F16] owns the protocol point and
states it with [F15] OP-5's two-parent rule.

1. Validate (§7.2).
2. Read the store file's bytes `B0` (absent: empty). Compute `B1` by §7.3.
3. If `B1 = B0`, go to step 7.
4. Create `tmp/config.<nonce>` with create-new semantics ([F02 §5.3]), write `B1` in one write, and make it durable
   (`durable+meta`).
5. Read the store file again. If its bytes differ from `B0`, another process changed it: delete the temporary file and restart
   at step 2, at most twice; a third difference exits 7 (`config: the store file changed concurrently; retry`).
6. `rename_replace` the temporary file onto `config`, with the bounded share retry of [OS/fs §6.3], holding no lock byte
   ([OS/fs §6.3]: never under the writer or flush byte); then `durable-name` on the store directory and on `tmp/`.
7. **Bump.** Take the writer byte (waiting at most `lock.writer-wait-ms`, [F17 §10]), publish `HEAD` as a read-modify-write of
   the newest valid slot ([80 §2.4.3]) whose only change is `config_gen ← config_gen + 1` (wrapping at 2^32, §8.2), and release
   the writer byte. The publish appends no log record and is not flushed (§8.4).
8. Exit 0. If step 7 timed out or failed, the file change stands: exit 0 with `CFG16`.

Residual race (open point 9): steps 5 and 6 are not atomic, so two `config set` commands on one store in the same instant can
still lose one update; the loser's own step 5 cannot see it. Configuration writes are owner actions, rare and interactive;
`config check` after a set shows the effective file.

### 7.5 Rewriting the user file

The same steps as §7.4, with these differences (this resolves [F02] open point 15):
- The directory `<base>/moirai` is created if it is missing ([F02 §7.3] rule 5).
- The temporary file is `<base>/moirai/config.<nonce>`, `<nonce>` a `u64` from the OS's cryptographically secure random source
  (`Entropy::fill_random`, [OS/README §4.6]) in decimal ([F02 §5.3]'s rule). Before step 4, the process deletes every other entry of that directory named
  `config.<u64dec>` ([F02 §6.3]'s `u64dec`): only an interrupted rewrite leaves one, and a concurrent writer whose temporary file
  is deleted fails at step 6 and restarts at step 2.
- `durable-name` applies to `<base>/moirai` only.
- Step 7 bumps the `config_gen` of the store the process discovered, if any, so its running MCP servers re-read the user file;
  other stores' servers see the change at their next store open, `config_gen` change or restart. Without a discovered store
  step 7 is skipped silently.

### 7.6 `init` and the initial store file

`moirai init [--here | --link STORE] [--default-branch B] [--set KEY=VALUE]... [--force --shadow]`.

- **`--set KEY=VALUE`** (repeatable; open point 11) accepts a store key. An `init` key ([F17 §2]: `store.log-extent-bytes`,
  `store.hist-frame-commits`, `store.hist-frame-bytes`) goes into `HEAD`'s `InitParams` ([F17 §2.2] IP-5); any other store key
  is written into the initial store file. A user key, an unknown key or an invalid value refuses `init` (exit 2) before
  anything is created. So does a set of values that fails a constraint of [F17 §3] (C-1–C-4) with the given `--set` values
  and the defaults of every other key: `init` exits 2 with `config_value` naming the constraint (for example `C-1:
  store.commit.inline-max-bytes (1MiB) does not fit store.log-extent-bytes (64KiB)`), before anything is created (pass 1,
  P1-12). The test harness passes [F17 §12]'s full test profile this way, never an extent alone ([F17 §12] TP-2); the Store
  API equivalent is [API §8.1].
- **`--default-branch B`** is `--set default-branch=B`.
- **The `project` root's content-hash algorithm** is not a key and has no `--set`: `init` reads the object format of the
  store's repository (`extensions.objectFormat`; `sha1` when the store has no repository) and records it in `HEAD`'s
  `project_oid_algo` ([F04 §5.16], [F17 §2.1], [F20 §2.3]; pass 1, A1-15, S1-28, P1-4).
- **The initial store file** is `tmp/config.<nonce>` renamed into place as part of `init`'s initial set ([F02 §2.4]; [F16] fixes
  the sequence). Its bytes, in order:
  1. the line `# moirai store configuration (git-config syntax); moirai config list --defaults lists every key` and LF;
  2. the entries below, each placed by §7.3's append rule, applied in bytewise order of the full keys:
     - `default-branch = B` when `--default-branch` or `--set default-branch=…` was given;
     - `files.main-ref = <branch>` when `init` runs inside a git repository whose working tree has a checked-out branch
       (rule `main-ref`, §10.1): the short name of the branch that tree's `HEAD` names;
     - every other `--set` store key.
- **Rule `main-tree`** (§10.1). When `init` runs inside a git repository in a tree other than the main worktree ([F02 §2.1]),
  it writes the user entry `stores.<store-id>.files.main-tree = <tree>` by §7.5 after the store is complete, `<tree>` being the
  canonical top level of the tree it ran in ([OS/path]). Failure to write it (no user location, a sandbox) is a notice, never a
  failure of `init`; without the entry the main worktree applies and `doctor` warns ([40 §5.1]).
- `init --link` writes no configuration.

## 8. `HEAD.config_gen` (frozen)

### 8.1 The field

`config_gen` is a `u32` field of the `HEAD` slot ([AR §4.2]), little-endian ([F01 §4.1]); [F04]'s offset table places it, and
this chapter defines no fixed-size structure of its own. It is store-local runtime state: not
versioned, not part of any canonical form, commit id, `state(ref)` digest or image, never exported and never compared by the
reference model ([60 §4.3]: `HEAD` bytes are out of the model's scope).

### 8.2 Values

- `init` writes 0 in both slots.
- `moirai config set` and `unset` add 1 modulo 2^32 (§7.4 step 7, §7.5). After `init`, nothing else changes it except
  `restore` and `repair` (§8.4).
- A reader compares values for **inequality** only: a changed value means "re-read", and wrap-around is harmless (2^32 bumps
  between two requests of one server do not occur).

### 8.3 Readers

The MCP server reads `HEAD` at the start of every request ([AR §4.2]). If the newest valid slot's `config_gen` differs from the
value the server recorded at its last configuration read, the server re-reads both configuration files (§5.5 steps 3–5) before
it serves the request, and records the new value. The same check precedes the server's maintenance between requests. CLI and
hook processes do not consult `config_gen`: they read the files at start (§5.4).

### 8.4 Preservation

- Every publish is a read-modify-write of the newest valid slot ([80 §2.4.3], X-F3), so every publish other than a bump copies
  `config_gen` unchanged. [F04] and [F16] state this for the field.
- The bump publish is not flushed. After an OS crash the on-disk slot may carry the old value; every MCP server has restarted
  by then and reads the files at its first store open, so no stale configuration survives.
- `restore` writes the `config_gen` of the backup's newest valid slot; `repair --rebuild-from-log` keeps the newest valid slot's
  value, or writes 0 when no slot is valid. Either value is correct, because a restored or repaired store is re-opened by
  every process (`HEAD.retired`, [F02 §3.6]).

## 9. The registry format (frozen)

### 9.1 A registry row

The binary carries one row per key pattern ([AR §13] "Typed registry"); `moirai-config` (M1) holds it as data, and the
documentation is generated from it. The fields:

| Field | Content |
|---|---|
| `key` | the key pattern (§3.3), parameters written `<name>` |
| `type` | a type of §4.1 with its parameters, spelled as in §4.1 (`int[1..8]`, `enum(exact\|strong)`, `set(heartbeat\|cursor\|session-mark)`) |
| `default` | one of: a canonical value; `none` (no value: the behaviour the key's documentation states for "unset" applies); `key:<other key>` (the effective value of another key); `rule:<rule name>` (a rule this chapter defines, named in the row); `per-param` (for a pattern with a parameter: a list of `<param value>:<default>` pairs and a final `*:<default>` pair for every other parameter value, each `<default>` being any form above except `per-param`) |
| `scope` | `store` or `user`, with the markers `q` (qualifiable, §2.3) and `user-lower` (§2.2) |
| `reload` | `hot`, `restart`, `init` or `install` (§5.4) |
| `env` | the environment variable, or none (§10.12) |
| `flags` | the flags that override the key, or none (§10.12) |
| `constraints` | the constraint ids the key takes part in (§5.3) |
| `doc` | one line of ASCII, at most 120 bytes |

### 9.2 Registry invariants

A unit test of `moirai-config` and `xtask coverage` check them.

- **RG-1.** No key instance matches two patterns (§3.3 rule 5).
- **RG-2.** Every default is valid for its type and range, and the defaults satisfy every constraint when every `init` key has
  its default; with other recorded `init` values §5.3's fallback applies (pass 1, P1-12).
- **RG-3.** Every key has exactly one scope; `q` appears only on user keys; `user-lower` only on store keys.
- **RG-4.** No key begins with the segment `stores`; no key reuses a retired name (§6.4).
- **RG-5.** Every environment variable begins `MOIRAI_`, is named by one key only, and contains none of `KEY`, `TOKEN`,
  `SECRET`, `PASSWORD` ([90 §2.1]: harnesses strip such variables from MCP server environments).
- **RG-6.** Every key of visibility class V (§9.4) has a model function in [RULES/policy-keys] ([PLAN §3.2] WP-90, WP-94), a
  rule file that R-MODEL writes in WP-90 and that the index lists as planned; the citation is made before the file exists as
  [F01 §2.2] allows (pass 1, A1-50).
- **RG-7.** Every key pattern is at most 255 bytes and 16 segments, and each fixed segment follows §3.2's `segment` rule in
  lower case.

### 9.3 `moirai config list --defaults`

**Text:** one line per row, in bytewise order of `key`; fields separated by two spaces:

```
<key>  <type>  default=<default>  scope=<scope>  reload=<reload>[  env=<VAR>][  flags=<flag>,<flag>]  # <doc>
```

`<default>` is written as in §9.1 (`default=16000`, `default=none`, `default=per-param(architect:24000,architecture-critic:24000,*:16000)`,
`default=key:mcp.result-max-bytes`, `default=rule:main-tree`); `<scope>` as `store`, `user`, `user,q` or `store,user-lower`.

**JSON** (`--json v1`): `data` is an array of one object per row, keys in this order:

```
{"key":<string>,"type":<string>,"default":<default>,"scope":<string>,"qualifiable":<bool>,"user_lower":<bool>,
 "reload":<string>,"env":<string or null>,"flags":[<string>...],"constraints":[<string>...],"doc":<string>}

<default> = {"kind":"value"|"none"|"key"|"rule","value":<string or null>}
          | {"kind":"per-param","params":[{"param":<string>,"default":<default without per-param>}...]}
```

`value` holds the canonical value, the other key's name or the rule's name, and is `null` for `none`. The `*` pair is the last
element of `params`.

### 9.4 The specification columns of §10

This chapter's registry tables carry, beside the binary's fields, three specification columns that the binary does not:

- **vis** — the key's visibility class:

| Class | Meaning | Who checks every allowed value |
|---|---|---|
| **V** | changes a `Store` API result, an exit class, a commit id or a `state(ref)` digest | the reference model, by the named function, over the sweep of §9.5 ([AR §13], [60 §3.14], [PLAN §7] E8) |
| **I** | changes only physical layout, time, caches or private memory ([F17 §1.5] class I) | the engine's invariance: two runs whose profiles differ only in class-I keys give the same results, commit ids and state digests ([F17 §1.5] SP-1, extended to every class-I key) |
| **Rs** | adds a resource-class refusal that depends on memory accounting or timing ([F17 §1.5] Rs) | GT2's refusal rule ([F17 §1.5] SP-2); GT3, GT4 |
| **B** | a budget: cuts a read deterministically (rows, bytes, work, `fs`, as-of ops) or by wall clock (deadlines and `-ms` keys), with exit 10, a cursor or `unverified` states | GT9 (budget replay, M7); the model computes only the effective budget ([60 §4.2]: "budgets not modelled") |
| **O** | changes output only: ceilings, pages, texts, rendering, the language, warnings | the golden files of GT12 and the reference renderer ([PLAN §3.2] WP-71a); CLI text is outside the model ([60 §4.3]) |
| **X** | changes harness integration or effects outside the store: hook registration and hook behaviour, harness files, the git index, remotes, discovery | GT12, `doctor hooks\|agents\|sandbox`, `integrate --check`, discovery tests ([F02 §3]) |

- **effect point** — where and when the effective value is used.
- **read by** — the engine component (a crate of [PLAN §2.2]–§2.3) and, for class V, the reference-model function. Model
  functions are written without the crate prefix `moirai_model::`; [RULES/policy-keys] (WP-90) binds them, each tagged
  `spec: [CFG §10.x]` ([60 §4.6]), and is normative for their names: this chapter's `read by` and `model` cells follow its
  `function` cells, and where they differ policy-keys wins (spec sync 2b). The model takes a configuration snapshot (a typed map from key
  instance to value) with each `Store` API command; it never parses a file.

### 9.5 Sweep plan

[AR §13] "Sweep plan", made exact:

- **Allowed-value sets** per type: `bool`: both; `enum`: every value; `set`: the empty set, each single member, the full set;
  `words`, `glob-list`, `url-list`: the default, the empty list and one non-default list; `int`, `size`, `duration`, `percent`:
  the default, the lower bound, the upper bound, and the [F17 §12] test value where one exists. The reference model runs the
  upper bound unclipped (a bound is a value, not a scale); only a checker whose run at the upper bound would exceed its own
  scale clips it to model scale ([60 §4.4] item 8) (spec sync 2b); `path`: set and unset; `family`, `word`: the default and one other value.
- **One at a time.** Every key is swept away from its default one at a time over its allowed-value set, on the production
  profile ([F17 §12] TP-1). Class V: the reference model and GT2; the other classes: the checker named in §9.4.
- **Pairs.** `files.policy.auto` × `files.deletion-inference`; `merge.strict` × `merge.policy.<kind>`; `hooks.transport` ×
  `files.hooks.edit-evidence`; `maintenance.rollup` × {`quiet.from-lane-measuring`, `quiet.tail-cap-multiplier`}
  ([AR §13]).
- **Hooks.** Each `hooks.<hook>.enabled` key is set to `false` one at a time, `moirai hooks install` runs, and `doctor hooks`
  must report no difference ([AR §13]).
- **Volume.** The full gate volume runs on the default configuration; store parameters are also swept over [F17 §12]'s test
  profile.

## 10. The key registry (not frozen)

Columns: **key**; **type** (§4.1); **default** (§9.1, in canonical form; `HOLE(…)` until WP-81a fills it, see the Holes
section and [F17]'s); **scope**
(`store`; `user`; `user, q`; `store, user-lower`); **reload** (§5.4); **vis** (§9.4); **effect point**; **read by** (§9.4);
**source**. Environment variables and flags are in §10.12.

### 10.1 Discovery, paths and roots

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `discovery.git-hint` | `bool` | `true` | user | hot | X | discovery step 4 ([F02 §3.1], [F02 §3.4]), before a store is known; `MOIRAI_GIT_HINT` uses the `bool` spellings (answers [F02] open point 19) | `moirai-app` discovery; [F02 §3] discovery tests | [AR §13], [AR §2.14] |
| `default-branch` | `ref` | `main` | store | hot | V | the last step of the branch order of [90 §4.1] (no explicit branch, lease, binding, marker or hint), per command or call | `moirai-vcs` context resolver; model `context::resolve_branch` | [AR §13], [AR §5a.4] |
| `roots.<name>` | `path` | `none`: the root renders `unmapped root` ([40 §2.4]) | user, q | hot | V | every mapping of a named root to a directory: resolution, settles, `link --at`, `file add --root` | `moirai-links` root table; model `links::root_dir` | [40] R-13, [40 §2.4] |
| `files.main-tree` | `path` | `rule:main-tree` (§7.6): the tree where `init` ran, recorded as a store-qualified entry when it is not the main worktree; unset: the main worktree, and `doctor` warns | user, q | hot | V | the designated tree of `main` at every re-bind, settle and file verb ([40 §5.3] item 1) | `moirai-links`; model `links::designated_tree` | [AR §13], [40 §5.1]; open point 6 |
| `files.main-ref` | `git-ref` | `rule:main-ref` (§7.6): the branch checked out where `init` ran, written into the store file by `init`; outside git: `none` | store | hot | V | the expected git ref of the main tree ([40 §5.3] item 2, I-F12) at every re-bind and settle | `moirai-links`; model `links::tree_gate` | [AR §13], [40] R-13 |
| `files.cloud` | `enum(metadata-only\|refuse)` | `metadata-only` | user, q | hot | V | link creation and every automatic resolution step under a cloud root ([40 §4.6], I-F11) | `moirai-links`, `os::project`; model `links::cloud_policy` | [AR §13], [40] #14 |

### 10.2 Store parameters

[F17] is normative for each key's meaning, valid range (by its row P01–P34), decision point, test-profile value and class
([F17 §1.2]); this table registers them. All are store-scope. Holes are [F17]'s.

| key | [F17] row; type | default | reload | vis | effect point | read by |
|---|---|---|---|---|---|---|
| `store.log-extent-bytes` | P01; `size` | `64MiB` | init | I | extent creation and rotation; recorded in `InitParams` ([F17 §4.1]) | `moirai-store` log |
| `store.log-active-extents` | P02; `int` | `4` | hot | I | each delta checkpoint: retirement ([F17 §4.2]) | `moirai-store` maintenance |
| `store.hist-frame-commits` | P03; `int` | `256` | init | I | retirement's frame rule ([F17 §4.3]) | `moirai-store` retirement |
| `store.hist-frame-bytes` | P04; `size` | `1MiB` | init | I | retirement's frame and split rules ([F17 §4.3]) | `moirai-store` retirement |
| `store.commit.inline-max-bytes` | P05; `size` | `1MiB` | hot | I; Rs for agent verbs | phase 1 of every write: the write-size switch ([F17 §4.4]) | `moirai-store` write path |
| `store.checkpoint.ops` | P06; `int` | `HOLE(F17-ckpt-ops)` | hot | I | delta-checkpoint trigger C1 ([F17 §5.2]) | `moirai-store` maintenance |
| `store.checkpoint.bytes` | P07; `size` | `HOLE(F17-ckpt-bytes)` | hot | I | trigger C1 | `moirai-store` maintenance |
| `store.checkpoint.body-bytes` | P08; `size` | `HOLE(F17-ckpt-body)` | hot | I | trigger C1 | `moirai-store` maintenance |
| `store.tail.max-overlay-bytes` | P09; `size` | `HOLE(F17-tail-overlay)` | hot | I | trigger C1's overlay clause ([F17 §5.2]) | `moirai-store` overlay |
| `store.tail.max-overlay-bytes.quiet` | P10; `size` | `HOLE(F17-tail-overlay-quiet)` | hot | I | the quiet cap ([F17 §5.3]) | `moirai-store` overlay |
| `quiet.tail-cap-multiplier` | P11; `int` | `HOLE(F17-quiet-mult)` | hot | I | the quiet cap ([F17 §5.3]) | `moirai-store` maintenance |
| `store.tail.runtime-bytes` | P12; `size` | `2MiB` | hot | I | runtime-only fold trigger C2 ([F17 §5.2]) | `moirai-store` maintenance |
| `maintenance.cli-threshold-multiplier` | P13; `int` | `2` | hot | I | the process-kind multiplier `m` ([F17 §5.2]) | `moirai-store` maintenance |
| `store.fold-width` | P14; `int` | `3` | hot | I | each delta checkpoint: tiered fold ([F17 §6.1]) | `moirai-store` maintenance |
| `maintenance.rollup-threshold` | P15; `percent` | `25` | hot | I | the rollup-due test ([F17 §6.2]) | `moirai-store` maintenance |
| `store.dict.train-sample-bytes` | P16; `size` | `4MiB` | hot | I | each rollup ([F17 §6.3]) | `moirai-store` rollup |
| `store.dict.retrain-growth` | P17; `percent` | `25` | hot | I | each rollup ([F17 §6.3]); registered here as [F17] OP-17-03 asks | `moirai-store` rollup |
| `store.fts.tier2-nodes` | P18; `int` | `20000` | hot | I | each delta checkpoint and rollup ([F17 §6.4]) | `moirai-store` producers ([50] F12) |
| `store.promotion.overlay-ops` | P19; `int` | `HOLE(F17-promo-ops)` | hot | I | after a `sync`, at each delta checkpoint, after a rollup ([F17 §7]) | `moirai-store` promotion |
| `store.promotion.overlay-bytes` | P20; `size` | `HOLE(F17-promo-bytes)` | hot | I | as P19 | `moirai-store` promotion |
| `store.promotion.age-checkpoints` | P21; `int` | `HOLE(F17-promo-age)` | hot | I | as P19 | `moirai-store` promotion |
| `store.kahn-fallback-edges` | P22; `int` | `1000` | hot | I | every validation of a candidate that adds precedence edges ([F17 §8.1]) | `moirai-graph` validator V03 |
| `store.suspect-budget` | P23; `int` | `10000` | hot | V | phase 1 of every write ([F17 §8.2]) | `moirai-graph` derived state; model `derived::affected_with_budget` |
| `store.image.loose-pack-threshold` | P24; `int` | `HOLE(F17-loose-pack)` | hot | I | each export run ([F17 §9.1]) | `moirai-image` export |
| `store.pack-objects-max` | P25; `int` | `65536` | hot | I | each object appended to a pack ([F17 §9.2]) | `moirai-image` export |
| `lock.writer-wait-ms` | P26; `int` | `HOLE(F17-lock-writer)` | hot | Rs | each blocking acquisition of the writer byte ([F17 §10], [OS/lock §4]) | `moirai-store` via `os::lock` |
| `lock.flush-wait-ms` | P27; `int` | `HOLE(F17-lock-flush)` | hot | Rs | each acquisition of the flush byte ([F17 §10]; [80] X-F11 registers it) | `moirai-store` via `os::lock` |
| `idempotency.retention` | P28; `duration` | `30d` | hot | V | each idempotency lookup ([F17 §11.1]) | `moirai-store`; model `idem::Table::lookup` |
| `idempotency.default-window` | P29; `duration` | `10m` | hot | V | each lookup under a default key ([F17 §11.1]) | `moirai-store`; model `idem::Table::lookup` |
| `gc.reflog-expire` | P30; `duration` | `90d` | hot | V | each `gc` run ([F17 §11.2]) | `moirai-store` gc; model `gc::reachable_after_gc` |
| `gc.cruft-delay` | P31; `duration` | `14d` | hot | V | each `gc` run ([F17 §11.2]) | `moirai-store` gc; model `gc::reachable_after_gc` |
| `gc.trash-expire` | P32; `duration` | `14d` | hot | I | each `gc` run ([F17 §11.3]) | `moirai-store` gc |
| `gc.fileobs-idle-expire` | P33; `duration` | `30d` | hot | I | each `gc` run ([F17 §11.3]) | `moirai-store` gc |
| `gc.delete-grace` | P34; `duration` | `1m` (60 s) | hot | I | each deletion decision ([F17 §11.4]); registered here as [F17] OP-17-03 asks | `moirai-store` maintenance, gc |

Constraints C-1 to C-4 are [F17 §3]'s; §5.3 applies them.

### 10.3 Durability, quiet mode, maintenance, leases

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `durability.lazy-kinds` | `set(heartbeat\|cursor\|session-mark)` | `heartbeat,cursor,session-mark` | store | hot | V | the append of each heartbeat, read-cursor and session-mark record: a kind in the set gets the lazy durability tag, else durable ([F05], [AR §6.5]); the key governs these three kinds only: graph mutations are always durable (a rule) and R4 runtime evidence is always lazy. Its effect shows only after an OS crash, which no `Store` API stream has ([API §6.7]): GT3 checks it, not GT2 | `moirai-store` append path; model `crash::survives` (what a crash may lose), consumed by GT3's crash harness only | [AR §13], [AR §6.5] |
| `quiet.from-lane-measuring` | `bool` | `true` | store | hot | V | every quiet-mode test: at maintenance decision points and at the verbs quiet mode refuses without `--force` ([AR §6.6]). Quiet mode is on when the explicit flag is set (`quiet on`, `HEAD.flags.quiet`) or when this key is true and a measuring lane exists: the explicit flag wins, so the key can only add quiet mode (spec sync 2b) | `moirai-store`, `moirai-app`; model `quiet::in_quiet_mode` | [AR §13], [AR §6.6] |
| `maintenance.rollup` | `enum(auto\|explicit)` | `auto` | store | hot | I | when a rollup is due ([F17 §6.2]) after a CLI write or an MCP request: `auto` spawns `moirai gc --rollup --if-needed`; `explicit` spawns nothing and `brief` and `doctor` warn (O) | `moirai-app` maintenance spawn | [AR §13], [AR §4.9] |
| `lease.ttl-default` | `duration[1m..30d]` | `15m` | store | hot | V | a `claim` without `--ttl`: the lease deadline, and the renewal-by-use threshold (half the TTL) ([AR §6.2], [90 §4.4]) | `moirai-graph` leases; model `lease::ttl_for` | [AR §13] |
| `lease.reclaim-older-than` | `duration[1m..3650d]` | `30m` | store | hot | V | `reclaim` given neither `--older-than` nor `--run`: the age bound | `moirai-graph` leases; model `lease::reclaim` | [AR §13], [AR §7.1] |
| `lease.orchestrator-ttl` | `duration[1m..30d]` | `12h` | store | hot | V | minting and renewal of an orchestrator session role lease that no slot anchors ([90 §4.3]) | `moirai-graph` leases; model `lease::ttl_for` | [90 §10.8] |
| `backup.max-age` | `duration[1h..3650d]` | `1d` | store | hot | O | `brief` and `doctor`: the age warning when the newest `Backup` record is older | `moirai-app` | [AR §13] |

### 10.4 Memory

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `mcp.overlay-bytes` | `size[0..256MiB]` | `4MiB` | store | hot | I | MCP server, at request end: branch overlays beyond the bound are evicted | `moirai-app` MCP front end, `moirai-store` overlay cache | [AR §13] |
| `mcp.overlay-bytes.<client>` | `size[0..256MiB]` | `per-param(codex:0,*:key:mcp.overlay-bytes)` | store | hot | I | as above, in a server whose client profile is `<client>`; 0 releases every mapping and overlay at request end ([90 §4.5]) | as above | [90 §10.8] |
| `mcp.overlay-lru` | `int[1..8]` | `8` | store | hot | I | MCP server overlay cache: count ceiling beside the byte bound | as above | [AR §13] (open point 22) |
| `git.delta-cache-bytes.cli` | `size[0..64MiB]` | `256KiB` | store | hot | I | git object layer, CLI and hook processes: delta-chain cache | `moirai-git` | [AR §13] |
| `git.delta-cache-bytes.mcp` | `size[0..64MiB]` | `1MiB` | store | hot | I | as above, MCP server | `moirai-git` | [AR §13] |
| `mem.rss-gate.cli` | `size[1MiB..1GiB]` | `4000000` | store | hot | Rs | a CLI or hook process: headroom = this + `mem.rss-gate.cli-per-view` × extra ref views − private bytes, at the start of every query (`mem`) and in phase 1 of every write (`wmem`) ([50 §5.10], [OS/mem §3]) | `moirai-lq`, `moirai-store` | [50 §5.10] "a named store parameter", [AR §8.3] RAM (open point 14) |
| `mem.rss-gate.cli-per-view` | `size[0..64MiB]` | `1MiB` | store | hot | Rs | as above | as above | as above |
| `mem.rss-gate.mcp` | `size[1MiB..1GiB]` | `15625KiB` (16,000,000 B) | store | hot | Rs | the MCP server: headroom = this − private bytes | as above | as above |
| `files.max-read-bytes` | `size[128KiB..4GiB]` | `16MiB` | store | hot | V | every project-file content read: a larger file is `Unavailable(size)` ([F20], [40 §2.5]) | `moirai-files` reader; model `links::content_available` | [40] R-13 |
| `files.max-line-hashes` | `int[1024..16777216]` | `65536` | store | hot | V | anchor resolution: beyond it, window-only anchors are `unverified (size)` ([F20]; never the similarity limit, [F20 §2.10.5]) | `moirai-files` anchors; model `anchors::window_available` | [40] R-13, [71] RAM-M4 |
| `files.deep.threads` | `int[1..64]` | `8` | store | hot | I | `links check\|sync --deep`: walk threads | `moirai-links` | [AR §13] |
| `files.deep.content-readers` | `int[1..8]` | `2` | store | hot | I | `--deep`: files read at once | `moirai-links` | [AR §13] |

When private bytes cannot be read (`MeterError`, [OS/mem §3]), `mem` and `wmem` are their requested values (the default or
the per-call raise, §10.5), as if the headroom were large (open point 15).

### 10.5 Queries and `TX`

**Budget keys** ([50 §5.10]). `query.budget.default.<b>` is the budget a query gets without a per-call raise;
`query.caps.<role>.<b>` is the ceiling up to which a caller whose presented lease has role `<role>` ([90 §4.1] rights
row) may raise it with `--budget` or the MCP `budget` parameter; an unleased caller has the role `general-purpose`. A
requested value above the ceiling is handled as [LQ/errors] states (open point 13). The effective `mem` is min(value,
headroom), never below 256 KiB (§10.4). **`wmem`** (pass 1, P1-11) is the write working set of phase 1 ([AR §4.5]
step 4, [F17 §4.4] W2): a budget like the others, requested by `query.budget.default.wmem` or a per-call
`--budget wmem=<v>` up to `query.caps.<role>.wmem`, and effective as max(256 KiB, min(requested, headroom)). It replaces
the former `tx.wmem-max` (§6.4), which could never bind under W2's old `min(1 MiB, headroom)`. A raise of `wmem` never
admits a larger changeset: an agent `TX` is bounded by `store.commit.inline-max-bytes` of changeset whatever its `wmem`
([F17 §4.4] W1, W4; at the production value ≈ 4,400–5,500 ops, below the default `tx.max-ops`). **Decided 2026-09-28**
(owner question OQ-P-2, option (a)): both values stay (P05 1 MiB, `tx.max-ops` 10,000), P05 bounds an agent `TX`, and
E501 names the split. Within that bound, a `TX` whose phase-1 working set exceeds the default 1 MiB needs
`--budget wmem=<v>` up to 4 MiB; E501 names the raise for `wmem` and the split for the inline bound ([LQ/errors §5.4];
pass 1, round 1). `<b>` and the defaults:

| `<b>` | type | `query.budget.default.<b>` | agent maximum ([50 §5.10]) = `query.caps.<role>.<b>` default for every role but `orchestrator` and `owner` | `query.caps.orchestrator.<b>`, `query.caps.owner.<b>` default (10 × the agent maximum, [AR §13]) |
|---|---|---|---|---|
| `work` | `int[1..10000000000]` | `2000000` | `20000000` | `200000000` |
| `mem` | `size[256KiB..1GiB]` | `1MiB` | 2 MiB in a CLI or hook process, 4 MiB in the MCP server (`rule:agent-max-mem`) | 20 MiB CLI, 40 MiB MCP (`rule:agent-max-mem` × 10) |
| `wmem` | `size[256KiB..1GiB]` | `1MiB` | `4MiB` ([AR §4.5] step 4, [50 §5.10]: the agent maximum of a write's working set) | `40MiB` |
| `rows` | `int[1..1000000]` | `50` | `500` | `5000` |
| `bytes` | `size[1000..10000000]` | `8000` | `24000` | `240000` |
| `visited` | `int[1..1000000000]` | `100000` | `1000000` | `10000000` |
| `refs` | `int[1..80]` | `4` | `8` | `80` |
| `fs` | `int[1..1000000000]` | `400` (provisional; recalibrated at M7, [50 §5.10]) | `10000` | `100000` |
| `deadline-cli` | `duration[100ms..1h]` | `2s` | `2s` (the deadline is not raised by agents, [50 §5.10]) | `20s` |
| `deadline-mcp` | `duration[100ms..1h]` | `5s` | `5s` | `50s` |

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `query.budget.default.<b>` (ten keys) | as the table above | as the table above | store | hot | B | the start of every query run: read verbs, `q`, `TX` evaluation, MCP `query`, the class queries of `pack` and `brief`; `wmem` at phase 1 of every write ([F17 §4.4] W2); [LQ/std §2.4]'s `BUDGET` classes derive `work` from this key (open point 18) | `moirai-lq` budgets, `moirai-store` write path; model `budget::effective` (arithmetic only) | [AR §13], [50 §5.10] |
| `query.caps.<role>.<b>` (ten per role) | as the table above | `per-param` as the table above | store | hot | B | a per-call raise by a caller of that role; `links check` runs its `fs` at `query.caps.orchestrator.fs` ([LQ/std §4.21]); under the test profile ([F17 §12] TP-3) `wmem`'s request is its cap | `moirai-lq`; model `budget::effective` | [AR §13], [50 §5.10] ([50] D6) |
| `query.safelist.<role>` | `enum(off\|named-only)` | `off` | store | hot | V | the binder, for a caller of that role: free-form LQ is refused with E406 ([LQ/errors]) | `moirai-lq` binder; model `profile::read_safelist` | [AR §13] ([50] D3) |
| `query.asof.max-ops.cli` | `int[0..10000000]` | `16000` | store | hot | B | as-of view construction in a CLI or hook process: beyond it E303 ([50 §5.8]) | `moirai-lq` planner | [AR §13], [50 §5.8] |
| `query.asof.max-ops.mcp` | `int[0..10000000]` | `100000` | store | hot | B | as above, MCP server | `moirai-lq` planner | as above |
| `files.read.max-uncached-ancestry` | `int[0..64]` | `1` | store | hot | B | every read path's git work: beyond it `unverified (git)` ([F20 §5.11]) | `moirai-links` | [AR §13], [40] R-13 |
| `files.read.max-e6-commits` | `int[0..4096]` | `32` | store | hot | B | as above, E6 commits per command | `moirai-links` | as above |
| `input.max-bytes` | `size[64KiB..1GiB]` | `16MiB` | store | hot | O | every text a command reads from stdin or `-f FILE` (LQ text, `apply` batches, bodies, quote files): read incrementally, and refused with exit 2 as soon as it exceeds this many bytes, before any budget applies ([LQ/lexical §8], [OS/shell §5.2]; pass 1, P1-39) | `moirai-app` CLI input | this chapter (P1-39) |
| `tx.max-statements` | `int[1..1000000]` | `1000` | store | hot | V | `TX` binding: a larger block is refused with E501 naming the split ([50 §3.10] item 10) | `moirai-lq` `TX`; model `budget::check_caps` | [AR §13] |
| `tx.max-ops` | `int[1..1000000]` | `10000` (a call may raise it to the agent maximum 50,000, [50 §5.10]) | store | hot | V | `TX` execution, as above; the count is the block's net ops, one per changed key ([F06 §7.8] NF-1; spec sync 2b) | `moirai-lq` `TX`; model `budget::check_caps` | [AR §13] |
| `tx.max-work-in-lock` | `int[0..1000000000]` | `500000` (provisional; M7's work-unit calibration sets it, [50 §3.10] item 10) | store | hot | I | re-validation under the writer byte: re-evaluate within this many units, else release and re-run phase 1 ([AR §4.5] step 7) | `moirai-lq`, `moirai-store` | [AR §13], [50 §3.10] |

### 10.6 File links (R4)

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `files.policy.auto` | `enum(exact\|strong)` | `exact` | store | hot | V | every automatic re-bind decision: `strong` also applies unique strong candidates, marked as guesses ([F20]) | `moirai-links`; model `links::auto_policy` | [40] R-13 ([40] #1) |
| `files.scratchpads` | `enum(refuse\|allow)` | `refuse` | store | hot | V | `link --at` and `file add` of a path in a session scratchpad | `moirai-links`; model `links::scratchpad_policy` | [40] R-13 ([40] #2) |
| `files.ignore` | `glob-list` (gitignore patterns) | `target/,node_modules/,build/` | store | hot | V | the ignore matcher of a tree with no git and no ignore file ([F20 §4.4]) | `moirai-files` matcher; model `links::ignored` | [40] R-13 |
| `files.read-budget-ms` | `int[1..60000]` | `20` | store | hot | B | every read path: the wall-clock safety net over the `fs` units; reported like E503 ([50 §5.10], [40] R-13) | `moirai-links` | [40] R-13 |
| `files.session-start-cap-ms` | `int[1..10000]` | `150` | store | hot | B | the `SessionStart` settle's hard cap ([AR §7.5]) | `moirai-app` hooks, `moirai-links` | [40] R-13 |
| `files.links-sync-ms` | `int[1..3600000]` | `2000` | store | hot | B | `links sync` without `--budget-ms` | `moirai-links` | [40] R-13 |
| `files.deep.budget-ms` | `int[1..3600000]` | `10000` | store | hot | B | `links check\|sync --deep` without `--budget-ms` ([40 §7.2]: "within the 10 s default budget"); registered here (open point 27) | `moirai-links` | [40 §7.2] |
| `mcp.links-sync-slice-ms` | `int[1..1000]` | `200` | store | hot | I | the MCP `links_sync` call's slice | `moirai-app` MCP front end | [AR §13] |
| `files.settle.others-after` | `duration[0..3650d]` | `1d` (24 h) | store | hot | X | `SessionStart`: links other than the brief's and the bound lane's are settled only when the last full settle is older | `moirai-app` hooks | [40] R-13 |
| `files.pending-escalate` | `duration[1h..3650d]` | `14d` | store | hot | O | rendering: a `pending` link older than this is marked per link | `moirai-app` rendering | [40] R-13 |
| `files.deletion-inference` | `enum(explicit\|main-tree-commits)` | `explicit` | store | hot | V | settles of the main tree: `main-tree-commits` records git deletions as `removed` (I-F7) | `moirai-links`; model `links::deletion_inference` | [40] R-13 ([40] #6) |
| `files.mv-git` | `bool` | `false` | store | hot | X | `file mv`: stage the rename with `git mv` (the git index is outside the model) | `moirai-app` file verbs | [40] R-13 ([40] #7) |
| `files.confirm-roles` | `words` (roles) | `orchestrator,owner` | store | hot | V | `links fix --confirm`: who may confirm an agent's guess; never the acceptor | `moirai-links`; model `links::confirm_rights` | [40] R-13 ([40] #16) |
| `files.portable-names` | `enum(refuse\|warn)` | `refuse` | store | hot | V | `file mv` (refuse or warn) and `link`, `file add` (warn) on names some OS cannot hold ([80 §2.10] P5) | `moirai-links`, `os::path`; model `links::portable_name_policy` | [40] R-13, [80] X-F11 |
| `files.hooks.evidence` | `bool` | `true` | store | hot | X | `hooks install` registers the `mv`/`rm` evidence hook only when true; the hook also reads the key when it fires and exits when false | `moirai-app` hooks, integrate | [40] R-13 ([40] #4) |
| `files.hooks.edit-evidence` | `enum(auto\|on\|off)` | `auto` (on with the `mcp_tool` transport, off with command hooks) | store | hot | X | as above, for the `Write\|Edit` hook | `moirai-app` hooks, integrate | [40] R-13 ([40] #4) |

### 10.7 Hooks

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `hooks.transport` | `enum(auto\|mcp\|command)` | `auto` (`mcp_tool` in Claude Code and Codex, command hooks elsewhere) | user, q | install | X | `integrate` and `hooks install`: the handler kind of every hook ([AR §7.5]) | `moirai-app` integrate | [AR §13] |
| `hooks.session-start.enabled`, `hooks.user-prompt-submit.enabled`, `hooks.subagent-start.enabled`, `hooks.agent-launched.enabled` (Claude Code only), `hooks.subagent-stop.enabled`, `hooks.stamp.enabled` (Claude Code only) | `bool` | `true` each | store | install | X | `hooks install` registers exactly the enabled hooks and removes the others; `doctor hooks` reports any difference ([AR §13]) | `moirai-app` integrate; GT12, `doctor hooks` | [AR §13] |
| `hooks.session-start.settle` | `bool` | `true` | store | hot | X | `SessionStart`: whether it settles links | `moirai-app` hooks | [AR §13] |
| `hooks.session-start.path-export` | `bool` | `true` | user, q | hot | X | `SessionStart`: append a `PATH` export to `CLAUDE_ENV_FILE` ([80 §2.12]) | `moirai-app` hooks | [AR §13], [80] X-F11 |
| `hooks.session-start.worker-pack` | `bool` | `true` | store | hot | X | `SessionStart` of a dispatched worker (`MOIRAI_LEASE` or `MOIRAI_RUN` set): the role pack instead of the brief ([90 §7.5]) | `moirai-app` hooks | [90 §10.8] |
| `hooks.session-start.orchestrator-lease` | `bool` | `true` | store | hot | X | `SessionStart` of a main session: mint the orchestrator's session role lease ([90 §4.3]) | `moirai-app` hooks | [90 §10.8] |
| `hooks.subagent-start.auto-sync` | `bool` | `true` | store | hot | X | `SubagentStart`: auto-apply a clean `sync` preview (D5) | `moirai-app` hooks | [AR §13] |
| `hooks.sync-auto-keys` | `int[0..1000000]` | `2000` | store | hot | X | `SubagentStart`: the largest preview auto-applied; 0 never applies | `moirai-app` hooks | [AR §13] |
| `hooks.stamp.permission` | `enum(allow\|ask)` | `allow` | user, q | hot | X | the stamp hook's `permissionDecision` ([AR §7.5]) | `moirai-app` hooks | [AR §13] ([AR §11] #6) |
| `hooks.stamp.ask-for` | `set(owner-authority\|edge-delete\|links-confirm)` | `owner-authority` | user, q | hot | X | the writes for which the stamp answers `ask` | `moirai-app` hooks | [AR §13] (#6) |
| `hooks.delta.max-commits` | `int[1..1000000]` | `2000` | store | hot | O | `UserPromptSubmit` delta: commits scanned before `N older changes` | `moirai-app` hooks | [AR §13], [AR §6.3] |

### 10.8 Agent tokens and output

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `pack.budget.<role>` | `size[1000..1000000]` | `per-param(architect:24000,architecture-critic:24000,*:16000)` (provisional; set at M9 by the recorded-dispatch test, [AR §7.4]) | store | hot | O | `pack` without `--budget` | `moirai-app` pack | [AR §13], [73 F2] |
| `pack.cli.max-bytes` | `size[1000..28000]` | `24000` | store, user-lower | hot | O | every CLI pack without `-o FILE`: the rendered text never exceeds it | `moirai-app` pack | [AR §13], [73 F1] |
| `pack.mcp.max-bytes` | `size[1000..48000]` | `25000` | store | hot | O | every MCP pack: min(this, the profile's MCP result ceiling) | `moirai-app` pack | [AR §13], [90 §10.8] |
| `pack.quota.c2`, `.c3`, `.c4-dev`, `.c4-critic`, `.c5` | `percent[0..100]` | `15`, `20`, `30`, `40`, `10` | store | hot | O | pack fill step 3: minimum share per class ([AR §7.4]); constraint K-1 | `moirai-app` pack | [AR §13] |
| `pack.staleness-notice` | `enum(off\|ids\|lines)` | `lines` | store | hot | O | `complete` and `apply` given a pack digest ([AR §6.2]) | `moirai-app` | [AR §13] |
| `brief.budget` | `size[1000..9500]` | `8000` | store | hot | O | `brief` without `--budget`; the `SessionStart` brief | `moirai-app` brief | [AR §13] |
| `brief.lang` | `enum(en\|ru)` | `en` | user, q | hot | O | the language of `brief`'s fixed texts | `moirai-app` rendering | [AR §13] ([AR §11] #13) |
| `hooks.subagent-start.budget` | `size[1000..10000]` | `3000` | store | hot | O | the `SubagentStart` role pack and a dispatched worker's `SessionStart` pack | `moirai-app` hooks | [AR §13], [90 §10.8] |
| `hooks.delta.budget` | `size[100..10000]` | `600` | store | hot | O | the `UserPromptSubmit` delta | `moirai-app` hooks | [AR §13], [90 §10.8] |
| `mcp.always-load` | `set(brief\|pack\|get\|query\|changes\|branch\|claim\|complete\|remember\|write)` | empty | store | restart | X | `tools/list`: tools not deferred | `moirai-app` MCP front end | [AR §13], [AR §7.2] |
| `mcp.result-max-bytes` | `size[1000..48000]` | `25000` | store | hot | O | every MCP result ([90 §6.4]) | `moirai-app` MCP front end | [AR §13], [90 §10.8] |
| `mcp.result-max-bytes.<client>` | `size[1000..48000]`; for `codex` at most 36,000 ([90 §10.8]) | `per-param(codex:HOLE(CFG-codex-mcp-result),*:key:mcp.result-max-bytes)` | store | hot | O | as above, under profile `<client>` | as above | [90 §6.4], [90 §10.5] P3 |
| `output.nonzero-exit-max-bytes` | `size[1000..10000]` | `8000` | store | hot | O | stdout of a CLI result with a non-zero exit ([AR §7.1]) | `moirai-app` rendering | [AR §13] |
| `output.nonzero-exit-max-bytes.<client>` | `size[1000..10000]` | `key:output.nonzero-exit-max-bytes` | store | hot | O | as above, under profile `<client>` | as above | [90 §10.8] |
| `mcp.ids-page-bytes` | `size[1000..25000]` | `8000` | store | hot | O | id-dense MCP output pages | `moirai-app` MCP front end | [90 §10.8] |
| `output.ids-max-bytes` | `size[0..1000000000]`; 0 = unlimited | `24000` | store | hot | O | the `--ids` page ([LQ/envelope], [90 §2.1]) | `moirai-app` rendering | [90 §10.8] (open point 17) |
| `export.memory-md` | `enum(auto\|full\|pointer)` | `auto` (a pointer line while the `SessionStart` hook is installed) | store | hot | X | `export memory-md` | `moirai-app` export | [AR §13] |

**Constraint K-1.** `pack.quota.c2 + pack.quota.c3 + pack.quota.c4-dev + pack.quota.c5 ≤ 100` and
`pack.quota.c2 + pack.quota.c3 + pack.quota.c4-critic + pack.quota.c5 ≤ 100` (C1 is fixed; the quotas are minimum shares of
one budget, [AR §7.4] step 3).

### 10.9 Harnesses and client profiles

Nothing here changes semantics or the tool list, except the model-profile keys, which select a write rule ([90 §8.2]).

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `client.profile` | `enum(auto\|claude\|codex\|generic)` (+ Tier B names only if #45 builds them) | `auto`: MCP `clientInfo`, then environment detection, then `generic` ([90 §4.1] Client row) | user, q | hot | V | every command and MCP request: the client profile of [API §4.2] CX-7, which a value other than `auto` sets after `--client` and `MOIRAI_CLIENT`; hence the default model family (`lq.model-profile.default.<client>`), the write rule and E411 (V), and the ceilings, instruction text and default tool subset ([90 §6.4]) (O) (spec sync 2b) | `moirai-app`; model `api::Store::resolve` (CX-7) | [90 §10.8] |
| `mcp.tools` | `enum(read\|core\|all)` | `all`; `integrate` writes `core` for harnesses that load schemas up front | store | restart | X | server start: the served tool subset ([90 §6.4]) | `moirai-app` MCP front end | [90 §10.8] |
| `integrate.instructions-scope` | `enum(project\|user)` | `project` | user | install | X | `integrate`: where the `AGENTS.md` block goes | `moirai-app` integrate | [90 §10.8] |
| `integrate.claude-md` | `enum(import\|copy)` | `import` | user | install | X | `integrate claude`: `CLAUDE.md` import line or copy | `moirai-app` integrate | [90 §10.8] |
| `integrate.codex.store-writes` | `enum(writable-root\|execpolicy-store\|execpolicy\|mcp)` | `HOLE(CFG-codex-store-writes)` | user | install | X | `integrate codex`: the sandbox write route ([90 §5.2]); the exit-7 text's owner line ([90 §5.3]) | `moirai-app` integrate | [90 §10.8], [90 §10.5] P7 |
| `integrate.codex.approval` | `enum(prompt\|writes\|split\|approve)` | `HOLE(CFG-codex-approval)` | user | install | X | `integrate codex`: the plugin's approval modes | `moirai-app` integrate | [90 §10.8], [90 §10.5] P6 |
| `integrate.hooks` | `enum(none\|min\|full)` | `rule:hooks-tier`: `full` when the harness being integrated is Tier A (`claude`, `codex`), `min` for a Tier B harness (only if #45 builds one) | user | install | X | `integrate`: the hook set rendered | `moirai-app` integrate | [90 §10.8] |
| `lq.model-profile.<family>` | `enum(gated\|compatible\|unknown)` | `per-param(claude-opus-5-5:HOLE(CFG-model-profile-opus),*:unknown)`: written from the latest LQ-Bench run ([90 §8.2]) | store | hot | V | every write by a session of that family: the profile, hence the write rule and the reading echo ([90 §8.1] L2, L8) | `moirai-lq`; model `profile::model_profile` | [90 §10.8], [50 §7.4] item 9 |
| `lq.model-profile.default.<client>` | `family` | `per-param(claude:claude-opus-5-5,codex:unknown,generic:unknown)` | store | hot | V | a session that declares no model ([90 §4.1] Model row) | `moirai-lq`; model `profile::model_profile` | [90 §10.8] |
| `query.safelist.model.<profile>` | `enum(off\|named-only\|dry-targets)` | `per-param(unknown:named-only,*:off)` | store | hot | V | every write by a session with that profile: `named-only` refuses free-form `TX` with E411; `dry-targets` also admits a `DRY` → `IF TARGETS` pair ([90 §8.1] L2), where `ctx.dry` counts as the block's `DRY` ([API §9.1]; spec sync 2b) | `moirai-lq` `TX` binder; model `profile::model_write_rule` | [90 §10.8] |

### 10.10 Git image

`<name>` is a destination name; the destination used without `--to` is `default` (open point 8). At most four destinations
exist, one per `HEAD.image_cursor` entry ([AR §4.2], [F14]).

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `image.dest.<name>.path` | `path` | `per-param(default:rule:dest-path,*:none)`; rule `dest-path`: `<parent of the main worktree>/<main worktree name>-moirai.git` inside a repository, `<parent of D>/<name of D>-moirai.git` for a store `D/.moirai` outside one | user, q | hot | X | every export, import, `image gc` and `doctor image` naming the destination; a destination with no path is refused by the verb | `moirai-image` | [AR §13], [80] X-F11 |
| `image.dest.<name>.refs` | `glob-list` (ref globs, [F14]) | `main,tags/*,lane/*` | store | hot | V | each export: which refs are copied | `moirai-image` export; model `image::export_set` | [AR §13] (#4) |
| `image.dest.<name>.granularity` | `enum(checkpoint\|commit)` | `checkpoint` | store | hot | V | each future export: which commits are written ([AR §5b.8]) | `moirai-image` export; model `image::export_set` | [AR §13] (#4) |
| `image.dest.<name>.object-format` | `enum(sha1\|sha256)` | `sha1` | store | init | X | destination creation (`image export --create`); recorded in the destination ([F14]) | `moirai-image` | [AR §13] (#4) |
| `image.dest.<name>.kind` | `enum(bare-repo)` | `bare-repo` | store | init | X | destination creation; recorded in the destination | `moirai-image` | [AR §13] (#4) |
| `image.dest.<name>.anchor-text` | `enum(full\|hash-only)` | `full` | store | hot | V | each export: anchor quote text written or only its digests; an import of a `hash-only` anchor gives `text-unavailable`; commit ids unchanged ([40 §5.7]) | `moirai-image`; model `image::anchor_text_on_import` | [40] R-13 ([40] #15) |
| `image.dest.<name>.git.pack-threads` | `int[1..64]` | `2` | store | hot | I | written into the destination's git configuration at `--create` and at `image gc` ([60 §3.5]) | `moirai-image` | [AR §13] |
| `image.dest.<name>.git.pack-window-memory` | `size[1MiB..4GiB]` | `64MiB` | store | hot | I | as above | `moirai-image` | [AR §13] |
| `image.export.on-merge-to-main` | `bool` | `true` | store | hot | X | after a merge into `main`: an export | `moirai-image`, `moirai-app` | [AR §13] |
| `image.export.max-age` | `duration[1h..3650d]` | `1d` | store | hot | X | the first `SessionStart` after this age runs an incremental export; the default of `image export --if-older`; the `brief`/`doctor` warning ([AR §7.5]) | `moirai-app` hooks, `moirai-image` | [AR §13], [74 A22] |
| `image.import-merge` | `enum(auto\|stage)` | `auto` | store | hot | V | a divergent import: merge by the typed rules or always stage | `moirai-image`, `moirai-vcs`; model `image::import_merge` | [AR §13] |
| `image.allowed-remotes` | `url-list` | empty | user, q | hot | X | `image push` and every transport: nothing is pushed to an unlisted remote (owner decision #16) | `moirai-image` transport | [AR §13] |
| `image.transport.spawn-git` | `bool` | `true` | user, q | hot | X | `image push\|pull`: spawn `git` when present, else print the command | `moirai-image` transport | [AR §13] (#3) |

### 10.11 Merge and runs

| key | type | default | scope | reload | vis | effect point | read by | source |
|---|---|---|---|---|---|---|---|---|
| `merge.strict` | `bool` | `false` | store | hot | V | every merge and sync: value conflicts land as conflict values (`false`) or stage (`true`) | `moirai-vcs`; model [RULES/merge-table] `land-or-stage` (column `strict`) | [AR §13] (#10) |
| `runs.granularity` | `enum(workflow\|agent-call)` | `workflow` | store | hot | V | `run open` by the dispatcher contract: one `run` per Workflow run or per agent call | `moirai-app`; model `runs::open_policy` | [AR §13] (#19) |

### 10.12 Environment variables and flags

| key | environment variable | flags (for one command unless stated) |
|---|---|---|
| `discovery.git-hint` | `MOIRAI_GIT_HINT` | — |
| `client.profile` | `MOIRAI_CLIENT` | `--client` on every verb; `moirai mcp --client` for the server's lifetime; `doctor --client` |
| `output.ids-max-bytes` | `MOIRAI_IDS_MAX_BYTES` (open point 17) | — |
| `default-branch` | — | `init --default-branch` (writes the key) |
| `mcp.tools` | — | `moirai mcp --tools` (server lifetime); `integrate --tools` (rendering) |
| `hooks.transport` | — | `integrate --transport` |
| `integrate.hooks` | — | `integrate --hooks` |
| `integrate.codex.store-writes` | — | `integrate --store-writes` |
| `merge.strict` | — | `merge --strict` (`true`) |
| `files.mv-git` | — | `file mv --git` (`true`) |
| `files.portable-names` | — | `file mv --allow-nonportable` (`warn`) |
| `lease.ttl-default` | — | `claim --ttl` |
| `lease.reclaim-older-than` | — | `reclaim --older-than` |
| `gc.reflog-expire`, `gc.cruft-delay` | — | `gc --reflog-expire`, `gc --cruft-delay` |
| `image.dest.<name>.refs`, `.granularity`, `.object-format` | — | `image export --refs`, `--granularity`, `--object-format` (the last only with `--create`) |
| `image.export.max-age` | — | `image export --if-older` |
| `pack.budget.<role>` | — | `pack --budget` (capped by `pack.cli.max-bytes` unless `-o`) |
| `brief.budget` | — | `brief --budget` |
| `query.budget.default.<b>` | — | `--budget <b>=<v>` on every read verb, `q` and `tx`; the MCP `budget` parameter (up to the caller's `query.caps.<role>.<b>`) |
| `files.links-sync-ms`, `files.deep.budget-ms` | — | `links check\|sync --budget-ms` |
| every store key | — | `init --set KEY=VALUE` (§7.6) |

The policy-data row `merge.policy.<kind>` is overridden by `merge --policy` and `resolve --all --policy` ([AR §7.1]).

### 10.13 Policy data (schema rows, not keys)

Versioned per branch, store-wide, read at every write the row governs; changed by schema writes — a `policy` item
([F08 §8.5.6]) written by the `Schema` command ([API §9.8]) — never by `moirai config` (§2.4). A row without an item on
the view takes the default below. The model evaluates them from its rule tables ([RULES/README]) over the view's policy
items (spec sync 2b; until the model reads them from the view it takes them as the input `PolicyData`).

| row | values | default | effect point | model | source |
|---|---|---|---|---|---|
| `policy.self-claim-roles` | role set | `developer, tester`; a role-less self-claim is `developer` | `claim ID`, `claim --next` | `policy::Rights::mint` | [AR §13], [90 §4.3] |
| `policy.mint.role-lease` | role set | `orchestrator, owner` | `claim --role R --run`, bulk claims | `policy::Rights::mint` | [AR §13], [90 §4.3] |
| `policy.hook-label` | `narrow` only (a design rule, listed for completeness) | `narrow` | every write carrying a hook label: rights are the intersection of the lease's and the label's rows | `policy::narrowing_label` | [90 §10.8] |
| `policy.role.<role>.mcp-write` | yes, no | yes for `architect`, `architecture-critic`, `researcher`, `project-analyst`; no otherwise | MCP writes | `policy::Rights::verb` | [AR §13] ([AR §11] #7) |
| `policy.role.<role>.tx` | `per-statement` (the `role-statements` rows of [RULES/role-write-policy] whose key it is) or `none` (none of those statement classes) ([RULES/policy-keys] PV-005) | `per-statement` for every role; under it node `DELETE`, `RESOLVE` and query definitions for `orchestrator` and `owner` through the CLI, and bulk targets for `orchestrator` and `owner` ([RULES/role-write-policy] WX-005; spec sync 2b) | every `TX` statement (E406) | `policy::Rights::statement` | [AR §13], [50 §6.5] ([50] D2) |
| `policy.role.developer.fields` | field list | the [AR §7.3] allowlist | `set` and `TX SET` by a developer lease | `policy::Rights::field` | [AR §13] ([50] D12) |
| `policy.role.<role>.define-query` | yes, no | yes for `orchestrator`, `owner` | `DEFINE QUERY`, `DROP QUERY` | `policy::Rights::statement` | [AR §13] (#25) |
| `policy.role.<role>.authority-owner` | yes, no | the owner's main session with `--owner-quote` | writes with `authority = owner` | `policy::Rights::value` | [AR §13] (#13) |
| `edges.blocks.on-src-deleted`, `edges.gates.on-src-deleted` | `flag`, `drop-notify` | `flag` | deletion of an edge source ([AR §3.3]) | `delete::edge_policy` ([RULES/delete-policy-matrix]) | [AR §13] (#9) |
| `merge.policy.<kind>` | the values of [RULES/merge-table] `auto-policy` | none (opt-in) | merge resolution per kind | `merge::auto_policy` ([RULES/merge-table] `auto-policy`) | [AR §13], [AR §5a.7] |

Unleased callers always get the `general-purpose` row ([AR §7.3], [90 §4.3]; a rule, not a row).

### 10.14 The 25 former owner questions

[60 §3.14] turned 25 former owner questions into keys or policy rows that the reference model implements for every allowed
value. This table maps each to its keys and class, for [RULES/policy-keys] (WP-90).

| Former question | Keys or rows | Class | Model function or checker |
|---|---|---|---|
| [AR §11] #3, spawn half | `image.transport.spawn-git` | X | GT7 transport tests (M5); no model semantics |
| #4 | `image.dest.<name>.{path, refs, granularity, object-format, kind}` | X, V, V, X, X | `image::export_set` for refs and granularity; GT7 for the rest |
| #6 | `hooks.stamp.permission`, `hooks.stamp.ask-for` | X | GT12 |
| #7 | `policy.role.<role>.mcp-write` | V | `policy::Rights::verb` |
| #9, policy half | `edges.blocks.on-src-deleted`, `edges.gates.on-src-deleted` | V | [RULES/delete-policy-matrix] |
| #10 | `merge.strict` | V | [RULES/merge-table] `land-or-stage` |
| #12 | `durability.lazy-kinds`, `quiet.tail-cap-multiplier`, `quiet.from-lane-measuring` | V, I, V | `crash::survives`; SP-1; `quiet::in_quiet_mode` |
| #13 | `brief.lang`; `policy.role.<role>.authority-owner` | O; V | GT12; `policy::Rights::value` |
| #14, retention half | `gc.reflog-expire`, `gc.cruft-delay`, `idempotency.retention` | V | `gc::reachable_after_gc`, `idem::Table::lookup` |
| #19 | `runs.granularity` | V | `runs::open_policy` |
| #25, definer half | `policy.role.<role>.define-query` | V | `policy::Rights::statement` |
| [40 §9.2] #1 | `files.policy.auto` | V | `links::auto_policy` |
| [40] #2 | `roots.<name>`, `files.scratchpads` | V | `links::root_dir`, `links::scratchpad_policy` |
| [40] #4 | `files.hooks.evidence`, `files.hooks.edit-evidence` | X | GT12; the model takes evidence as input events ([60 §4.4] item 1) |
| [40] #5 | `files.usn` (removed, §6.4) | — | — |
| [40] #6 | `files.deletion-inference` | V | `links::deletion_inference` |
| [40] #7 | `files.mv-git` | X | GT17 (the git index is outside the model) |
| [40] #8 | `files.hooks.nudge` (removed, §6.4) | — | — |
| [40] #14 | `files.cloud` | V | `links::cloud_policy` |
| [40] #15 | `image.dest.<name>.anchor-text` | V | `image::anchor_text_on_import` |
| [40] #16 | `files.confirm-roles` | V | `links::confirm_rights` |
| [50] D2 | `policy.role.<role>.tx` | V | `policy::Rights::statement` |
| [50] D3 | `query.safelist.<role>` | V | `profile::read_safelist` |
| [50] D6 | `query.caps.<role>.*` | B | `budget::effective` (the ceiling arithmetic); GT9 for the cut (open point 12) |
| [50] D12 | `policy.role.developer.fields` | V | `policy::Rights::field` |

### 10.15 Never a key

[AR §13] "Never a key", with [80] X-F11's additions: R4's resolver-version constants (R-14: thresholds, the 50 ms quiescence,
`is_text`, the case fold, the never-candidate patterns, the E3d identity rule, the E6 window bound; [F20]); the LQ grammar,
logic, counting, hop bounds and error texts; the canonical form, the `.moi` codec, hash algorithms and identity derivations;
the durability of graph mutations; the typed merge rules; I32′ and [40] #10 and #11; the absence of timers, threads and
watchers at idle; the durability class of each protocol point and its per-OS calls; the `LOCK` layout and lock order; the
group-commit and chain rules; the liveness and boot-identity rules; the crash-gated store allow-list ([80 §2.6]). The
layout and protocol constants of [F17 §13.2] are not keys either.

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] row "Store layout": the `config` syntax, precedence and unknown-key rule (not the key set) | complete | §3, §5, §6 |
| [60 §2.5] audit row "Configuration": the git-config syntax, the precedence, the unknown-key rule, the registry format — not the key set | complete | §3–§6, §9 |
| [60 §2.5] audit row "`HEAD`": `config_gen` | its values, bump, readers and preservation; the offset is [F04]'s | §8 |
| [60 §2.5] row "Store parameters": tunable parameters as `config` keys, init-fixed ones in `HEAD` | the registration of every key (type, scope, reload class, visibility, effect point, reader), the `init --set` route and the constraint mechanics; meaning, ranges and values are [F17]'s | §5.3, §7.6, §10.2 |
| [60 §2.5] "Cross-platform" row: the per-OS user-scope configuration locations; `lock.flush-wait-ms` and the other new keys registered, not frozen | the reading and rewriting of the user file and the key registrations; the locations are [F02 §7]'s | §2.1, §7.5, §10.2, §10.6, §10.7, §10.10 |
| [80] X-F11 | the registration of `lock.flush-wait-ms`, the `image.dest.<name>.path` default, `files.portable-names`, `hooks.session-start.path-export`, and the "Never a key" additions; the frozen locations are [F02 §7]'s | §10.2, §10.6, §10.7, §10.10, §10.15 |
| [80 §3.2] row "Config syntax (git-config)": LF or CR LF on read, LF written, paths with `/` | complete | §3.1, §4.1, §7.3 |
| [40] R-13 | complete: `roots.<name>`, every `files.*` key R-13 names, `image.dest.<name>.anchor-text`; the dropped `files.hooks.nudge` and `files.usn` as retired names | §10.1, §10.4–§10.6, §10.10, §6.4 |
| [40] R-14 | the boundary only: its constants are never keys; the constants are [F20]'s | §10.15 |
| [90 §10.8] | complete: every key and the policy rows | §10.4, §10.7–§10.9, §10.13 |
| [90 §10.1] row "Output contract" | the byte-ceiling keys (`mcp.result-max-bytes`, `.<client>`, `mcp.ids-page-bytes`, `output.ids-max-bytes`, `output.nonzero-exit-max-bytes`); the contract is [F19]'s and [LQ/envelope]'s | §10.8 |
| [90 §10.1] row "Error table and refusal texts" | none; the unknown-model write code (E411) is [LQ/errors]'s and the refusal is selected by `query.safelist.model.<profile>` | §10.9 |
| [AR §13] | complete registry: every key of its eleven tables, the policy data and "Never a key" | §10 |
| [60 §3.14]: the 25 former owner questions as keys or rows | complete mapping | §10.14 |
| [F17] OP-17-03: register `store.log-active-extents`, `store.dict.retrain-growth`, `gc.delete-grace` | complete | §10.2 |
| [50 §5.10] budget keys and the named RSS gate | complete | §10.4, §10.5 |
| R-1…R-12, R-15…R-18; F1–F18; X-F1…X-F10, X-F12 | none | — |

## Holes

This chapter's own holes. The store-parameter holes (`F17-ckpt-ops`, `F17-ckpt-bytes`, `F17-ckpt-body`, `F17-tail-overlay`,
`F17-tail-overlay-quiet`, `F17-quiet-mult`, `F17-promo-ops`, `F17-promo-bytes`, `F17-promo-age`, `F17-loose-pack`,
`F17-lock-writer`, `F17-lock-flush`) are [F17]'s; §10.2 references them. The share-retry bound of the configuration rename is
[OS/fs]'s `OS-share-retry-ms`.

| id | what | decided by | candidates | constraint the value must meet |
|---|---|---|---|---|
| `CFG-codex-store-writes` | default of `integrate.codex.store-writes` | probe P7 of measurement 7 (WP-56), filled by WP-81a ([60 §3.1] "the default of `integrate.codex.store-writes`") | `writable-root` (the design default); `execpolicy-store` | `writable-root` unless P7 shows that the writable-root exemption does not give the elevated sandbox's users write access to the store, or that files they create stay unwritable by the owner; then `execpolicy-store`, never `mcp` ([90 §5.2]). Without Codex access P7 does not run and the value is `writable-root` ([90 §10.5]) |
| `CFG-codex-approval` | default of `integrate.codex.approval` | probe P6 of measurement 7 (WP-56), filled by WP-81a ([90 §10.5] P6) | `split` (the design default); `writes`; `prompt`; `approve` | reads, `claim`, `complete` and `remember` run without a prompt and destructive batches still prompt interactively in the TUI and the app, with a headless override that `codex exec` workers can use; without Codex access, `split` ([90 §10.5]) |
| `CFG-codex-mcp-result` | default of `mcp.result-max-bytes.codex` | probes P3 and P4 of measurement 7 (WP-56), filled by WP-81a ([90 §10.5] P3: "Decides: §6.3 ceilings") | 16,000 (the design value); 12,000; 8,000 | a result at this size, and two such results printed by one code-mode `exec`, reach the model uncut in the measured `exec` output cap, ASCII and Cyrillic; at most 36,000; without Codex access, 16,000 ([90 §10.5]) |
| `CFG-model-profile-opus` | default of `lq.model-profile.claude-opus-5-5` | GT13 on LQ-3 (WP-72), filled by WP-81a ([90 §8.2]: "the defaults are written from the latest LQ-Bench run") | `gated`; `compatible`; `unknown` | `gated` iff Opus 5.5 meets every LQ-Bench write gate ([AR §7.7.5], [50 §7.4] item 6); else `compatible` iff ≥ 75 % after one retry on every stratum; else `unknown` ([90 §8.2]) |

## Open points for the review

1. **Key-name syntax** (§3.2, §3.3). The design says "git-config style" and gives `[files]` `policy.auto = exact`. This
   chapter fixes: dotted headers `[a.b]` whose entries may themselves be dotted; top-level entries before the first header
   (so the one-segment key `default-branch` can be written); no git subsection form `[section "sub"]`; ASCII case folding of
   key names as git folds section and variable names; parameter segments restricted to lower-case letters, digits, `-` and
   `_`. Consequence for [F08] (WP-14): root names should use the same alphabet, so every root can be named in `roots.<name>`;
   `project` and `abs` stay reserved.
2. **Escapes only inside quotes** (§3.4). git also processes `\` outside quotes, which makes a hand-written `D:\notes` invalid
   (`\n`). This chapter keeps a backslash literal outside quotes and recognises only `\\` and `\"` inside quotes. `config set`
   writes paths with `/`, so moirai's own output never needs an escape.
3. **Malformed files and per-command reporting** (§3.1, §3.5, §6.3). The design lists warnings only in `config check` and
   `doctor`. A malformed file (whose keys all fall back) and an invalid environment variable are also reported on every
   command, one warning line each, because otherwise a whole configuration or a harness-set variable is dropped without trace
   (X5). Line-level problems stay in `check` and `doctor`, as the design says. A malformed header ignores the entries under
   it, so they cannot fall under the previous header.
4. **An invalid environment variable** (§5.2) is skipped and resolution continues with the file, not directly with the
   default; [AR §13]'s "falls back to the default" is read as applying to file values, where the next source is the default.
5. **Store-qualified user entries** (§2.3). The design puts per-project, machine-local keys (`files.main-tree`, `roots.<name>`,
   `image.dest.<name>.path`, `image.allowed-remotes`) in the user scope, but one user file serves every store of a user, so a
   single `files.main-tree` cannot name two projects' trunks. `stores.<store-id>.<key>` resolves this without a second file.
   It is part of the frozen syntax and precedence, so the review should confirm it before the freeze.
6. **`files.main-tree`'s default: a conflict between documents.** [AR §13] says "the directory where `init` ran"; [40 §5.1]
   (normative for R-13) says that when the key is unset `main` uses the main worktree and `doctor` warns. Following [40] for
   its reservation, the fallback is the main worktree; to keep [AR]'s intent, `init` run in another tree records that tree as
   a store-qualified user entry (§7.6). A user-scope default cannot be recorded in the store, because a store restored on
   another machine never carries user keys.
7. **`files.main-ref`'s default** ("the branch checked out at `init`") is made durable by `init` writing it into the store
   file; outside git it is unset.
8. **Image destinations.** The design names no default destination. This chapter calls it `default`, gives only it a path
   default, and defines `<project>` outside a repository. At most four destinations exist (`HEAD.image_cursor[4]`); [F14]
   (WP-15) maps names to `dest` numbers and records `object-format` and `kind` in the destination.
9. **The `config set` protocol and `config_gen`** (§7.4, §8). The rename runs outside every lock byte ([OS/fs §6.3]); the bump is
   a read-modify-write publish under the writer byte that changes only `config_gen`, appends no record and is not flushed.
   Two concurrent `config set` commands can still lose one update between the compare and the rename; configuration writes
   are rare owner actions, so the chapter accepts this rather than holding the writer byte across a rename and a directory
   flush. [F16] (WP-16) should list the protocol point with its classes and [F15] OP-5's two-parent `durable-name`; [F04]
   (WP-11) should place `config_gen` and state that every other publish copies it. A seeded bug for WP-40 is optional,
   because a lost bump only delays a re-read.
10. **Hand edits and running MCP servers** (§5.4). A hand edit reaches a running server at its next store open, the next
    `config_gen` change or its restart; `config set` of any key, even to the same value, is the documented reload trigger.
    Detecting hand edits per request would cost a stat per request, which [AR §13] rules out.
11. **`init --set KEY=VALUE`** (§7.6) names the flag [F17 §2.2] IP-5 asks this chapter for, and gives the test profile its
    route ([F17 §12] TP-2). [F19] and [API] (WP-25) should carry it; the Store API `Init` command should take the same
    key/value list, and a `ConfigSet`/`ConfigUnset` command should let GT2 streams change hot keys between commands.
12. **Budget keys and the model** (§9.4, §10.14). [60 §3.14] counts [50] D6 (`query.caps.<role>.*`) among the keys the model
    implements for every value, while [60 §4.2] says budgets are not modelled (GT9 checks them). Resolution: the model
    implements the ceiling arithmetic (`budget::effective`); the cuts are GT9's. E8's "every allowed value of each policy key"
    is read as every class-V key.
13. **`query.caps.<role>` defaults and raises above a ceiling.** [AR §13] gives only orchestrator and owner (10 × the agent
    maximum). This chapter sets every other role's default to the agent maximum of [50 §5.10], gives `mem` a per-process-kind
    rule, and lets orchestrator and owner raise the deadline 10 ×; their `refs` ceiling is 80, above E304's "(at most 8)", which
    [LQ/errors] should word per role. How a request above the
    ceiling is answered (clamped with a notice, or refused) is [LQ/errors]'s; proposal: clamp and show the applied value in
    the `budget` object, with a notice. **Pass 1 (A1-53):** E304's help renders the caller's own ceiling ([LQ/errors §5.4]).
14. **The RSS-gate keys** (§10.4). [50 §5.10] asks for "a named store parameter (chapter 17 or the configuration registry)"
    for the process kind's RSS gate; [F17] does not register it, so this chapter does (`mem.rss-gate.*`). The design writes
    the gates in MB; the defaults read MB as 10^6 bytes (4,000,000 and 16,000,000, canonically `4000000` and `15625KiB`), the
    conservative reading.
15. **Unknown private bytes** (§10.4). [OS/mem §3] leaves the `mem` value for a failed meter reading to [CFG] and [LQ]. This
    chapter uses the configured maximum (`query.budget.default.mem`, and 1 MiB for `wmem`), so a meter failure never shrinks
    a query below what the test profile uses. **Pass 1 (P1-11):** `wmem` is now a budget; a failed reading gives both `mem`
    and `wmem` their requested values (§10.4).
16. **Model family names** (§4.3). The design keys `lq.model-profile.<family>` by family but gives no spelling; this chapter
    derives a family name from the declared model id and reserves `default` and `unknown`. The Opus 5.5 family is
    `claude-opus-5-5`.
17. **`output.ids-max-bytes = 0` "for scripts".** The design says a script may set 0 but names no channel. This chapter adds the
    environment variable `MOIRAI_IDS_MAX_BYTES`, which obeys [90 §2.1]'s naming rule.
18. **`BUDGET` classes and `query.budget.default.work`.** [LQ/std §2.4] fixes `light`, `medium` and `heavy` at 200,000,
    2,000,000 and 20,000,000. Proposal for [LQ/std] (WP-19): `medium` is `query.budget.default.work`, `light` a tenth and
    `heavy` ten times it, so the key moves all three and the defaults stay as written. **Pass 1 (A1-37): adopted** in
    [LQ/std §2.4].
19. **Hole-id conflict across spec files** — closed in review pass 1 (S1-40). [OS/lock §4] wrote `lock-writer-wait-ms` and
    `lock-flush-wait-ms` and said [CFG] owned them; [F17] owns the same values as `F17-lock-writer` and `F17-lock-flush`
    (OP-17-23). [OS/lock] now cites [F17]'s ids; this chapter references them and owns neither ([F01] open point 15).
20. **Open points of other chapters closed here.** [F01] open point 10 (§3.1 cites [F01 §6.7]); [F02] open point 15 (§7.5),
    16 (§2.5) and 19 (§10.1: `MOIRAI_GIT_HINT` uses the `bool` spellings, and an invalid value is `CFG12`); [F17] OP-17-03
    (§10.2) and IP-5's flag (§7.6).
21. **[PLAN §3.3]'s gaps for WP-18** — the unknown-model write error code, whether E406's new fix text is frozen, F18's
    violation classes — belong to WP-18's other chapter [F19] and to [LQ/errors] (which already lists E411
    `unknown_model_write`); this chapter does not resolve them. The configuration diagnostics CFG01–CFG17 (§6.2) are proposed
    for [F19]'s code table, which may renumber them. **Pass 1 (A1-39, A1-62): closed.** [F19 §10.6] keeps the numbers and
    names as they stand; the text form, the seven-space continuation indent and the footer order are [LQ/errors §4.2]'s.
22. **`mcp.overlay-lru`'s default.** [74 §5.3] proposed 4; [AR §13], the design of record after the audits, has 8. This
    chapter follows [AR].
23. **Reload class of the evidence-hook keys.** [AR §13] gives `files.hooks.evidence` and `.edit-evidence` the class `hot`
    and the `hooks.<hook>.enabled` keys the class `install`, while `hooks install` reads all of them. §10.6 specifies both
    effect points for the evidence keys: registration at install, and a check at each firing.
24. **Measurement 15 and the `files.*-ms` keys.** [60 §5.2] row 15 "decides R4 budgets". This chapter reads that as the time
    budgets and gates of [40 §7], which the measurement re-derives, not as the values of `files.read-budget-ms`,
    `files.session-start-cap-ms` and `files.links-sync-ms`, which stay design-fixed. If the review reads it the other way,
    they become holes `CFG-files-read-ms`, `CFG-files-session-start-ms` and `CFG-files-links-sync-ms` decided by WP-55.
25. **`pack.cli.max-bytes` and measurement 7.** Item 7 records the Bash tool's inline and failure caps ([AR §8.2]). The
    defaults 24,000 and 8,000 stay design-fixed; if the measured inline cap is below 28,000 characters, the review should lower
    the range bound of `pack.cli.max-bytes` and, if needed, its default.
26. **Provisional values revisited after M0.** `pack.budget.<role>` (M9), `query.budget.default.fs` and `tx.max-work-in-lock`
    (M7) are design-fixed provisional values, not M0 holes; their revisit follows the design sections that set them.
27. **`files.deep.budget-ms`** is a key this chapter adds for [40 §7.2]'s "10 s default budget" of `--deep`, because
    operational values go into keys (AGENTS.md; [74] C50).
28. **Lists.** List types use one comma-separated value; an item cannot contain a comma (a glob can match one with `[,]`). git's
    multi-valued variables are not used, so "the last occurrence wins" holds for every key.
29. **Ranges and reload classes are registry data.** The design gives few ranges; this chapter sets one for every key so that
    `config set` can validate. They belong to the unfrozen key set.
30. **Exit codes of `moirai config`** (§7.1): 2 for every refused request, including a malformed target file, 3 for `get` of an
    unset file value, 7 for a store that cannot be written or keeps changing. [F19] should confirm them.
31. **The `config` envelope key** (§5.6) is additive under [LQ/envelope §7.2]; [LQ/envelope §7.1]'s key table and [F19]'s
    envelope should list it, after the keys they already name.
32. **Canonical spellings of design defaults.** The registry prints defaults in canonical form (§4.1), so the design's "24 h"
    is `1d`, "60 s" is `1m`, "64m" is `64MiB`, and the MCP RSS gate is `15625KiB`. The values are unchanged.
33. **Pass 1 changes** (P1-11, P1-12, P1-39, S1-39, A1-50, A1-55). `wmem` becomes a budget (`query.budget.default.wmem`
    = 1 MiB, `query.caps.<role>.wmem` = 4 MiB for agents), effective as max(256 KiB, min(requested, headroom));
    `tx.wmem-max` is retired (§6.4), and [F17 §4.4] W2, W4, §12 TP-3 and its tables restate these keys (round 1). A
    default-cap `TX` does not fit the inline bound whatever its `wmem` ([F17 §4.4] W4); E501 names the split for that
    and the raise for `wmem`. **Decided 2026-09-28** (owner question OQ-P-2, option (a)):
    `store.commit.inline-max-bytes` (P05, 1 MiB) and the default `tx.max-ops` (10,000) both stay; P05 bounds an agent
    `TX` and E501 names the split. `init` refuses a set of values that fails C-1–C-4 (exit 2, §7.6), and a tunable's
    fallback is min(default, the largest admissible value under the recorded `init` values) (§5.3, RG-2).
    `input.max-bytes` (16 MiB) bounds stdin and `-f` input before any budget. Root names start with a letter, as
    [F08 §5.4.1] requires. `[RULES/delete-policy]` is corrected to `delete-policy-matrix`; `[RULES/policy-keys]` stays
    cited as a planned file. The configuration duration grammar and LQ's differ on purpose, and both chapters say so
    (§4.1, [LQ/lexical §5.6]).
34. **Spec sync 2b.** The `read by` and `model` cells take [RULES/policy-keys]' bound names, which are normative (§9.4).
    `client.profile` is class V and read by [API §4.2] CX-7. `durability.lazy-kinds` is checked by GT3 only, since no
    `Store` API stream has an OS crash ([API §6.7]). `policy.role.<role>.tx` takes `per-statement` or `none`, and bulk
    targets are for `orchestrator` and `owner` ([RULES/role-write-policy] WX-005). Policy data are schema items of class
    `policy` ([F08 §8.5.6]). §7.2 names the refusal codes and checks `config unset` like a set; §2.3 admits a qualified
    user-lower key, which §5.1 already read. `tx.max-ops` counts net ops; quiet mode is the flag or (the key and a
    measuring lane); `ctx.dry` is the block's `DRY` under `dry-targets`; the model sweeps upper bounds unclipped.
