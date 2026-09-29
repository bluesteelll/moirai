//! System crashes ([F15 §2.5] "System crash"): the crash surface a caller enumerates, the crash plan it chooses, and the
//! crash image that materialises any number of post-crash worlds from one pre-crash state.
//!
//! The crash enumerator (WP-32) drives this API: it runs a scenario to a crash point, takes a [`CrashImage`] (or lets a
//! trigger capture one at a scheduling point, [`crate::SimWorld::capture_at`]), reads its [`CrashSurface`] — every file's
//! size history H(f) and non-clean sectors with their candidate counts, every pending namespace operation, every write in
//! flight — and materialises one world per [`CrashPlan`] it picks: every subset of ≤ 12 dirty sectors at baseline or
//! newest, random subsets beyond, the torn sector, the pending operations lost in any subset, the `HEAD` slots' 3 × 3
//! states ([F15 §6.4]). Anything a plan leaves open is resolved by its [`Resolve`] policy.

use std::collections::{BTreeMap, BTreeSet};

use moirai_vfs::BootId;

use crate::SimWorld;
use crate::adversary::{PartialWrite, SeededAdversary, Site};
use crate::content::{BeyondFill, CrashPick, SecState, SectorKind, SectorView};
use crate::namespace::NsKind;
use crate::rng::{Rng, splitmix};
use crate::trace::{EventKind, Trace};
use crate::world::{Chooser, DeathCause, Kernel, PointInfo, Sched, SimConfig, State, TState};

/// How a plan resolves every choice it does not name.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum Resolve {
    /// The world's adversary decides (seeded).
    #[default]
    Seeded,
    /// Everything reverts: every dirty sector keeps its baseline, every file its durable size, no pending operation
    /// survives, no sector tears, every in-flight write applied nothing; poisoned sub-sectors take their first candidate.
    Baseline,
    /// Everything persists: every dirty sector keeps its newest version, every file its current size, every pending
    /// operation survives, every in-flight write applied fully; poisoned sub-sectors take their last candidate.
    Newest,
}

/// What one sector keeps.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum SectorPick {
    /// The whole sector keeps candidate `i`: for a dirty sector 0 the baseline, i the version vᵢ; for a poisoned or
    /// dirty-over-poison sector every sub-sector takes candidate `i`.
    Version(u64),
    /// Each sub-sector keeps its own candidate. On a dirty sector this is the file's torn sector (at most one per file,
    /// FM-1.2); on a poisoned or dirty-over-poison sector it is unbounded (FM-3.3).
    Subsectors([u64; 8]),
}

/// The choices for one file.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct FilePlan {
    /// The size after the crash; must be a member of H(f).
    pub size: Option<u64>,
    /// Sector choices by sector index. When a plan names a file, its torn sector comes only from these picks.
    pub sectors: BTreeMap<u64, SectorPick>,
    /// The bytes beyond the old durable size.
    pub beyond: Option<BeyondFill>,
}

/// One post-crash state, as the caller chooses it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct CrashPlan {
    /// The policy for everything not named below.
    pub resolve: Resolve,
    /// The pending namespace operations that survive (by id); `None` leaves them to `resolve`.
    pub survivors: Option<BTreeSet<u64>>,
    /// Per-file choices, by node.
    pub files: BTreeMap<u64, FilePlan>,
    /// How each write in flight applied, by write id.
    pub in_flight: BTreeMap<u64, PartialWrite>,
    /// The identity of the next boot; `None` derives a fresh one.
    pub next_boot: Option<BootId>,
    /// Wall-clock time between the crash and the next boot, in ms.
    pub downtime_ms: u64,
}

impl CrashPlan {
    /// Every choice seeded.
    pub fn seeded() -> CrashPlan {
        CrashPlan::default()
    }

    /// Everything reverts (see [`Resolve::Baseline`]).
    pub fn baseline() -> CrashPlan {
        CrashPlan {
            resolve: Resolve::Baseline,
            ..CrashPlan::default()
        }
    }

    /// Everything persists (see [`Resolve::Newest`]).
    pub fn newest() -> CrashPlan {
        CrashPlan {
            resolve: Resolve::Newest,
            ..CrashPlan::default()
        }
    }

