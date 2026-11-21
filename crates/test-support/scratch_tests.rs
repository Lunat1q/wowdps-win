//! Tests for directory ownership and the normal process-exit fallback.

use crate::test_scratch::{self, Dir};

#[test]
fn dropping_a_guard_removes_its_directory_and_contents() {
    let dir = Dir::new_in(&test_scratch::root());
    let path = dir.path().to_path_buf();
    std::fs::create_dir(path.join("nested")).unwrap();
    std::fs::write(path.join("nested/file"), b"scratch").unwrap();
    drop(dir);
    assert!(!path.exists());
}

#[test]
fn a_panicking_test_body_removes_its_directory() {
    let mut path = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = Dir::new_in(&test_scratch::root());
        path = Some(dir.path().to_path_buf());
        std::fs::write(dir.path().join("file"), b"scratch").unwrap();
        panic!("test body failed");
    }));
    assert!(result.is_err());
    assert!(!path.unwrap().exists());
}

#[test]
fn creating_a_guard_preserves_an_existing_directory() {
    let parent = Dir::new_in(&test_scratch::root());
    let first = Dir::new_in(parent.path());
    std::fs::write(first.path().join("keep"), b"owned").unwrap();
    let second = Dir::new_in(parent.path());
    assert_ne!(first.path(), second.path());
    assert_eq!(std::fs::read(first.path().join("keep")).unwrap(), b"owned");
}

#[test]
fn process_roots_are_removed_after_successful_and_failed_test_runs() {
    for fail in [false, true] {
        let parent = Dir::new_in(&test_scratch::root());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "scratch_tests::child_process", "--nocapture"])
            .env("WOWDPS_SCRATCH_CHILD", if fail { "fail" } else { "pass" })
            .env("TMPDIR", parent.path())
            .output()
            .unwrap();
        assert_eq!(output.status.success(), !fail, "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let path = stdout
            .lines()
            .find_map(|line| line.strip_prefix("scratch-root="))
            .expect("the child exercised the process root");
        assert!(std::path::Path::new(path).starts_with(parent.path()));
        assert!(!std::path::Path::new(path).exists(), "{path}");
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }
}

#[test]
fn long_temp_paths_keep_socket_roots_short_and_clean() {
    let parent = Dir::new_in(&test_scratch::root());
    let long = parent.path().join("long-temporary-directory-".repeat(3));
    std::fs::create_dir(&long).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "scratch_tests::child_process", "--nocapture"])
        .env("WOWDPS_SCRATCH_CHILD", "short")
        .env("TMPDIR", &long)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout
        .lines()
        .find_map(|line| line.strip_prefix("scratch-root="))
        .expect("the child exercised the short root");
    let socket = stdout
        .lines()
        .find_map(|line| line.strip_prefix("scratch-socket="))
        .expect("the child bound the complete socket suffix");
    let capacity = if cfg!(target_os = "linux") { 108 } else { 104 };
    assert!(socket.len() < capacity, "full socket path: {socket}");
    assert!(!std::path::Path::new(path).exists(), "{path}");
    assert_eq!(std::fs::read_dir(&long).unwrap().count(), 0);
}

#[test]
fn child_process() {
    let Ok(mode) = std::env::var("WOWDPS_SCRATCH_CHILD") else {
        return;
    };
    let root = if mode == "short" {
        test_scratch::short_root()
    } else {
        test_scratch::root()
    };
    std::fs::write(root.join("file"), b"scratch").unwrap();
    #[cfg(unix)]
    let _socket = if mode == "short" {
        let dir = root.join(format!("wowdps-lake-parity-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("test.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        println!("scratch-socket={}", socket.display());
        Some(listener)
    } else {
        None
    };
    println!("scratch-root={}", root.display());
    assert_ne!(mode, "fail", "test body failed");
}
