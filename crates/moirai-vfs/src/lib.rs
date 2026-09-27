//! The storage seam of moirai: the `Vfs` trait (durability classes, `sync_dir`, `sync_group`, `rename_noreplace` and
//! `rename_replace`, `swap_dirs`, the `map_sealed` contract, the environment-guard types), the complete `ProjectFs`
//! trait (read and write side), the `Meter` trait (free space, available physical memory, child peak, heap high-water
//! mark), `LockBytes` with the target-independent in-process grant table, the wall, monotonic and boot clocks, and the
//! evidence types `ProcId`, `BootId`, `Liveness`, `OsFileId` and `VolumeCaps`.
//!
//! Product crate with no dependencies, shared by the simulator (`moirai-vfs-sim`) and the OS layer (`moirai-os`);
//! checked by GT20 (e) on every target. Filled by WP-30 (R-HARN-S) to the signatures of `docs/spec/os/` (WP-17).
//! Sources: [80 §2.1–§2.3, §2.7, §2.11]; `docs/m0/PLAN.md` §2.2, §6.2 R2.