    /// Only the pending operations `ids` survive.
    pub fn with_survivors(mut self, ids: impl IntoIterator<Item = u64>) -> CrashPlan {
        self.survivors = Some(ids.into_iter().collect());
        self
    }

    /// Sets the plan of one file.
    pub fn with_file(mut self, node: u64, plan: FilePlan) -> CrashPlan {
        self.files.insert(node, plan);
        self
    }
}

/// One file as the crash surface shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSurface {
    /// The node (the key of [`CrashPlan::files`]).
    pub node: u64,
    /// Its current absolute paths (`/`-separated from the world root); empty for an unlinked file still open.
    pub paths: Vec<String>,
    /// ds(f).
    pub durable_size: u64,
    /// cs(f).
    pub size: u64,
    /// H(f), ascending.
    pub sizes: Vec<u64>,
    /// The non-clean sectors, ascending, except the rewritten ones.
    pub sectors: Vec<SectorView>,
    /// The rewritten sectors as half-open ranges of sector indices: `dirty`, with every candidate equal to their current
    /// content (written with the bytes they held, such as an extent's zero-fill). A crash leaves them as they are; a
    /// failed flush poisons them (FM-3.1).
    pub rewritten: Vec<(u64, u64)>,
}

impl FileSurface {
    /// The dirty sectors (the candidates for the torn sector and the subset enumeration of [F15 §6.4]).
    pub fn dirty(&self) -> impl Iterator<Item = &SectorView> {
        self.sectors.iter().filter(|s| s.state == SectorKind::Dirty)
    }
}

/// One pending namespace operation as the crash surface shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpSurface {
    /// The id (the members of [`CrashPlan::survivors`]).
    pub id: u64,
    /// What it is.
    pub kind: NsKind,
    /// The node it acts on.
    pub node: u64,
    /// The names it reads or writes, as absolute paths.
    pub names: Vec<String>,
    /// Per parent directory, whether a qualifying `sync_dir` has counted it (FM-2.3 (i)).
    pub synced: Vec<bool>,
}

/// One write in flight.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteSurface {
    /// The id (the key of [`CrashPlan::in_flight`]).
    pub id: u64,
    /// The writing process.
    pub proc: u32,
    /// The file.
    pub node: u64,
    /// The write's offset.
    pub offset: u64,
    /// Its length.
    pub len: u64,
}

/// Everything a crash may decide ([F15 §2.5], §6.4), in a deterministic order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrashSurface {
    /// The scheduling point the image was taken at.
    pub point: u64,
    /// Files with something to decide (a non-clean sector or more than one possible size).
    pub files: Vec<FileSurface>,
    /// Pending namespace operations in issue order.
    pub ops: Vec<OpSurface>,
    /// Writes in flight (applied, in part, before sectors resolve).
    pub writes: Vec<WriteSurface>,
}

impl CrashSurface {
    /// The file surface of `node`, if it has something to decide.
    pub fn file(&self, node: u64) -> Option<&FileSurface> {
        self.files.iter().find(|f| f.node == node)
    }
}

/// Why a crash plan was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanError {
    /// No file with this node exists.
    UnknownFile(u64),
    /// The size is not a member of H(f).
    SizeNotInHistory {
        /// The file.
        node: u64,
        /// The size asked.
        size: u64,
    },
    /// The sector is clean (a clean sector never changes, FM-1.3).
    CleanSector {
        /// The file.
        node: u64,
        /// The sector.
        sector: u64,
    },
    /// A candidate index at or beyond the sector's candidates.
    NoSuchCandidate {
        /// The file.
        node: u64,
        /// The sector.
        sector: u64,
        /// The index asked.
        index: u64,
        /// The candidates.
        candidates: u64,
    },
    /// More than one torn dirty sector in one file (FM-1.2).
    TwoTornSectors {
        /// The file.
        node: u64,
    },
    /// No pending operation has this id.
    UnknownOp(u64),
    /// No write in flight has this id.
    UnknownWrite(u64),
}

