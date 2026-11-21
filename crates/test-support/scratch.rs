//! Scratch storage for tests. Include this module only in test builds (or
//! test-support features). No Cargo dependency or shipped behavior is added.
//!
//! Existing per-test guards can keep removing their children early. The
//! process root also covers setup panics and paths held in process-wide state.
//! Normal exits, including libtest failures, run the C runtime's exit hook;
//! aborts and signals that kill the process cannot run cleanup.

#![allow(dead_code, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Once, OnceLock};

static ROOT: OnceLock<Dir> = OnceLock::new();
static SHORT_ROOT: OnceLock<Dir> = OnceLock::new();

unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> std::ffi::c_int;
}

/// One owned directory, removed on drop, including during panic unwinding.
/// Exclusive creation never takes over a directory left by another process.
pub struct Dir(PathBuf);

impl Dir {
    pub fn new_in(parent: &Path) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!("wd-{}-{n}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create scratch directory {path:?}: {e}"),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    fn remove(&self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            use std::io::Write as _;
            let _ = writeln!(
                std::io::stderr(),
                "remove scratch directory {:?}: {e}",
                self.0
            );
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        self.remove();
    }
}

extern "C" fn cleanup() {
    for dir in [ROOT.get(), SHORT_ROOT.get()].into_iter().flatten() {
        dir.remove();
    }
}

fn process_dir(slot: &'static OnceLock<Dir>, base: PathBuf) -> PathBuf {
    slot.get_or_init(|| {
        let dir = Dir::new_in(&base);
        static REGISTER: Once = Once::new();
        REGISTER.call_once(|| {
            // SAFETY: the callback has C ABI, never unwinds, and touches only
            // process-lifetime statics. atexit is provided by the C runtime on
            // Linux and macOS, the platforms these tests build for.
            assert_eq!(unsafe { atexit(cleanup) }, 0, "register scratch cleanup");
        });
        dir
    })
    .path()
    .to_path_buf()
}

/// Parent for test scratch paths; removed at normal process exit.
pub fn root() -> PathBuf {
    process_dir(&ROOT, std::env::temp_dir())
}

/// Preserve the fake compositor's short Unix socket paths even when the
/// system's temporary directory (notably on macOS) has a long name.
pub fn short_root() -> PathBuf {
    let base = std::env::temp_dir();
    if base.as_os_str().len() <= 40 {
        root()
    } else {
        process_dir(&SHORT_ROOT, PathBuf::from("/tmp"))
    }
}
