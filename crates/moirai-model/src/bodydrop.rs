//! `BodyDrop` ([API §8.7]; [F06 §8.1]): the owner drops bodies by their BLAKE3-128. The command keeps every state, id
//! and digest as it is (DB-5): it adds the hashes, with their DB-2 reasons, to the store's dropped set
//! ([`Store::dropped`], [`Store::drop_reasons`]), which every reader of a body consults first (DB-6) and every merge's
//! text rule reads ([RULES/merge-table] CS-011). The model executes no purge (DB-8, class I), has no `Backup` record
//! (`backups` is empty) and keeps neither the note nor the actor, which no result shows.

use crate::api::{Ctx, Data, Door, Reply, Store};
use crate::err::{Refusal, Res};
use crate::state::{Aspect, Body, KVal, Node};
use std::collections::BTreeSet;

/// The most hashes one call names ([API §8.7] `hashes`).
pub const MAX_HASHES: usize = 2048;

/// The longest note, in bytes ([API §8.7] `note`).
pub const MAX_NOTE: usize = 200;

/// The reason a body was dropped ([F06 §8.1] DB-2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropReason {
    /// `secret`.
    Secret,
    /// `private`.
    Private,
    /// `other`.
    Other,
}

impl DropReason {
    /// The reason a name gives; `None` for a name that is not one ([API §8.7] `reason`).
    pub fn parse(s: &str) -> Option<DropReason> {
        match s {
            "secret" => Some(DropReason::Secret),
            "private" => Some(DropReason::Private),
            "other" => Some(DropReason::Other),
            _ => None,
        }
    }

    /// The reason's name ([F06 §8.1] DB-2; [API §15.7] `dropped`).
    pub fn name(self) -> &'static str {
        match self {
            DropReason::Secret => "secret",
            DropReason::Private => "private",
            DropReason::Other => "other",
        }
    }
}

/// The `data` of `BodyDrop` ([API §8.7] "Result").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyDropData {
    /// `dropped`: the hashes this command dropped, ascending.
    pub dropped: Vec<[u8; 16]>,
    /// `already`: the named hashes that were dropped before it, ascending.
    pub already: Vec<[u8; 16]>,
    /// `backups`: always empty, the model having no `Backup` record.
    pub backups: Vec<String>,
    /// `purged`: always true, no purge being pending in a model that runs none (excluded from the comparison,
    /// [API §16.3]).
    pub purged: bool,
}

/// A hash argument: exactly 32 lower-case hexadecimal digits ([API §8.7] `hashes`).
fn parse_hash(s: &str) -> Option<[u8; 16]> {
    let b = s.as_bytes();
    if b.len() != 32 {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (digit(b[2 * i])? << 4) | digit(b[2 * i + 1])?;
    }
    Some(out)
}

/// Every body a node of a state carries: its own body, and the bodies of the `base`, `ours` and `theirs` sides and of
/// the existence images of its conflict values ([F06 §6.2]).
fn bodies(x: &Node) -> Vec<&Body> {
    let mut v: Vec<&Body> = x.body.iter().collect();
    for c in x.conflicts.values() {
        for side in [&c.base, &c.ours, &c.theirs] {
            if let Some(KVal::Body(b)) = side {
                v.push(b);
            }
        }
        for image in c.images.iter().flatten() {
            if let Some(KVal::Body(b)) = image.get(&Aspect::Body) {
                v.push(b);
            }
        }
    }
    v
}

