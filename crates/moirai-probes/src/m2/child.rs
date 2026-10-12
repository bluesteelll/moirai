//! Measurement 2's child writers: the line protocol between the coordinator and a child, the child's loop [`serve`]
//! over a [`ChildRig`], and [`ProcessWriters`], the pool of `m2 --writer` processes.
//!
//! **The protocol** (one ASCII line each way; a store path is the rest of its line, so it may hold spaces):
//! - `go <op> <start_ns> <cost_ns> <filler> <dir>`: commit op `op` with `filler` filler bytes and the in-lock cost
//!   `cost_ns` to the store in `dir`, starting at `start_ns` on the system-wide monotonic clock;
//! - `ack <op> <t_start> <t_ack> <sync_data> <sync_meta> <bytes_read> <phase1_ns> <event>…`, an event being
//!   `W<req>:<grant>`, `F<req>:<grant>`, `w<t>` or `f<t>` (a writer or flush grant or release);
//! - `err <op> <message>`;
//! - `quit`, or the end of the input: the child exits 0.
//!
//! A child's stderr is the coordinator's (never a pipe nobody drains); its stdout carries only replies.

use super::burst::{Ack, Writers};
use super::locks::{Byte, LockEvent, LockVfs, PhaseTap};
use crate::m1::store::{Rig, ToyRig};
use moirai_vfs::Vfs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// One `go` line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Go {
    /// The commit's operation.
    pub op: u64,
    /// T: when the child starts.
    pub start_ns: u64,
    /// The in-lock cost of the commit's `Config`.
    pub cost_ns: u64,
    /// The filler bytes.
    pub filler: u32,
    /// The store.
    pub dir: PathBuf,
}

impl Go {
    /// The line, without its line feed.
    pub fn line(&self) -> String {
        format!(
            "go {} {} {} {} {}",
            self.op,
            self.start_ns,
            self.cost_ns,
            self.filler,
            self.dir.to_string_lossy()
        )
    }

    /// Parses a `go` line.
    pub fn parse(line: &str) -> Result<Go, String> {
        let bad = || format!("not a go line: '{line}'");
        let rest = line.strip_prefix("go ").ok_or_else(bad)?;
        let mut w = rest.splitn(5, ' ');
        let mut num =
            || -> Result<u64, String> { w.next().and_then(|x| x.parse().ok()).ok_or_else(bad) };
        let (op, start_ns, cost_ns) = (num()?, num()?, num()?);
        let filler = u32::try_from(num()?).map_err(|_| bad())?;
        let dir = w.next().filter(|d| !d.is_empty()).ok_or_else(bad)?;
        Ok(Go {
            op,
            start_ns,
            cost_ns,
            filler,
            dir: PathBuf::from(dir),
        })
    }
}

/// A child's reply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    /// The commit was acknowledged.
    Ack(Ack),
    /// It failed.
    Err {
        /// The operation.
        op: u64,
        /// Why.
        message: String,
    },
}

impl Reply {
    /// The line, without its line feed.
    pub fn line(&self) -> String {
        match self {
            Reply::Ack(a) => {
                let mut s = format!(
                    "ack {} {} {} {} {} {} {}",
                    a.op, a.t_start, a.t_ack, a.sync_data, a.sync_meta, a.bytes_read, a.phase1_ns
                );
                for e in &a.events {
                    s.push(' ');
                    s.push_str(&match *e {
                        LockEvent::Grant {
                            byte: Byte::Writer,
                            req,
                            grant,
                        } => format!("W{req}:{grant}"),
                        LockEvent::Grant {
                            byte: Byte::Flush,
                            req,
                            grant,
                        } => format!("F{req}:{grant}"),
                        LockEvent::Release {
                            byte: Byte::Writer,
                            t,
                        } => format!("w{t}"),
                        LockEvent::Release {
                            byte: Byte::Flush,
                            t,
                        } => format!("f{t}"),
                    });
                }
                s
            }
            Reply::Err { op, message } => {
                format!("err {op} {}", message.replace(['\r', '\n'], " "))
            }
        }
    }

