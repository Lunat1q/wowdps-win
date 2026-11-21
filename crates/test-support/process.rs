//! A test-owned process boundary: all threads end before its parent removes
//! scratch. The child borrows that root and defers local sandbox cleanup.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};

pub struct TestOutput {
    pub entered: bool,
    pub root: PathBuf,
    pub output: Output,
}

struct Reap<'a>(&'a mut Child);

impl Drop for Reap<'_> {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// The owner stays in the parent. Even a child test that panics or writes in
/// an exit handler cannot race this directory's removal.
pub fn run_test(test: &str, configure: impl FnOnce(&mut Command)) -> TestOutput {
    let dir = super::Dir::new_in(&super::short_parent());
    let root = dir.path().to_path_buf();
    for folder in ["rt", "config", "cache", "data", "state", "home"] {
        std::fs::create_dir(root.join(folder)).expect("test environment directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(root.join("rt"), std::fs::Permissions::from_mode(0o700))
            .expect("private test runtime");
    }
    let stdout = std::fs::File::create(root.join("stdout")).expect("child stdout");
    let stderr = std::fs::File::create(root.join("stderr")).expect("child stderr");
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", test, "--include-ignored", "--nocapture"])
        .env("WOWDPS_OWNED_TEST", test)
        .env("WOWDPS_OWNED_ROOT", &root)
        .env("TMPDIR", &root)
        .env("TMP", &root)
        .env("TEMP", &root)
        .env("XDG_RUNTIME_DIR", root.join("rt"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HOME", root.join("home"))
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().expect("stdout handle"))
        .stderr(stderr.try_clone().expect("stderr handle"));
    configure(&mut command);
    let mut child = command.spawn().expect("spawn owned test process");
    let child = Reap(&mut child);
    let status = child.0.wait().expect("wait for owned test process");
    let output = Output {
        status,
        stdout: std::fs::read(root.join("stdout")).expect("read child stdout"),
        stderr: std::fs::read(root.join("stderr")).expect("read child stderr"),
    };
    // Reap and the output handles drop before dir, on success and unwinding.
    TestOutput {
        entered: root.join(".test-entered").is_file(),
        root,
        output,
    }
}

pub fn in_child() -> bool {
    std::env::var_os("WOWDPS_OWNED_TEST").is_some()
        && std::env::var_os("WOWDPS_OWNED_ROOT").is_some()
}

pub(super) fn borrowed_root() -> Option<PathBuf> {
    in_child()
        .then(|| std::env::var_os("WOWDPS_OWNED_ROOT").map(PathBuf::from))
        .flatten()
}

/// Put this first in a filesystem-writer test, before setup can spawn a thread.
/// A child executes the unchanged body; the parent waits, cleans up, and then
/// propagates its result. Exact selection also covers explicitly ignored tests.
pub fn isolate(test: &str) -> bool {
    if in_child() && std::env::var("WOWDPS_OWNED_TEST").as_deref() == Ok(test) {
        let root = borrowed_root().expect("parent-owned root");
        std::fs::write(root.join(".test-entered"), test).expect("acknowledge selected test");
        return false;
    }
    let result = run_test(test, |_| {});
    assert!(
        result.entered && result.output.status.success(),
        "isolated {test}: {:?}\nstdout:\n{}\nstderr:\n{}",
        result.output.status,
        String::from_utf8_lossy(&result.output.stdout),
        String::from_utf8_lossy(&result.output.stderr)
    );
    true
}
