//! The artifact hooks: fallback A of docs/m0/tools.md §4.4, failure 2.
//!
//! libfuzzer-sys turns a panic into `std::process::abort()`, which on Windows is `__fastfail` (exit `0xC0000409`): no
//! in-process handler runs, so libFuzzer never writes `crash-*` and the crashing input is lost. An allocation failure
//! (`handle_alloc_error`, for example a size read from the input) aborts the same way, without any panic hook, and
//! libFuzzer's `-malloc_limit_mb` needs the sanitizer's malloc hooks, which a sanitizer-off build lacks.
//!
//! [`record`] keeps a copy of the input about to run in one reused buffer (one copy per execution, no allocation once
//! the buffer has grown to the largest input). Its first call installs two hooks:
//! - a panic hook in front of libfuzzer-sys's: it writes the buffer where libFuzzer would have written the crash
//!   (`crash-<sha1>`), prints libFuzzer's own line for it, and then lets libfuzzer-sys's hook print the panic and
//!   abort;
//! - an allocation-error hook (`std::alloc::set_alloc_error_hook`, the nightly feature `alloc_error_hook` of the
//!   pinned toolchain): it writes the buffer as `oom-<sha1>`, libFuzzer's name for an out-of-memory input, prints the
//!   same line, and then calls the previous hook, which prints "memory allocation of N bytes failed"; the standard
//!   library aborts after it.
//!
//! Where the input goes, as libFuzzer decides it (`FuzzerLoop.cpp`, `DumpCurrentUnit`; `FuzzerFlags.def`):
//! `-exact_artifact_path=<path>` verbatim, else `<-artifact_prefix=><kind>-<SHA-1 of the input, lower-case hex>`,
//! the prefix being concatenated as text (cargo-fuzz passes `fuzz/artifacts/<target>/`). The last occurrence of a
//! flag wins, as in libFuzzer's parser. With the artifact in place, `cargo fuzz run <target> <artifact>` reproduces
//! the crash and `cargo fuzz tmin` minimises it (its child processes get `-exact_artifact_path=`).

use sha1::{Digest, Sha1};
use std::alloc::Layout;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, OnceLock, PoisonError, TryLockError};

/// The input of the execution in progress.
static CURRENT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static INSTALL: Once = Once::new();
/// Where the allocation-error hook writes: the destination of the latest [`install`].
static OOM_DEST: Mutex<Option<Dest>> = Mutex::new(None);
/// The allocation-error hook in place before ours, which ours calls last.
static OOM_PREVIOUS: OnceLock<fn(Layout)> = OnceLock::new();
/// Set while the allocation-error hook runs: an allocation failing inside it (a process truly out of memory) returns
/// at once instead of recursing, and the standard library aborts.
static IN_OOM_HOOK: AtomicBool = AtomicBool::new(false);

/// Records the input a fuzz target is about to run. Every target calls it first, with the whole input
/// (`fuzz_target!(|data: &[u8]| { moirai_fuzz::record(data); … })`; the gate's `fuzz` step enforces it); its first
/// call installs the hooks.
pub fn record(data: &[u8]) {
    INSTALL.call_once(|| install(Dest::from_args(std::env::args_os())));
    store(data);
}

/// Copies `data` into the reused buffer.
fn store(data: &[u8]) {
    let mut c = CURRENT.lock().unwrap_or_else(PoisonError::into_inner);
    c.clear();
    c.extend_from_slice(data);
}

/// Where a failing input is written, from libFuzzer's command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dest {
    /// `-exact_artifact_path=`: the whole path.
    pub exact: Option<String>,
    /// `-artifact_prefix=`: text put in front of `<kind>-<sha1>`; empty means the working directory.
    pub prefix: String,
}