    /// Parses an `ack` or `err` line.
    pub fn parse(line: &str) -> Result<Reply, String> {
        let bad = || format!("not a reply: '{line}'");
        if let Some(rest) = line.strip_prefix("err ") {
            let (op, message) = rest.split_once(' ').unwrap_or((rest, ""));
            return Ok(Reply::Err {
                op: op.parse().map_err(|_| bad())?,
                message: message.to_string(),
            });
        }
        let rest = line.strip_prefix("ack ").ok_or_else(bad)?;
        let mut w = rest.split(' ');
        let mut num =
            || -> Result<u64, String> { w.next().and_then(|x| x.parse().ok()).ok_or_else(bad) };
        let mut a = Ack {
            op: num()?,
            t_start: num()?,
            t_ack: num()?,
            sync_data: num()?,
            sync_meta: num()?,
            bytes_read: num()?,
            phase1_ns: num()?,
            events: Vec::new(),
        };
        for e in w {
            let pair = |s: &str| -> Option<(u64, u64)> {
                let (r, g) = s.split_once(':')?;
                Some((r.parse().ok()?, g.parse().ok()?))
            };
            let ev = match e.split_at_checked(1) {
                Some(("W", s)) => pair(s).map(|(req, grant)| LockEvent::Grant {
                    byte: Byte::Writer,
                    req,
                    grant,
                }),
                Some(("F", s)) => pair(s).map(|(req, grant)| LockEvent::Grant {
                    byte: Byte::Flush,
                    req,
                    grant,
                }),
                Some(("w", s)) => s.parse().ok().map(|t| LockEvent::Release {
                    byte: Byte::Writer,
                    t,
                }),
                Some(("f", s)) => s.parse().ok().map(|t| LockEvent::Release {
                    byte: Byte::Flush,
                    t,
                }),
                _ => None,
            };
            a.events.push(ev.ok_or_else(bad)?);
        }
        Ok(Reply::Ack(a))
    }
}

/// A child writer's [`Rig`], which also reports each commit's lock events and phase-1 time.
pub trait ChildRig: Rig {
    /// The lock events and the phase-1 time (ns) since the last call, taken.
    fn drain(&self) -> (Vec<LockEvent>, u64);
}

/// The child writer's real rig: the toy log over [`LockVfs`] of `vfs`, its handles timed by a [`PhaseTap`].
pub type ChildToyRig<V> = ToyRig<LockVfs<V>, PhaseTap<LockVfs<V>>>;

/// The child writer's real rig over `vfs`.
pub fn child_rig<V: Vfs + Clone>(vfs: V) -> ChildToyRig<V> {
    let v = LockVfs::new(vfs);
    ToyRig::with_tap(v.clone(), PhaseTap::new(v))
}

impl<V: Vfs + Clone> ChildRig for ChildToyRig<V> {
    fn drain(&self) -> (Vec<LockEvent>, u64) {
        (self.vfs.log().drain(), self.tap.drain_ns())
    }
}

/// One `go`: wait until its start, then one CLI commit — open with the draw, commit, stamp the acknowledgement, read the
/// counters and lock events, close — and the reply. A commit whose acknowledgement came sooner than its cost is an
/// `err`.
pub fn serve_one<R: ChildRig>(rig: &R, go: &Go) -> Reply {
    let _ = rig.drain();
    rig.wait_until(go.start_ns);
    let t_start = rig.mono_ns();
    let c0 = rig.counters();
    let run = || -> Result<Ack, String> {
        let mut h = rig.open(&go.dir, go.cost_ns)?;
        let done = rig.commit(&mut h, go.op, go.filler);
        let t_ack = rig.mono_ns();
        let c1 = rig.counters();
        let (events, phase1_ns) = rig.drain();
        rig.close(h);
        done?;
        Ok(Ack {
            op: go.op,
            t_start,
            t_ack,
            sync_data: c1.sync_data.saturating_sub(c0.sync_data),
            sync_meta: c1.sync_meta.saturating_sub(c0.sync_meta),
            bytes_read: c1.bytes_read.saturating_sub(c0.bytes_read),
            phase1_ns,
            events,
        })
    };
    match run() {
        Ok(a) if a.t_ack.saturating_sub(a.t_start) < go.cost_ns => Reply::Err {
            op: go.op,
            message: format!(
                "acknowledged {} ns after its start, below its in-lock cost of {} ns",
                a.t_ack.saturating_sub(a.t_start),
                go.cost_ns
            ),
        },
        Ok(a) => Reply::Ack(a),
        Err(message) => Reply::Err { op: go.op, message },
    }
}

