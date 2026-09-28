//! The random source ([OS/README §4.6]): the seventh sub-trait of `Vfs`, through which every random value moirai stores
//! or names a file with is drawn — the store id, the epoch, `tmp/` and user-file nonces, `LeaderRec.nonce`,
//! `SlotRec.nonce` and random uids ([F02 §4, §5.3], [F03 §7.1, §8.1], [F04 §5.3], [F08 §2.2], [CFG §7.5]).
//!
//! The OS call stays inside `moirai-os` ([80 §1] X3); the simulator draws from the test seed, one stream per simulated
//! process, so that every draw replays ([F15 §6.4] "Determinism", A-7).

/// The cryptographically secure random source; a supertrait of `Vfs` ([OS/README §4.1, §4.6]). Implemented by
/// `moirai_os::OsVfs` (module `os::proc`: `BCryptGenRandom` with the system-preferred RNG, `getrandom(…, 0)`,
/// `getentropy`) and by `moirai_vfs_sim::SimVfs`.
pub trait Entropy {
    /// Fills all of `buf` with bytes from the OS's cryptographically secure random source. Never returns an error and
    /// never fills less than `buf.len()` bytes: an impossible failure of the OS call panics, naming the call and the OS
    /// code ([OS/README §4.6] "Failure rule"). Each value is drawn with one call of exactly its width; the caller, not
    /// this call, draws again where the owning chapter says so.
    fn fill_random(&self, buf: &mut [u8]);
}
