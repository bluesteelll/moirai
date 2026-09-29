//! The binding context of the binder tests ([LQ/canonical-ast §9]): the core schema, a synthetic store with nodes
//! `#1`–`#3000` (a few with known kinds), commits of sequence numbers 1–5000 (one of them `c41d7e0b…`, the example
//! rendering of a `{{commit:…}}` placeholder, [lqbench README §3.2]) and the default caller.

use crate::lq::bind::{self, Bound};
use crate::lq::cast::{CDefine, CQuery, CTx, CommitId, Root, Uid, encode, sexpr};
use crate::lq::ctx::{BindCtx, Caller, MapIds, Params};
use crate::lq::diag::{Code, Diag, line_col};
use crate::lq::parser::{ParseOptions, parse_define, parse_read, parse_write};
use crate::lq::schema::Schema;

/// The uid the fixture gives `#n`.
pub(super) fn uid(n: u32) -> Uid {
    let mut u = [0u8; 16];
    u[..4].copy_from_slice(&n.to_be_bytes());
    u[4] = 0x5e;
    u[15] = 0x77;
    u
}

/// The commit id the fixture gives sequence number `s`.
pub(super) fn commit(s: u64) -> CommitId {
    let mut c = [0u8; 32];
    if s == 4466 {
        c[..4].copy_from_slice(&[0x41, 0xd7, 0xe0, 0xb2]);
    } else {
        c[0] = 0xc0;
        c[1..9].copy_from_slice(&s.to_be_bytes());
    }
    c[31] = 0x99;
    c
}

/// Known kinds of a few fixture nodes.
pub(super) const KINDS: [(u32, &str); 16] = [
    (12, "task"),
    (51, "task"),
    (88, "task"),
    (89, "task"),
    (90, "task"),
    (93, "task"),
    (130, "finding"),
    (131, "doc"),
    (133, "doc"),
    (164, "verdict"),
    (212, "rule"),
    (254, "doc"),
    (300, "area"),
    (303, "artifact"),
    (700, "lane"),
    (901, "question"),
];

/// The fixture store's identity maps.
pub(super) fn ids() -> MapIds {
    let mut m = MapIds::new();
    for n in 1..=3000 {
        let kind = KINDS.iter().find(|(k, _)| *k == n).map(|(_, k)| *k);
        m.node(n, uid(n), kind);
    }
    for s in 1..=5000 {
        m.commit(s, commit(s), if s % 2 == 0 { "main" } else { "lane/net" });
    }
    m
}

/// A binding context over the core schema with `params` and `caller`, run by `f`.
pub(super) fn with_ctx<T>(
    params: &Params,
    caller: &Caller,
    f: impl FnOnce(&BindCtx<'_>) -> T,
) -> T {
    let schema = Schema::core();
    let ids = ids();
    let ctx = BindCtx {
        schema: &schema,
        ids: &ids,
        params,
        caller,
    };
    f(&ctx)
}

pub(super) fn show(src: &str, e: &[Diag]) -> String {
    super::show(src, e)
}

/// Binds a read with parameters and a caller.
pub(super) fn read_with(
    src: &str,
    params: &Params,
    caller: &Caller,
) -> Result<Bound<CQuery>, Vec<Diag>> {
    let p = parse_read(src, ParseOptions::default())
        .unwrap_or_else(|e| panic!("{src:?} does not parse: {}", show(src, &e)));
    with_ctx(params, caller, |ctx| bind::bind_read(ctx, src, &p.tree))
}

/// Binds a read that must bind.
pub(super) fn read(src: &str) -> Bound<CQuery> {
    read_with(src, &Params::new(), &Caller::default())
        .unwrap_or_else(|e| panic!("{src:?} does not bind: {}", show(src, &e)))
}

/// Binds a write with parameters and a caller.
pub(super) fn write_with(
    src: &str,
    params: &Params,
    caller: &Caller,
) -> Result<Bound<CTx>, Vec<Diag>> {
    let p = parse_write(src, ParseOptions::default())
        .unwrap_or_else(|e| panic!("{src:?} does not parse: {}", show(src, &e)));
    with_ctx(params, caller, |ctx| bind::bind_write(ctx, src, &p.tree))
}

/// Binds a write that must bind.
pub(super) fn write(src: &str) -> Bound<CTx> {
    write_with(src, &Params::new(), &Caller::default())
        .unwrap_or_else(|e| panic!("{src:?} does not bind: {}", show(src, &e)))
}

/// Binds a definition.
pub(super) fn define(src: &str) -> Result<Bound<CDefine>, Vec<Diag>> {
    let p = parse_define(src, ParseOptions::default())
        .unwrap_or_else(|e| panic!("{src:?} does not parse: {}", show(src, &e)));
    with_ctx(&Params::new(), &Caller::default(), |ctx| {
        bind::bind_define(ctx, src, &p.tree)
    })
}

/// The first binder error of a read: code, line, column (0, 0 when unlocated).
pub(super) fn read_err(src: &str) -> (Code, u32, u32) {
    read_err_with(src, &Params::new(), &Caller::default())
}

/// [`read_err`] with parameters and a caller.
pub(super) fn read_err_with(src: &str, params: &Params, caller: &Caller) -> (Code, u32, u32) {
    match read_with(src, params, caller) {
        Ok(_) => panic!("{src:?} binds"),
        Err(e) => first(src, &e),
    }
}

/// The first binder error of a write.
pub(super) fn write_err_with(src: &str, params: &Params, caller: &Caller) -> (Code, u32, u32) {
    match write_with(src, params, caller) {
        Ok(_) => panic!("{src:?} binds"),
        Err(e) => first(src, &e),
    }
}

/// The first binder error of a write with no parameters, as the default caller.
pub(super) fn write_err(src: &str) -> (Code, u32, u32) {
    write_err_with(src, &Params::new(), &Caller::default())
}

fn first(src: &str, e: &[Diag]) -> (Code, u32, u32) {
    let d = &e[0];
    match d.span {
        Some(s) => {
            let (l, c) = line_col(src, s.start);
            (d.code, l, c)
        }
        None => (d.code, 0, 0),
    }
}

/// The C-AST S-expression of a read.
pub(super) fn cast_of(src: &str) -> String {
    sexpr(Root::Query(&read(src).ast))
}

/// The encoding of a read's C-AST.
pub(super) fn bytes_of(src: &str) -> Vec<u8> {
    encode(Root::Query(&read(src).ast))
}
