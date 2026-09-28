//! The tree-sitter-rust oracle for FL-1's Rust scope scanner: it emits scope items as JSON for the differential in
//! `moirai-replay` ([40 §2.7.1]; [40 §8.3.4] row 8; WP-63, WP-74).
//!
//! Host-only crate (`xtask/host-only.toml`): tree-sitter compiles C, so the crate stays outside every checked graph,
//! is excluded from GT20 (e) and is built only by the replay job, unpoisoned. No crate depends on it: the
//! differential runs the `moirai-tsoracle` binary as a child process and reads its output (GT20 (b) rule 3). Sources:
//! [90 §11.1]; `docs/m0/PLAN.md` §2.1, §2.2, §6.2 R7. The pinned versions are [`record::TREE_SITTER`] and
//! [`record::TREE_SITTER_RUST`]; `--version` prints them with the grammar's ABI.
//!
//! # What the oracle reports
//!
//! For each Rust source, the **scope items** of [40 §2.7.1] — `mod`, `impl [Trait for] T`, `fn`, `struct`, `enum`,
//! `trait`, `const`, `static`, `macro_rules!` — each with its [F08 §10.3.1] scope segment (kind, name, qualifier),
//! its line span, its enclosing item and whether it is inside the oracle's claim. FL-1's hand-written scanner (WP-63)
//! reports the same items for the same file; the differential renders the scanner's result in this shape and compares
//! ([40 §8.3.4] row 8: name paths agree on ≥ 99.5 % of items).
//!
//! # Items
//!
//! These rules are tree-sitter-rust's view of Rust, made explicit so the scanner can match them. No chapter specifies
//! the scanner grammar yet: open point 30 of chapter 20 ([F20 §"Open points for the review"]) leaves it to review
//! pass 1. Until chapter 20's scanner-grammar appendix is written, rules 1–7 and [`canon`]'s spelling are the
//! proposal the oracle checks; they are to be adopted there, or changed there with the oracle following (and
//! [`record::FORMAT`] raised), before WP-63 is accepted, so that the scanner is specified by the specification and not
//! by its own oracle. Rule 8 is the oracle's own: it says which items the oracle vouches for.
//!
//! 1. **Kinds.** A node of one of the kinds of [`scan::ITEM_NODES`] is an item: `mod_item` (`mod`), `impl_item`
//!    (`impl`), `function_item` and `function_signature_item` (`fn`, with or without a body, so trait method
//!    declarations and foreign functions count), `struct_item`, `enum_item`, `trait_item`, `const_item`,
//!    `static_item` (`static` and `static mut`) and `macro_definition` (`macro_rules`). Nothing else is: not `union`,
//!    `type` aliases, associated types, `use`, `extern crate`, `extern` blocks, enum variants, fields, closures,
//!    `const` blocks, `const` generic parameters or `impl Trait` types. `const fn` is a `fn`. tree-sitter-rust also
//!    accepts, with no error, some syntax that is not stable Rust: lazy_static's `static ref X: T = …;` outside its
//!    macro, `default fn` and `static ||` closures. Such sources are not part of the scanner's contract.
//! 2. **Where.** Items are found at any depth: in modules, `impl` and `trait` bodies, `extern` blocks, function
//!    bodies and every block or initialiser inside them. The body of a macro invocation (`foo! { … }`) and of a
//!    `macro_rules!` definition is a token tree, so nothing inside one is an item. The oracle sets no depth limit, so
//!    a name path (below) can be longer than the 64 segments of [F08 §10.3.1]'s scope value; what a scanner records
//!    for an item nested deeper is for chapter 20's appendix to say.
//! 3. **Name.** The canonical spelling ([`canon`]) of the item's name: the identifier after the keyword (`r#`
//!    raw identifiers keep their prefix; `const _` has the name `_`), or, for an `impl`, the whole type after `for`
//!    (or after `impl` and its generic parameters), generic arguments included: `impl<T> From<T> for Wrap<T>` has the
//!    name `Wrap<T>`. A line break or a NUL byte inside a literal of the name is escaped, so the name is one line and
//!    holds no U+0000 ([`canon`] step 3). Bytes of the name that are not UTF-8 are replaced by U+FFFD, one per maximal
//!    invalid subpart (Unicode §3.9, as [`String::from_utf8_lossy`] does); a source that is not UTF-8 also counts one
//!    error (rustc rejects it). An item whose name is empty is not reported, and the items inside it go to the next
//!    enclosing item (rule 6). Error recovery produces such items: `impl<T> { … }` is an `impl_item` whose type is a
//!    MISSING node. Rule 8 takes the items inside one out of the claim.
//! 4. **Qualifier.** For `impl Trait for T`, the canonical spelling of the trait path, generic arguments included,
//!    with a leading `!` for a negative impl (`!Send`); empty for every other item. [F08 §10.3.1] names the
//!    qualifier `Trait` and does not say whether it keeps the `!` or the generic arguments; the scanner grammar of
//!    chapter 20's open point 30 decides, and until then this rule is the proposal.
//! 5. **Lines.** `start` is the 1-based line of the item's first token — its visibility (`pub`, `pub(crate)`), a
//!    qualifier (`unsafe`, `const`, `async`, `extern "C"`) or its keyword. Outer attributes (`#[…]`) and comments
//!    before the item, doc comments included, are not part of it, so `start` is the item's header line ([F20 §2.8]).
//!    When the visibility or a qualifier stands on a line of its own (`pub(crate)⏎fn f()`), `start` is that line.
//!    `end` is the line of the item's last byte (its closing `}` or `;`). Lines are counted at `0A` bytes, as
//!    `lines` counts them ([F20 §2.5]); one leading UTF-8 BOM is skipped and changes no line number.
//! 6. **Parent.** The nearest reported item whose node encloses the item's node. `extern` blocks and every other
//!    non-item node are transparent. Items nest only by containment in the source: an `impl` is a sibling of the
//!    `struct`, `enum` or `trait` it implements, never its child.
//! 7. **Order.** Pre-order: by first byte, an enclosing item before the items it contains.
//! 8. **Claim.** An item is `ok` — its name path, lines and parent are what rules 1–7 give for the source as written —
//!    when the source is valid UTF-8 and no syntax error (an ERROR or MISSING node of tree-sitter's tree) lies
//!    - inside the item's node, from its first token to its last byte;
//!    - on the path from the item's node to the root: no enclosing node is an ERROR node;
//!    - in the name or qualifier of an enclosing item node, reported or not (rule 3), because its name path carries
//!      them.
//!
//!    So one error takes out of the claim the items that contain it and the items it may have misplaced, and no
//!    other item of the file: in `fn a() {}` `fn b(x: Box<dyn 'a + Send>) {}` `fn c() {}` only `b` is not `ok`; an
//!    error in an `impl`'s where clause leaves its methods `ok`, and an error in its type does not.
//!
//! The **name path** of an item is the list of its ancestors' (kind, name, qualifier) triples, outermost first,
//! followed by its own: exactly the `segments` of [F08 §10.3.1]'s scope value. `mod a { impl Tr for S { fn f() {} } }`
//! gives, for `f`, the name path `(mod, a, "") / (impl, S, Tr) / (fn, f, "")`. A name path is not always unique in a
//! file (`cfg`-gated twins, [11 §2.6]), so the differential compares the items of a file as multisets.
//!
//! **What the shape supports.** [40 §8.3.4] row 8 counts items whose name paths agree. The same record also carries
//! what an anchor takes from the scanner besides the name path: `start` is the header line whose text
//! `span_hash` covers for `watch = header` ([F20 §2.8]), and `start`–`end` is a `symbol` anchor's hint
//! ([F20 §6.1] step 7). A differential can therefore report name-path agreement, header-line agreement and span
//! agreement separately. It compares the `ok` items of every record, the records with `errors` > 0 included, so its
//! denominator is the `ok` items; it counts the items with `ok` = false apart and reports that count next to the
//! agreement rate, so the exclusion is visible and bounded, not silent.
//!
//! # Known grammar gaps
//!
//! The pinned grammar does not parse some Rust. A source that uses it has `errors` > 0, and the items the error
//! touches (rule 8) are outside the claim, although error recovery usually keeps them. In stable Rust:
//!
//! - the explicit `safe` and `unsafe` qualifiers of items inside an `unsafe extern` block (`safe fn f();`,
//!   `safe static S: u8;`, `unsafe static S: u8;`, Rust 1.82, used by edition 2024 code; the block itself and
//!   unqualified items parse);
//! - a lifetime first in a trait object's bounds (`Box<dyn 'a + Send>`);
//! - an empty where-bound (`where C: ,`, `where [(); N]:`);
//! - a negative const generic argument (`N::<-1>()`);
//! - `~` in a macro's token tree, in an invocation or a `macro_rules!` definition;
//! - a NUL byte in a line comment (block and doc comments take one).
//!
//! In unstable Rust: `auto trait`, trait aliases (`trait A = B;`), `macro` 2.0 definitions, `impl const` and default
//! field values. One such construct used to take a whole file out of the comparison; rule 8 keeps every item it does
//! not touch in it. The test `known_grammar_gaps_are_syntax_errors` pins the list against a grammar upgrade.
//!
//! # Command line
//!
//! ```text
//! moirai-tsoracle [FILE | - | --files-from LIST]... [-- FILE...]
//! moirai-tsoracle --version
//! moirai-tsoracle --help | -h
//! ```
//!
//! - `FILE` is a Rust source path, relative to the working directory or absolute. `-` reads one source from standard
//!   input, except after `--`.
//! - `--files-from LIST` reads the paths to scan from the UTF-8 file LIST, one per line (LF or CRLF; empty lines are
//!   skipped; one UTF-8 BOM at the start of LIST is skipped, as Windows PowerShell 5.1's `Out-File -Encoding utf8`
//!   writes one; each path is taken as a `FILE` is), or from standard input when LIST is `-`. The list is processed at
//!   the option's place in the argument order, as it is read. It spares the 32 K-character command-line limit of
//!   Windows.
//! - After `--`, every argument is a `FILE`, even one that starts with `-`: `-- -` scans a file named `-`, not
//!   standard input.
//! - Standard input serves at most one of `-` and `--files-from -` per run.
//! - Inputs are scanned one at a time, in argument order, with one parser and one reused buffer. Each record is
//!   written and flushed as soon as its input is scanned, so a caller that feeds `--files-from -` one path at a time
//!   receives each record before it sends the next path.
//! - An input is held whole while it is scanned, and none may exceed [`scan::MAX_INPUT`] bytes (tree-sitter's 4 GiB
//!   offsets, plus a BOM): a larger file is refused from its size before any of it is read, and every read, standard
//!   input included, stops one byte past the limit.
//! - `--help` (or `-h`) prints the usage; `--version` prints the pins (below). Either must be the only argument.
//!
//! **Exit status:** 0 when every input was scanned. 1 when the grammar could not be loaded (the linked tree-sitter
//! runtime refuses it), an input or a LIST could not be read (missing, not UTF-8, larger than the limit) or parsed,
//! or the output could not be written: a message goes to standard error and the run stops. The records written
//! before an input failure are complete lines; an output failure may leave the last line partial. 2 on a usage error
//! (an unknown option, no input, an argument that is not valid Unicode, standard input requested twice, `--help` or
//! `--version` with another argument).
//!
//! # Output: JSON Lines, format 2
//!
//! Standard output carries one record per input, in input order. A record is one line of compact JSON (no spaces,
//! keys in the order shown) ended by `0A`:
//!
//! ```text
//! {"path":P,"errors":E,"items":[ITEM,...]}
//! ITEM = {"kind":K,"name":N,"qual":Q,"start":S,"end":L,"parent":R,"ok":B}
//! ```
//!
//! | Key | JSON type | Meaning |
//! |---|---|---|
//! | `path` | string | the input as given on the command line or in the list; `-` for standard input |
//! | `errors` | number | ERROR and MISSING nodes in tree-sitter's tree, plus 1 when the source (its BOM removed) is not valid UTF-8; 0 for a UTF-8 source tree-sitter-rust parses cleanly |
//! | `items` | array | the items (rules above), in pre-order |
//! | `kind` | string | `mod`, `impl`, `fn`, `struct`, `enum`, `trait`, `const`, `static` or `macro_rules`: [F08 §10.3.1]'s `skind` 1–9 ([`Kind::skind`]) |
//! | `name` | string | the canonical name (rule 3); never empty, one line |
//! | `qual` | string | the canonical qualifier (rule 4), one line; empty unless the item is `impl Trait for T` |
//! | `start`, `end` | number | first and last line of the item, 1-based and inclusive (rule 5) |
//! | `parent` | number or `null` | the index in `items` of the enclosing item (rule 6), always less than the item's own index; `null` at top level |
//! | `ok` | boolean | whether the item is inside the oracle's claim (rule 8); always `true` when `errors` is 0 |
//!
//! Example: for the source
//!
//! ```text
//! mod a {
//!     fn f() {}
//! }
//! impl Display for S {}
//! ```
//!
//! the record is (one line):
//!
//! ```text
//! {"path":"src/x.rs","errors":0,"items":[{"kind":"mod","name":"a","qual":"","start":1,"end":3,"parent":null,"ok":true},
//! {"kind":"fn","name":"f","qual":"","start":2,"end":2,"parent":0,"ok":true},
//! {"kind":"impl","name":"S","qual":"Display","start":4,"end":4,"parent":null,"ok":true}]}
//! ```
//!
//! `--version` prints one record, `{"oracle":"moirai-tsoracle","format":2,"tree_sitter":"0.27.0",
//! "tree_sitter_rust":"0.24.2","language_abi":N}`, so the differential can check the pins before it compares.
//! [`record::FORMAT`] changes whenever a key, a value's meaning or an item rule changes.

pub mod canon;
pub mod record;
pub mod scan;

pub use scan::{Item, Kind, Oracle, OracleError, Scan};