impl core::fmt::Display for PlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PlanError::UnknownFile(n) => write!(f, "no file node {n}"),
            PlanError::SizeNotInHistory { node, size } => {
                write!(f, "size {size} is not in H(f) of node {node}")
            }
            PlanError::CleanSector { node, sector } => {
                write!(f, "sector {sector} of node {node} is clean")
            }
            PlanError::NoSuchCandidate {
                node,
                sector,
                index,
                candidates,
            } => write!(
                f,
                "candidate {index} of sector {sector} of node {node}: only {candidates} exist"
            ),
            PlanError::TwoTornSectors { node } => {
                write!(f, "more than one torn sector in node {node}")
            }
            PlanError::UnknownOp(id) => write!(f, "no pending namespace operation {id}"),
            PlanError::UnknownWrite(id) => write!(f, "no write in flight {id}"),
        }
    }
}

impl std::error::Error for PlanError {}

/// A copy of a world's kernel at one instant, from which post-crash worlds are materialised.
#[derive(Clone)]
pub struct CrashImage {
    k: Kernel,
    rng: Rng,
    trace: Trace,
    cfg: SimConfig,
    next_table: u64,
    wait_mode: moirai_vfs::WaitMode,
    origin: Option<PointInfo>,
}

impl core::fmt::Debug for CrashImage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CrashImage")
            .field("point", &self.k.points)
            .finish()
    }
}

impl CrashImage {
    pub(crate) fn capture(st: &State) -> CrashImage {
        CrashImage {
            k: st.k.clone(),
            rng: st.ch.rng.clone(),
            trace: st.ch.trace.clone(),
            cfg: st.cfg.clone(),
            next_table: st.next_table,
            wait_mode: st.wait_mode,
            origin: None,
        }
    }

    /// The image with the scheduling point it was captured at.
    pub(crate) fn at(self, info: PointInfo) -> CrashImage {
        CrashImage {
            origin: Some(info),
            ..self
        }
    }

    /// The scheduling point at which the image was taken.
    pub fn point(&self) -> u64 {
        self.k.points
    }

    /// The scheduling point a capture trigger took the image at ([`crate::SimWorld::capture_at`],
    /// [`crate::SimWorld::capture_calls`]); `None` for [`crate::SimWorld::crash_image`].
    pub fn origin(&self) -> Option<PointInfo> {
        self.origin
    }

    /// The file flushes that had failed when the image was taken ([`crate::SimWorld::failed_flushes`]).
    pub fn failed_flushes(&self) -> u64 {
        self.k.failed_flushes
    }

    /// The node the absolute `path` names in the image's current namespace, if any (the key of
    /// [`CrashPlan::files`]).
    pub fn node_at(&self, path: &std::path::Path) -> Option<u64> {
        self.k.ns.lookup_abs(path).ok()
    }

    /// A copy whose generator is seeded with `seed`: its seeded crash choices (and the materialised world's later ones)
    /// differ from the original's, so one image yields many random post-crash states ([F15 §6.4] random tiers).
    pub fn reseeded(&self, seed: u64) -> CrashImage {
        CrashImage {
            rng: Rng::new(seed),
            ..self.clone()
        }
    }

    /// What a crash of this image may decide.
    pub fn surface(&self) -> CrashSurface {
        surface_of(&self.k)
    }

    /// The world after a system crash of this image resolved by `plan`, with a fresh seeded adversary (the image's
    /// generator state continues). The image is unchanged, so it can be materialised again with another plan.
    pub fn materialize(&self, plan: &CrashPlan) -> Result<SimWorld, PlanError> {
        let adv = SeededAdversary::new(self.cfg.rates, self.cfg.release_law.clone());
        self.materialize_with(plan, Box::new(adv))
    }