impl Dest {
    /// Reads `-exact_artifact_path=` and `-artifact_prefix=` from libFuzzer's arguments; the last of each wins.
    pub fn from_args<I, S>(args: I) -> Dest
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut d = Dest::default();
        for a in args {
            let a = a.as_ref().to_string_lossy();
            if let Some(p) = a.strip_prefix("-exact_artifact_path=") {
                d.exact = Some(p.to_owned());
            } else if let Some(p) = a.strip_prefix("-artifact_prefix=") {
                p.clone_into(&mut d.prefix);
            }
        }
        d
    }

    /// The file for a failing `input` of `kind` (`crash` or `oom`, libFuzzer's names).
    pub fn path_for(&self, kind: &str, input: &[u8]) -> String {
        match &self.exact {
            Some(p) => p.clone(),
            None => format!("{}{kind}-{}", self.prefix, sha1_hex(input)),
        }
    }
}

/// The lower-case hex SHA-1 of `input`, libFuzzer's artifact name (`Hash` in `FuzzerSHA1.cpp`).
pub fn sha1_hex(input: &[u8]) -> String {
    let mut s = String::with_capacity(40);
    for b in Sha1::digest(input) {
        s.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        s.push(char::from(b"0123456789abcdef"[usize::from(b & 15)]));
    }
    s
}

/// Installs the panic hook in front of the current one (libfuzzer-sys's, which prints the panic and aborts), and
/// points the allocation-error hook at `dest`, installing it once per process.
fn install(dest: Dest) {
    *OOM_DEST.lock().unwrap_or_else(PoisonError::into_inner) = Some(dest.clone());
    let next = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_current(&dest, "crash");
        next(info);
    }));
    OOM_PREVIOUS.get_or_init(|| {
        let previous = std::alloc::take_alloc_error_hook();
        std::alloc::set_alloc_error_hook(on_alloc_error);
        previous
    });
}

/// The allocation-error hook: writes the recorded input as `oom-<sha1>`, then runs the previous hook.
fn on_alloc_error(layout: Layout) {
    if IN_OOM_HOOK.swap(true, Ordering::SeqCst) {
        return;
    }
    let dest = match OOM_DEST.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        // `install` holds it: the failing allocation is its own.
        Err(TryLockError::WouldBlock) => None,
    };
    if let Some(d) = dest.as_deref().and_then(Option::as_ref) {
        write_current(d, "oom");
    }
    drop(dest);
    if let Some(previous) = OOM_PREVIOUS.get() {
        previous(layout);
    }
    IN_OOM_HOOK.store(false, Ordering::SeqCst);
}