impl Store {
    /// `BodyDrop` ([API §8.7]). The caller first: owner-attested only (`by` = `owner` with the orchestrator's session
    /// role lease, WT-012), through the CLI only (WV-051); any other call is E406 before anything else of the command
    /// is checked, once §4.3 rows 1-4 have passed. Then the arguments (`usage`); then each distinct hash ascending: a
    /// dropped one goes to `already`, an unheld one refuses the command (`not_found`, `what` = `body`), nothing written.
    /// The held, undropped hashes are recorded with the reason, drawing one value of CK-4's sequence; nothing is drawn
    /// when none is. Not keyed ([API §8] family W); quiet mode does not refuse it.
    // spec: [API §8.7]
    // spec: [F06 §8.1]
    // rule: WV-051, WT-012
    pub(crate) fn body_drop(
        &mut self,
        hashes: &[String],
        reason: &str,
        note: &str,
        by: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve(ctx, false)?;
        let mut rights = self.rights(&caller, ctx);
        if by == Some("owner") && rights.lease.as_ref().is_some_and(|l| l.session_role) {
            rights.owner_attested = true;
            rights.role = "owner".into();
        }
        if ctx.door != Door::Cli {
            return Err(Refusal::lq(
                "E406",
                "WV-051: body drop runs only through the CLI; no MCP tool, hook or Apply batch carries it",
            )
            .finish(None));
        }
        rights.verb("body-drop").map_err(|e| e.finish(None))?;
        if hashes.is_empty() || hashes.len() > MAX_HASHES {
            return Err(Refusal::usage_arg(
                "hashes",
                format!(
                    "body drop takes 1 to {MAX_HASHES} hashes, got {}",
                    hashes.len()
                ),
            ));
        }
        let mut named = BTreeSet::new();
        for h in hashes {
            named.insert(parse_hash(h).ok_or_else(|| {
                Refusal::usage_arg(
                    "hashes",
                    format!("{h} is not 32 lower-case hexadecimal digits"),
                )
            })?);
        }
        let reason = DropReason::parse(reason).ok_or_else(|| {
            Refusal::usage_arg(
                "reason",
                format!("{reason} is not a reason: secret, private or other"),
            )
        })?;
        if note.len() > MAX_NOTE {
            return Err(Refusal::usage_arg(
                "note",
                format!("the note is {} bytes, at most {MAX_NOTE}", note.len()),
            ));
        }
        let (already, fresh): (BTreeSet<[u8; 16]>, BTreeSet<[u8; 16]>) =
            named.into_iter().partition(|h| self.dropped.contains(h));
        let held = self.held_among(&fresh);
        if let Some(h) = fresh.iter().find(|h| !held.contains(*h)) {
            return Err(Refusal::not_found("body", crate::value::hex(h)));
        }
        if !fresh.is_empty() {
            // One `BodyDrop` record, one value of CK-4's sequence ([API §6.2]).
            self.hlc.record(self.env.wall_ms);
            for h in &fresh {
                self.dropped.insert(*h);
                self.drop_reasons.insert(*h, reason);
            }
        }
        let mut r = Reply::ok(Data::BodyDrop(Box::new(BodyDropData {
            dropped: fresh.into_iter().collect(),
            already: already.into_iter().collect(),
            backups: Vec::new(),
            purged: true,
        })));
        r.warnings = caller.warnings;
        Ok(r)
    }

    /// The hashes of `wanted` the store holds as a body ([F06 §8.1] DB-4): a body of a node, or of a conflict value's
    /// side, in the state of a commit the store still resolves (a `Gc` stops a commit resolving, [API §8.5]). One pass
    /// over the resolvable commits' states, stopping once every wanted hash is found: O(R·N) body hashes at most for R
    /// commits of N nodes.
    fn held_among(&self, wanted: &BTreeSet<[u8; 16]>) -> BTreeSet<[u8; 16]> {
        let mut found = BTreeSet::new();
        if wanted.is_empty() {
            return found;
        }
        for seq in self.dag.commits.keys() {
            if self.pruned.contains(seq) {
                continue;
            }
            let st = self.dag.state_at(Some(*seq), &self.alloc);
            for x in st.nodes.values() {
                for b in bodies(x) {
                    let h = crate::canon::body_hash(b);
                    if wanted.contains(&h) {
                        found.insert(h);
                        if found.len() == wanted.len() {
                            return found;
                        }
                    }
                }
            }
        }
        found
    }
}
