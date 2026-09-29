//! What an OS crash may lose ([CFG §10.3] `durability.lazy-kinds`; [F05 §6.1]; [AR §6.5]): the records of a kind in
//! the lazy set are acknowledged before they are durable, so a crash may lose the newest of them; every other record,
//! graph mutations always, survives once acknowledged ([60 §4.4] item 4). GT3 compares the engine after a crash with
//! the model's candidate states; this module decides which of the model's lazy effects a candidate may lack.

/// The record kinds the key governs ([CFG §10.3]): lease heartbeats, read cursors and session marks.
pub const KINDS: [&str; 3] = ["heartbeat", "cursor", "session-mark"];

/// Whether an acknowledged record of `kind` survives an OS crash under `durability.lazy-kinds` = `lazy` (its canonical
/// comma-separated set): a kind the set lists may be lost, any other survives. Graph mutations and every other record
/// kind are durable by rule; R4 runtime evidence is lazy by rule ([CFG §10.3]).
// spec: [CFG §10.3] durability.lazy-kinds
pub fn survives(kind: &str, lazy: &str) -> bool {
    match kind {
        "heartbeat" | "cursor" | "session-mark" => !lazy.split(',').any(|k| k == kind),
        "evidence" => false,
        _ => true,
    }
}

/// The deadlines a lease may have after an OS crash that follows a renewal by a lazy heartbeat record (`Lazy`, causes
/// 1 and 2, [F05 §9.11]): the renewed one, and, when `heartbeat` is lazy, the one before it. A renewal by a durable
/// `Lease` record of event 4 (an expired lease renewed by its holder, I17′) always survives.
// spec: [API §10.2]
pub fn deadlines_after_crash(before: u64, after: u64, durable_event: bool, lazy: &str) -> Vec<u64> {
    if durable_event || survives("heartbeat", lazy) {
        vec![after]
    } else {
        vec![after, before]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_kinds_may_be_lost() {
        let all = "heartbeat,cursor,session-mark";
        assert!(!survives("heartbeat", all) && survives("heartbeat", "cursor"));
        assert!(survives("commit", ""), "graph mutations are always durable");
        assert!(!survives("evidence", ""), "R4 evidence is always lazy");
        assert_eq!(deadlines_after_crash(1, 2, false, all), [2, 1]);
        assert_eq!(deadlines_after_crash(1, 2, false, ""), [2]);
        assert_eq!(deadlines_after_crash(1, 2, true, all), [2]);
    }
}