/// Writes the recorded input as a `kind` artifact. Never panics: a panic inside a panic hook aborts before
/// libfuzzer-sys's hook prints, and one inside the allocation-error hook aborts at once.
fn write_current(dest: &Dest, kind: &str) {
    let guard = match CURRENT.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        // Another thread is recording an input (the failing thread is not the fuzzing thread), or this one failed
        // to allocate inside `store`.
        Err(TryLockError::WouldBlock) => return,
    };
    let path = dest.path_for(kind, &guard);
    let mut err = std::io::stderr().lock();
    let _ = match std::fs::write(&path, &*guard) {
        Ok(()) => writeln!(
            err,
            "artifact_prefix='{}'; Test unit written to {path} (moirai-fuzz {kind} hook, {} bytes)",
            dest.prefix,
            guard.len()
        ),
        Err(e) => writeln!(
            err,
            "moirai-fuzz {kind} hook: could not write the failing input to {path}: {e}"
        ),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A scratch directory, removed when dropped, so a failed assertion leaves nothing behind.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> TestDir {
            let d = std::env::temp_dir().join(format!("moirai-fuzz-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            TestDir(d)
        }

        fn prefix(&self) -> String {
            format!("{}/", self.0.display())
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The planted input: the 0x05 after the magic is a length byte, written as an escape so the 13 bytes are
    /// visible.
    const PLANTED: &[u8] = b"MOI!\x05 planted";

    #[test]
    fn destinations_follow_libfuzzer() {
        let d = Dest::from_args([
            "target.exe",
            "-artifact_prefix=a/",
            "-rss_limit_mb=256",
            "-artifact_prefix=fuzz/artifacts/t/",
        ]);
        assert_eq!(d.exact, None);
        assert_eq!(
            d.path_for("crash", b"abc"),
            "fuzz/artifacts/t/crash-a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            d.path_for("oom", b"abc"),
            "fuzz/artifacts/t/oom-a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        let e = Dest::from_args(["t", "-exact_artifact_path=C:/x/min", "-artifact_prefix=p/"]);
        assert_eq!(e.path_for("crash", b"anything"), "C:/x/min");
        assert_eq!(e.path_for("oom", b"anything"), "C:/x/min");
        assert_eq!(
            Dest::from_args(["t"]).path_for("crash", b""),
            "crash-da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(PLANTED.len(), 13);
    }

    /// One test for everything that touches the process-wide panic hook and buffer, so no other test races it.
    /// `record` itself is not called: its hook would write into the working directory.
    #[test]
    fn the_hook_writes_the_recorded_input_and_chains() {
        let dir = TestDir::new("hook");
        let prefix = dir.prefix();
        // The buffer is reused: a large input, then a small one, keeps the capacity.
        store(&[1; 300]);
        store(PLANTED);
        {
            let c = CURRENT.lock().unwrap();
            assert_eq!(&*c, PLANTED);
            assert!(c.capacity() >= 300);
        }
        // The test harness's hook stands in for libfuzzer-sys's: it must still run after ours.
        let chained = std::sync::Arc::new(AtomicBool::new(false));
        let seen = chained.clone();
        let harness = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            seen.store(true, Ordering::SeqCst);
            harness(info);
        }));
        install(Dest {
            exact: None,
            prefix: prefix.clone(),
        });
        let r = std::panic::catch_unwind(|| panic!("planted bug (expected by this test)"));
        let _ = std::panic::take_hook();
        assert!(r.is_err());
        assert!(chained.load(Ordering::SeqCst));
        let written = std::fs::read(format!("{prefix}crash-{}", sha1_hex(PLANTED))).unwrap();
        assert_eq!(written, PLANTED);
        // -exact_artifact_path= wins over the prefix.
        let exact = dir.0.join("exact");
        install(Dest {
            exact: Some(exact.display().to_string()),
            prefix,
        });
        let r = std::panic::catch_unwind(|| panic!("planted bug (expected by this test)"));
        let _ = std::panic::take_hook();
        assert!(r.is_err());
        assert_eq!(std::fs::read(&exact).unwrap(), PLANTED);
    }

    /// The child side of [`an_allocation_failure_writes_an_oom_artifact`]: with `MOIRAI_FUZZ_OOM_DIR` set, it records
    /// the planted input and requests 2^62 bytes, which no allocator grants, so the process aborts in the hook.
    #[test]
    #[ignore = "run in a child process by an_allocation_failure_writes_an_oom_artifact"]
    fn oom_child() {
        let Some(dir) = std::env::var_os("MOIRAI_FUZZ_OOM_DIR") else {
            return;
        };
        install(Dest {
            exact: None,
            prefix: format!("{}/", Path::new(&dir).display()),
        });
        store(PLANTED);
        let v: Vec<u8> = Vec::with_capacity(std::hint::black_box(1usize << 62));
        std::hint::black_box(&v);
        unreachable!("an allocation of 2^62 bytes succeeded");
    }

    /// The allocation-error path end to end: the child aborts, and the planted input is on disk as `oom-<sha1>`.
    #[test]
    fn an_allocation_failure_writes_an_oom_artifact() {
        let dir = TestDir::new("oom");
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "artifact::tests::oom_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("MOIRAI_FUZZ_OOM_DIR", &dir.0)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "the child did not abort: {stderr}");
        let artifact = dir.0.join(format!("oom-{}", sha1_hex(PLANTED)));
        assert_eq!(
            std::fs::read(&artifact).ok().as_deref(),
            Some(PLANTED),
            "{stderr}"
        );
        assert!(stderr.contains("Test unit written to"), "{stderr}");
        assert!(
            stderr.contains("memory allocation of 4611686018427387904 bytes failed"),
            "the previous hook ran after ours: {stderr}"
        );
    }
}