    /// As [`CrashImage::materialize`], with the given adversary for the new world (and the plan's seeded choices).
    pub fn materialize_with(
        &self,
        plan: &CrashPlan,
        adv: Box<dyn crate::Adversary>,
    ) -> Result<SimWorld, PlanError> {
        validate(&self.k, plan)?;
        let mut st = State {
            cfg: self.cfg.clone(),
            ch: Chooser {
                adv,
                rng: self.rng.clone(),
                trace: self.trace.clone(),
                queue: Vec::new(),
            },
            k: self.k.clone(),
            sched: Sched::default(),
            tables: BTreeMap::new(),
            next_table: self.next_table,
            wait_mode: self.wait_mode,
            triggers: Vec::new(),
            captures: Vec::new(),
            point_log: None,
            busy_log: Vec::new(),
            violations: Vec::new(),
            stderr: Vec::new(),
            spawns: Vec::new(),
            release_drawn: [0; 3],
        };
        apply_crash(&mut st, plan);
        Ok(SimWorld::from_state(st))
    }
}

pub(crate) fn surface_of(k: &Kernel) -> CrashSurface {
    let mut files = Vec::new();
    for (&node, n) in &k.ns.nodes {
        let Some(f) = n.file() else {
            continue;
        };
        let sizes = f.content.sizes();
        if !f.content.has_unflushed() && sizes.len() <= 1 {
            continue;
        }
        files.push(FileSurface {
            node,
            paths: k.ns.paths_of(node),
            durable_size: f.content.ds,
            size: f.content.cs(),
            sizes,
            sectors: f.content.sector_views(),
            rewritten: f.content.same.ranges(),
        });
    }
    let dir_path = |d: u64| -> String {
        if d == crate::namespace::ROOT {
            String::new()
        } else {
            k.ns.paths_of(d)
                .into_iter()
                .next()
                .unwrap_or_else(|| format!("/<node {d}>"))
        }
    };
    let ops =
        k.ns.pending
            .iter()
            .map(|p| OpSurface {
                id: p.id,
                kind: p.op.kind(),
                node: p.op.node(),
                names: p
                    .op
                    .names()
                    .into_iter()
                    .map(|(d, name)| format!("{}/{}", dir_path(d), name))
                    .collect(),
                synced: p.synced.clone(),
            })
            .collect();
    let writes = k
        .writes
        .iter()
        .map(|w| WriteSurface {
            id: w.id,
            proc: w.proc,
            node: w.node,
            offset: w.offset,
            len: w.data.as_slice().len() as u64,
        })
        .collect();
    CrashSurface {
        point: k.points,
        files,
        ops,
        writes,
    }
}

fn validate(k: &Kernel, plan: &CrashPlan) -> Result<(), PlanError> {
    for (&node, fp) in &plan.files {
        let f =
            k.ns.nodes
                .get(&node)
                .and_then(|n| n.file())
                .ok_or(PlanError::UnknownFile(node))?;
        if let Some(size) = fp.size
            && !f.content.sizes().contains(&size)
        {
            return Err(PlanError::SizeNotInHistory { node, size });
        }
        let mut torn = 0;
        for (&sector, pick) in &fp.sectors {
            let st = f
                .content
                .secs
                .get(&sector)
                .ok_or(PlanError::CleanSector { node, sector })?;
            let (candidates, dirty) = match st {
                SecState::Dirty {
                    base,
                    over_poison,
                    versions,
                    ..
                } => ((base.len() + versions.len()) as u64, !over_poison),
                SecState::Poisoned { k } => (k.len() as u64, false),
            };
            let check = |index: u64| {
                if index >= candidates {
                    Err(PlanError::NoSuchCandidate {
                        node,
                        sector,
                        index,
                        candidates,
                    })
                } else {
                    Ok(())
                }
            };
            match pick {
                SectorPick::Version(v) => check(*v)?,
                SectorPick::Subsectors(subs) => {
                    for &v in subs {
                        check(v)?;
                    }
                    if dirty {
                        torn += 1;
                    }
                }
            }
        }
        if torn > 1 {
            return Err(PlanError::TwoTornSectors { node });
        }
    }
    if let Some(ids) = &plan.survivors {
        for &id in ids {
            if !k.ns.pending.iter().any(|p| p.id == id) {
                return Err(PlanError::UnknownOp(id));
            }
        }
    }
    for &id in plan.in_flight.keys() {
        if !k.writes.iter().any(|w| w.id == id) {
            return Err(PlanError::UnknownWrite(id));
        }
    }
    Ok(())
}