/// The child's loop (`m2 --writer`): one reply per `go`, flushed at once; `quit` or the end of the input ends it with
/// 0, an unreadable input or output with 1.
pub fn serve<R: ChildRig>(rig: &R, input: &mut dyn BufRead, output: &mut dyn Write) -> u8 {
    let mut line = String::new();
    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => return 0,
            Ok(_) => {}
            Err(_) => return 1,
        }
        let l = line.trim_end();
        if l == "quit" {
            return 0;
        }
        let reply = match Go::parse(l) {
            Ok(go) => serve_one(rig, &go),
            Err(message) => Reply::Err { op: 0, message },
        };
        if writeln!(output, "{}", reply.line())
            .and_then(|()| output.flush())
            .is_err()
        {
            return 1;
        }
    }
}

/// The pool of child writers: `<exe> --writer` processes with piped stdin and stdout, one reader thread per child
/// forwarding its lines. Dropping it sends `quit`, closes the pipes, waits up to 5 s for each child and kills the
/// rest.
pub struct ProcessWriters {
    children: Vec<Child>,
    stdins: Vec<ChildStdin>,
    rx: Receiver<(usize, Result<String, String>)>,
}

impl ProcessWriters {
    /// Starts `n` children of `exe` in `cwd`.
    pub fn spawn(exe: &Path, n: usize, cwd: &Path) -> Result<ProcessWriters, String> {
        let (tx, rx) = channel();
        let mut pool = ProcessWriters {
            children: Vec::with_capacity(n),
            stdins: Vec::with_capacity(n),
            rx,
        };
        for i in 0..n {
            let mut child = Command::new(exe)
                .arg("--writer")
                .current_dir(cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| format!("starting writer {i}: {e}"))?;
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("writer {i} has no pipes"));
            };
            pool.children.push(child);
            pool.stdins.push(stdin);
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let msg = line.map_err(|e| e.to_string());
                    let end = msg.is_err();
                    if tx.send((i, msg)).is_err() || end {
                        return;
                    }
                }
                let _ = tx.send((i, Err("the writer closed its output".into())));
            });
        }
        Ok(pool)
    }

    /// The children's process ids.
    pub fn pids(&self) -> Vec<u32> {
        self.children.iter().map(Child::id).collect()
    }
}

impl Writers for ProcessWriters {
    fn count(&self) -> usize {
        self.children.len()
    }

    fn send(&mut self, i: usize, go: &Go) -> Result<(), String> {
        let s = &mut self.stdins[i];
        writeln!(s, "{}", go.line())
            .and_then(|()| s.flush())
            .map_err(|e| format!("writer {i}: {e}"))
    }

    fn recv(&mut self, timeout: Duration) -> Result<(usize, Reply), String> {
        match self.rx.recv_timeout(timeout) {
            Ok((i, Ok(line))) => Reply::parse(&line).map(|r| (i, r)),
            Ok((i, Err(e))) => Err(format!("writer {i}: {e}")),
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("no reply within {} s", timeout.as_secs()))
            }
            Err(RecvTimeoutError::Disconnected) => Err("every writer is gone".into()),
        }
    }
}

impl Drop for ProcessWriters {
    fn drop(&mut self) {
        for s in &mut self.stdins {
            let _ = writeln!(s, "quit").and_then(|()| s.flush());
        }
        self.stdins.clear();
        let deadline = Instant::now() + Duration::from_secs(5);
        for c in &mut self.children {
            loop {
                match c.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => {
                        let _ = c.kill();
                        let _ = c.wait();
                        break;
                    }
                }
            }
        }
    }
}

/// The test double of the pool: each `send` runs that child's real [`serve_one`] at once on a `FakeRig` of its own
/// (its clock first brought to the coordinator's time, as a `go` arrives then), and queues the reply; `recv` returns
/// the queue in order and brings the coordinator's clock to the replying child's.
#[cfg(test)]
pub(crate) mod fake {
    use super::{Go, Reply, serve_one};
    use crate::m1::store::fake::{Ev, FakeRig};
    use crate::m2::burst::Writers;
    use crate::m2::locks::LockEvent;
    use std::collections::{BTreeMap, VecDeque};
    use std::time::Duration;

    impl super::ChildRig for FakeRig {
        fn drain(&self) -> (Vec<LockEvent>, u64) {
            (self.drain_events(), 0)
        }
    }

    pub(crate) struct FakeWriters {
        pub coord: FakeRig,
        pub children: Vec<FakeRig>,
        queue: VecDeque<(usize, Reply)>,
        /// A child whose `go` for this op arrives this many ns after T.
        pub late: BTreeMap<u64, u64>,
        /// Children that never reply.
        pub silent: Vec<usize>,
        /// The op a child's ack names instead of its own, by its own.
        pub misreport: BTreeMap<u64, u64>,
    }

