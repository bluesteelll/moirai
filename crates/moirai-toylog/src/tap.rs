//! Protocol notes for the trace predicates of [F13 §1.4] (`crash::trace::ig4_flush_discipline`,
//! `crash::trace::ig6_publish_monotone`) and of [F16 §17.2] "trace" rows: the toy reports each protocol step that no
//! `Vfs` event names (a phase, a re-write, a publish's fields) through a [`Tap`]. A harness on the simulator turns every
//! note into a trace note ([F13 §1.4]: "the protocol events of [F16]"); elsewhere [`NoTap`] drops them.
//!
//! A note is emitted by the process that performs the step, right after the step's last call returns (or right before
//! its first call, for a `begin`), so notes interleave with the `Vfs` events of that process exactly.

/// A protocol step that begins or ends: which one, and the lock client of the handle that performs it (the id of
/// `moirai_vfs::ClientId`, 0 while the handle has not been granted a byte), so a predicate can pair it with the lock
/// events of that client ([F13 §1.4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Step {
    /// `true` at the beginning, `false` at the end.
    pub begin: bool,
    /// The handle's lock client.
    pub client: u64,
}

/// One protocol note.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Note {
    /// Phase 1 of a write ([F16] P-25).
    Phase1(Step),
    /// Phase 3 (maintenance after the acknowledgement) ([F16] P-51).
    Phase3(Step),
    /// The flush holder's scan and re-write of the pending range ([F16] P-42, P-43).
    Rewrite(Step),
    /// A log flush (`durable` on the extents of the flushed range) ([F16] P-44).
    LogFlush(Step),
    /// A `HEAD` flush ([F16] P-13).
    HeadFlush(Step),
    /// A slot was written by a publish ([F16] P-48).
    Publish(PublishNote),
    /// A group with a `Checkpoint` record was appended ([F03 §3.1] rule 2, L-8; [F16] P-76: by the holder of the
    /// maintenance byte).
    CheckpointAppended(Step),
}

/// What a publish wrote and what it read ([F16] P-48–P-50, I-G6).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishNote {
    /// The base slot's `slot_seq`.
    pub base_seq: u64,
    /// The written slot's `slot_seq`.
    pub new_seq: u64,
    /// The base slot's `durable_lsn`.
    pub base_durable: u64,
    /// The written slot's `durable_lsn`.
    pub new_durable: u64,
    /// The base slot's `committed_lsn`.
    pub base_committed: u64,
    /// The written slot's `committed_lsn`.
    pub new_committed: u64,
    /// The base slot's `checkpoint_lsn`.
    pub base_checkpoint: u64,
    /// The written slot's `checkpoint_lsn`.
    pub new_checkpoint: u64,
    /// The base slot's `fence`.
    pub base_fence: u64,
    /// The written slot's `fence`.
    pub new_fence: u64,
    /// The first 8 bytes of the base slot's `boot_id`.
    pub base_boot: u64,
    /// The first 8 bytes of the written slot's `boot_id`.
    pub new_boot: u64,
    /// The publish follows a log flush of this holding (it may raise `durable_lsn`).
    pub flushed: bool,
    /// The publish is boot-change recovery's first write (it may change `boot_id`).
    pub boot_change: bool,
    /// The publisher's boot identity is Known.
    pub known_boot: bool,
    /// The slot written: 0 = A, 1 = B; and the slot the base came from.
    pub slots: (u8, u8),
}

/// The sink of the toy's protocol notes.
pub trait Tap: Clone + Send + Sync + 'static {
    /// Records one note.
    fn note(&self, n: Note);
}

/// A tap that drops every note (the real `Vfs`, measurements).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct NoTap;

impl Tap for NoTap {
    fn note(&self, _n: Note) {}
}