/// The choices of one file's resolution: the plan's, then the policy's.
struct PlanPick<'a> {
    plan: Option<&'a FilePlan>,
    resolve: Resolve,
    ch: &'a mut Chooser,
    node: u64,
    ds: u64,
    cs: u64,
}

impl CrashPick for PlanPick<'_> {
    fn size(&mut self, sizes: &[u64]) -> usize {
        let want = match (self.plan.and_then(|p| p.size), self.resolve) {
            (Some(s), _) => s,
            (None, Resolve::Baseline) => self.ds,
            (None, Resolve::Newest) => self.cs,
            (None, Resolve::Seeded) => {
                return self
                    .ch
                    .pick(Site::CrashSize, u32::MAX, self.node, 0, sizes.len() as u64)
                    as usize;
            }
        };
        // Both policies' sizes are members of H(f) (ds by definition, cs by FM-2.2), and a plan's size was validated.
        sizes.iter().position(|&s| s == want).unwrap_or_else(|| {
            panic!(
                "simulator: crash size {want} of node {} is not in H(f) {sizes:?}",
                self.node
            )
        })
    }

    fn torn(&mut self, dirty: &[u64]) -> Option<u64> {
        if let Some(p) = self.plan {
            return p
                .sectors
                .iter()
                .find(|(s, pick)| matches!(pick, SectorPick::Subsectors(_)) && dirty.contains(s))
                .map(|(&s, _)| s);
        }
        match self.resolve {
            Resolve::Seeded if !dirty.is_empty() => {
                let v = self.ch.pick(
                    Site::CrashTorn,
                    u32::MAX,
                    self.node,
                    0,
                    dirty.len() as u64 + 1,
                );
                (v > 0).then(|| dirty[v as usize - 1])
            }
            _ => None,
        }
    }

    fn sector(&mut self, s: u64, n: u64) -> u64 {
        match self.plan.and_then(|p| p.sectors.get(&s)) {
            Some(SectorPick::Version(v)) => *v,
            Some(SectorPick::Subsectors(subs)) => subs[0],
            None => match self.resolve {
                Resolve::Seeded => self.ch.pick(Site::CrashSector, u32::MAX, self.node, s, n),
                Resolve::Baseline => 0,
                Resolve::Newest => n - 1,
            },
        }
    }

    fn sub(&mut self, s: u64, j: usize, n: u64) -> u64 {
        match self.plan.and_then(|p| p.sectors.get(&s)) {
            Some(SectorPick::Version(v)) => *v,
            Some(SectorPick::Subsectors(subs)) => subs[j],
            None => match self.resolve {
                Resolve::Seeded => {
                    self.ch
                        .pick(Site::CrashSub, u32::MAX, self.node, s * 8 + j as u64, n)
                }
                Resolve::Baseline => 0,
                Resolve::Newest => n - 1,
            },
        }
    }

    fn beyond(&mut self) -> BeyondFill {
        if let Some(b) = self.plan.and_then(|p| p.beyond) {
            return b;
        }
        match self.resolve {
            Resolve::Seeded => match self.ch.pick(Site::CrashBeyond, u32::MAX, self.node, 0, 3) {
                0 => BeyondFill::Resolved,
                1 => BeyondFill::Zeros,
                _ => BeyondFill::Garbage(self.ch.pick(
                    Site::CrashGarbage,
                    u32::MAX,
                    self.node,
                    0,
                    u64::MAX,
                )),
            },
            _ => BeyondFill::Resolved,
        }
    }
}

/// Crashes the live world in place.
pub(crate) fn crash_in_place(st: &mut State, plan: &CrashPlan) -> Result<(), PlanError> {
    validate(&st.k, plan)?;
    apply_crash(st, plan);
    Ok(())
}