    impl FakeWriters {
        pub fn new(coord: &FakeRig, n: usize) -> FakeWriters {
            FakeWriters {
                coord: coord.clone(),
                children: (0..n).map(|_| coord.child()).collect(),
                queue: VecDeque::new(),
                late: BTreeMap::new(),
                silent: Vec::new(),
                misreport: BTreeMap::new(),
            }
        }
    }

    impl Writers for FakeWriters {
        fn count(&self) -> usize {
            self.children.len()
        }

        fn send(&mut self, i: usize, go: &Go) -> Result<(), String> {
            self.coord.world.borrow_mut().trace.push(Ev::Go {
                child: i,
                op: go.op,
            });
            let child = &self.children[i];
            child.catch_up(self.coord.now());
            if let Some(late) = self.late.get(&go.op) {
                child.catch_up(go.start_ns + late);
            }
            let mut reply = serve_one(child, go);
            if let (Reply::Ack(a), Some(&op)) = (&mut reply, self.misreport.get(&go.op)) {
                a.op = op;
            }
            if !self.silent.contains(&i) {
                self.queue.push_back((i, reply));
            }
            Ok(())
        }

        fn recv(&mut self, _timeout: Duration) -> Result<(usize, Reply), String> {
            let (i, r) = self.queue.pop_front().ok_or("no reply within 60 s")?;
            let op = match &r {
                Reply::Ack(a) => a.op,
                Reply::Err { op, .. } => *op,
            };
            self.coord
                .world
                .borrow_mut()
                .trace
                .push(Ev::Ack { child: i, op });
            let t = self.children[i].now();
            self.coord.catch_up(t);
            Ok((i, r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m1::store::fake::{Ev, FakeRig};
    use crate::m2::burst::tests::{wg, wr};

    fn go(op: u64, start: u64, cost: u64) -> Go {
        Go {
            op,
            start_ns: start,
            cost_ns: cost,
            filler: 450,
            dir: PathBuf::from("/w/m2.1/store.1"),
        }
    }

    /// A world with the store `/w/m2.1/store.1` (made at time 0) and a child process on it.
    fn child_world() -> (FakeRig, FakeRig) {
        let coord = FakeRig::new();
        coord.world.borrow_mut().lat.init = 0;
        coord.init(Path::new("/w/m2.1/store.1")).unwrap();
        let child = coord.child();
        coord.clear();
        (coord, child)
    }

    /// Every field of a `go` and an `ack` line, every event kind, an `err` line and a store path with spaces read back
    /// as written; a broken line is refused.
    #[test]
    fn go_and_ack_lines_round_trip() {
        let g = Go {
            op: 17,
            start_ns: 123_456_789,
            cost_ns: 5_000_000,
            filler: 1_048_576,
            dir: PathBuf::from("D:/work dir/m2.42/store.3"),
        };
        assert_eq!(
            g.line(),
            "go 17 123456789 5000000 1048576 D:/work dir/m2.42/store.3"
        );
        assert_eq!(Go::parse(&g.line()), Ok(g));
        let a = Ack {
            op: 17,
            t_start: 10,
            t_ack: 99,
            sync_data: 1,
            sync_meta: 2,
            bytes_read: 4_096,
            phase1_ns: 33,
            events: vec![
                wg(11, 12),
                LockEvent::Grant {
                    byte: Byte::Flush,
                    req: 20,
                    grant: 25,
                },
                wr(30),
                LockEvent::Release {
                    byte: Byte::Flush,
                    t: 40,
                },
            ],
        };
        let line = Reply::Ack(a.clone()).line();
        assert_eq!(line, "ack 17 10 99 1 2 4096 33 W11:12 F20:25 w30 f40");
        assert_eq!(Reply::parse(&line), Ok(Reply::Ack(a)));
        let e = Reply::Err {
            op: 3,
            message: "open: no store".into(),
        };
        assert_eq!(Reply::parse(&e.line()), Ok(e));
        for bad in ["go 1 2 3", "go 1 2 3 4 ", "go 1 2 3 99999999999 d", "hi"] {
            assert!(Go::parse(bad).is_err(), "{bad}");
        }
        for bad in [
            "ack 1 2",
            "ack 1 2 3 4 5 6 7 X1:2",
            "ack 1 2 3 4 5 6 7 W1",
            "err x y",
            "hi",
        ] {
            assert!(Reply::parse(bad).is_err(), "{bad}");
        }
    }

    /// A `go` opens the store with its cost, as a new process does, commits its op with its filler, and the ack
    /// carries the counters' delta and the drained lock events.
    #[test]
    fn a_go_opens_with_its_cost_commits_and_acks_with_its_counts() {
        let (coord, child) = child_world();
        child.counters.borrow_mut().sync_data = 5;
        let Reply::Ack(a) = serve_one(&child, &go(7, 0, 3_000)) else {
            panic!("an ack expected");
        };
        let t = coord.trace();
        assert!(t.contains(&Ev::Open {
            id: 1,
            dir: PathBuf::from("/w/m2.1/store.1"),
            cost: 3_000
        }));
        assert!(t.contains(&Ev::Commit {
            id: 1,
            op: 7,
            filler: 450
        }));
        assert_eq!((a.op, a.sync_data, a.sync_meta), (7, 1, 0));
        assert_eq!(a.events.len(), 4, "{:?}", a.events);
        assert_eq!(a.append_hold(), Some(20_000 + 3_000));
        assert!(child.drain_events().is_empty(), "drained into the ack");
    }

    /// The child waits until T before it opens; `t_ack` is stamped before the close (whose 10^9 ns it excludes), and
    /// the reply follows the close.
    #[test]
    fn the_child_starts_at_the_start_time_and_acks_before_the_close() {
        let (coord, child) = child_world();
        coord.world.borrow_mut().lat.close = 1_000_000_000;
        let input = format!("{}\nquit\n", go(1, 50_000, 0).line());
        let mut out = Vec::new();
        assert_eq!(serve(&child, &mut input.as_bytes(), &mut out), 0);
        let reply = Reply::parse(String::from_utf8(out).unwrap().trim_end()).unwrap();
        let Reply::Ack(a) = reply else {
            panic!("{reply:?}")
        };
        assert_eq!(a.t_start, 50_000);
        assert_eq!(
            a.t_ack,
            50_000 + 1_000 + 20_000,
            "the close's 10^9 ns are not in it"
        );
        let t = coord.trace();
        let at = |e: &Ev| t.iter().position(|x| x == e).unwrap();
        assert!(
            at(&Ev::WaitUntil(50_000))
                < t.iter().position(|e| matches!(e, Ev::Open { .. })).unwrap()
        );
        assert_eq!(
            child.now(),
            50_000 + 1_000 + 20_000 + 1_000_000_000,
            "the close ran"
        );
    }

    /// A failing commit, or an acknowledgement sooner than its cost (a `Config` without its draw), is an `err` line.
    #[test]
    fn a_failure_or_an_unspent_cost_is_an_err_line() {
        let (coord, child) = child_world();
        coord.world.borrow_mut().fail_ops.insert(9);
        let r = serve_one(&child, &go(9, 0, 0));
        assert!(
            matches!(&r, Reply::Err { op: 9, message } if message.contains("scripted failure")),
            "{r:?}"
        );
        assert!(r.line().starts_with("err 9 "));
        assert!(
            coord.world.borrow().handles.is_empty(),
            "the handle is closed on failure"
        );
        coord.world.borrow_mut().ignore_cost = true;
        let r = serve_one(&child, &go(10, 0, 30_000));
        assert!(
            matches!(&r, Reply::Err { op: 10, message } if message.contains("below its in-lock cost")),
            "{r:?}"
        );
        coord.world.borrow_mut().ignore_cost = false;
        assert!(matches!(
            serve_one(&child, &go(11, 0, 30_000)),
            Reply::Ack(_)
        ));
    }

    /// The loop ends with 0 at `quit` and at the end of its input (a coordinator that went away), and answers a line
    /// it cannot read with an `err`.
    #[test]
    fn serve_ends_at_quit_or_end_of_input() {
        let (_coord, child) = child_world();
        let mut out = Vec::new();
        assert_eq!(serve(&child, &mut "".as_bytes(), &mut out), 0);
        assert!(out.is_empty());
        let input = format!("hello\n{}\n", go(2, 0, 0).line());
        assert_eq!(serve(&child, &mut input.as_bytes(), &mut out), 0);
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].starts_with("err 0 not a go line"));
        assert!(lines[1].starts_with("ack 2 "));
    }
}