/// The system crash of [F15 §2.5], steps 1–5.
fn apply_crash(st: &mut State, plan: &CrashPlan) {
    let point = st.k.points;
    // Step 1: every process dies; nothing it had in progress completes, except that a write in flight may have applied
    // in part.
    for i in 0..st.k.procs.len() {
        if st.k.procs[i].alive {
            st.k.procs[i].alive = false;
            st.k.procs[i].death = Some(DeathCause::SystemCrash);
            st.k.procs[i].scripted = std::collections::VecDeque::new();
            st.ev(
                EventKind::ProcEnd,
                None,
                i as u32,
                DeathCause::SystemCrash as u64,
                0,
                0,
            );
        }
        st.k.procs[i].maps = 0;
    }
    let writes = core::mem::take(&mut st.k.writes);
    for w in writes {
        let pw = match (plan.in_flight.get(&w.id), plan.resolve) {
            (Some(pw), _) => *pw,
            (None, Resolve::Baseline) => PartialWrite::Nothing,
            (None, Resolve::Newest) => PartialWrite::All,
            (None, Resolve::Seeded) => PartialWrite::from_choice(st.pick(
                Site::CrashPartial,
                w.proc,
                w.node,
                w.offset,
                u64::MAX,
            )),
        };
        st.apply_partial(w.proc, &w, pw);
        st.ev(EventKind::InFlight, None, w.proc, w.node, 0, pw.to_choice());
    }
    st.k.reads.clear();
    st.k.flushes.clear();
    st.k.handles.clear();
    st.tables.clear();
    // Step 4 (lock part): every byte is free.
    st.k.locks.clear();
    for t in st.sched.tasks.values_mut() {
        if t.state == TState::Blocked {
            t.state = TState::Runnable;
        }
    }
    // Step 3: the namespace.
    let survivors = {
        let State { ch, k, .. } = &mut *st;
        let resolve = plan.resolve;
        k.ns.crash(&mut |p| match (&plan.survivors, resolve) {
            (Some(ids), _) => ids.contains(&p.id),
            (None, Resolve::Baseline) => false,
            (None, Resolve::Newest) => true,
            (None, Resolve::Seeded) => ch.pick(Site::CrashOp, u32::MAX, p.op.node(), p.id, 2) == 1,
        })
    };
    // Step 2: file contents, in node order.
    let nodes: Vec<u64> =
        st.k.ns
            .nodes
            .iter()
            .filter(|(_, n)| n.file().is_some())
            .map(|(&id, _)| id)
            .collect();
    let mut resolved = 0;
    for node in &nodes {
        let State { ch, k, .. } = &mut *st;
        let Some(f) = k.ns.nodes.get(node).and_then(|n| n.file()) else {
            continue;
        };
        let c = &f.content;
        if c.secs.is_empty() && c.same.is_empty() && c.sizes().len() <= 1 {
            continue;
        }
        resolved += 1;
        let (ds, cs) = (c.ds, c.cs());
        let mut pick = PlanPick {
            plan: plan.files.get(node),
            resolve: plan.resolve,
            ch,
            node: *node,
            ds,
            cs,
        };
        k.ns.edit(*node, |c| c.crash(&mut pick));
    }
    st.ev(EventKind::Crash, None, u32::MAX, point, survivors, resolved);
    // Step 4: a new boot; the clocks restart; the wall clock went on.
    let c = &st.k.clock;
    let wall_now = c.wall_ms(0);
    let mut sm = st.cfg.seed ^ u64::from(c.boot_seq + 1).wrapping_mul(0x9E37_79B9);
    let boot_id = plan.next_boot.unwrap_or_else(|| {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&splitmix(&mut sm).to_le_bytes());
        b[8..].copy_from_slice(&splitmix(&mut sm).to_le_bytes());
        BootId(b)
    });
    let boot_origin = 1_000_000_000 + (splitmix(&mut sm) % 1_000_000_000);
    let mono_origin = splitmix(&mut sm) % (1 << 40);
    let seq = c.boot_seq + 1;
    st.k.clock = crate::world::Clocks {
        boot_seq: seq,
        boot_id,
        boot_ns: boot_origin,
        mono_ns: mono_origin,
        boot_origin_ns: boot_origin,
        wall_at_boot_ms: wall_now.saturating_add(plan.downtime_ms as i64),
    };
    // Kernel wait objects and parent wakes belong to dead processes.
    st.k.wakes.clear();
    st.ev(
        EventKind::Boot,
        None,
        u32::MAX,
        u64::from(seq),
        boot_id.hash(),
        boot_origin,
    );
}
