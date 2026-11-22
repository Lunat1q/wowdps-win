//! The `wowdps` dispatcher end to end: the built binary run as a subprocess
//! — usage, the git-style external dispatch, `stop`/`status` against nothing,
//! and a real daemon lifecycle on the fixture — every run sandboxed under a
//! temp dir (`XDG_RUNTIME_DIR` names the socket, so the user's real daemon is
//! never touched; `HOME`/`XDG_*` keep config, cache and state out of the
//! real home; `PATH` holds only what a test put there).

// In tests a panic IS the failure mechanism (clippy.toml's intent). The
// helper fns below sit outside #[test] items, which the allow-*-in-tests
// exemptions no longer cover, so this integration-test crate says it
// explicitly.
// The sandbox runs shell scripts as the external commands and the GUI.
#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use wowdps_proto::sock;

use wowdps_proto::{ClientMsg, DaemonMsg, PROTO_VERSION, wire};

const BIN: &str = env!("CARGO_BIN_EXE_wowdps");
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const DEADLINE: Duration = Duration::from_secs(15);

/// One isolated home for one test: runtime dir (socket + lock), state dir
/// (daemon.log), cache, config, a `bin` dir that is the whole `$PATH`.
static SANDBOX_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Sandbox {
    root: PathBuf,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        // A subreaper owns orphaned descendants process-wide. Serialize all
        // subprocess sandboxes so teardown cannot reap another test's child.
        let serial = SANDBOX_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        #[cfg(target_os = "linux")]
        child_processes::enable();
        // Short: the socket path must fit a sockaddr_un.
        let root =
            crate::test_scratch::short_root().join(format!("wdt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["rt", "state", "cache", "config/wowdps", "home", "bin"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        // The daemon must never spawn an overlay from a test, whatever is
        // running on the machine.
        std::fs::write(
            root.join("config/wowdps/config.toml"),
            "auto_overlay = false\ngame_process = \"wowdps-test-no-such-process\"\n",
        )
        .unwrap();
        Sandbox {
            root,
            _serial: serial,
        }
    }

    fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    fn socket(&self) -> PathBuf {
        self.root
            .join("rt/wowdps")
            .join(format!("wowdps-v{PROTO_VERSION}.sock"))
    }

    fn daemon_log(&self) -> String {
        std::fs::read_to_string(self.root.join("state/wowdps/daemon.log")).unwrap_or_default()
    }

    /// The daemon writes its last log line after it has already unlinked
    /// the socket, so a detached daemon's "clean exit" trails the socket's
    /// disappearance by a moment.
    fn wait_log_contains(&self, needle: &str) -> String {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let log = self.daemon_log();
            if log.contains(needle) {
                return log;
            }
            assert!(
                Instant::now() < deadline,
                "daemon.log never gained {needle:?}: {log:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        // Under `cargo llvm-cov` the children are instrumented too: keep
        // the profile sink so their coverage counts.
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            cmd.env("LLVM_PROFILE_FILE", profile);
        }
        cmd.env("PATH", self.bin_dir())
            .env("HOME", self.root.join("home"))
            .env("XDG_RUNTIME_DIR", self.root.join("rt"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .stdin(Stdio::null());
        cmd
    }

    fn wowdps(&self, args: &[&str]) -> Command {
        let mut cmd = self.command(BIN);
        cmd.args(args);
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.wowdps(args).output().expect("run wowdps")
    }

    /// Drop an executable script into the sandbox's `$PATH`.
    fn script(&self, name: &str, body: &str, mode: u32) -> PathBuf {
        let path = self.bin_dir().join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn wait_socket(&self, present: bool) {
        let deadline = Instant::now() + DEADLINE;
        while self.socket().exists() != present {
            assert!(
                Instant::now() < deadline,
                "socket {:?} never became present={present}",
                self.socket()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        child_processes::finish(&self.socket());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn canonical_fixture() -> String {
    std::fs::canonicalize(FIXTURE)
        .unwrap()
        .display()
        .to_string()
}

fn addon_account(sb: &Sandbox, account: &str, data: &str) {
    let product = sb.root.join("game/_retail_");
    std::fs::create_dir_all(sb.root.join("game/Data/data")).unwrap();
    std::fs::create_dir_all(product.join("Logs")).unwrap();
    std::fs::write(sb.root.join("game/.build.info"), "").unwrap();
    let dir = product
        .join("WTF/Account")
        .join(account)
        .join("SavedVariables");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wowdps.lua");
    std::fs::write(&path, data).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
        .unwrap();
    std::fs::write(
        sb.root.join("config/wowdps/config.toml"),
        format!(
            "logs_dir = {:?}\nauto_overlay = false\n",
            product.join("Logs")
        ),
    )
    .unwrap();
}

#[test]
fn addon_status_numbers_accounts_in_sorted_order_without_folder_names() {
    let sb = Sandbox::new("addon-sorted");
    // Creation order differs from folder order; counts identify each row.
    addon_account(&sb, "ZED", "WOWDPS_DATA = {}");
    addon_account(
        &sb,
        "ALPHA",
        "WOWDPS_DATA = { [\"players\"] = { [\"Player-1-A\"] = { [\"name\"] = \"Ada\" } } }",
    );
    let out = sb.run(&["addon"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with("saved:"))
            .collect::<Vec<_>>(),
        [
            "saved:   account 1: 1 players, 0 creatures, written 1970-01-01",
            "saved:   account 2: 0 players, 0 creatures, written 1970-01-01",
        ]
    );
    assert!(!text.contains("ALPHA") && !text.contains("ZED"), "{text}");
    assert!(stderr(&out).is_empty());
    assert!(!sb.socket().exists());
}

#[test]
fn addon_status_numbers_a_single_account_as_account_one() {
    let sb = Sandbox::new("addon-single");
    addon_account(&sb, "LANTERN", "WOWDPS_DATA = {}");
    let out = sb.run(&["addon", "status"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with("saved:"))
            .collect::<Vec<_>>(),
        ["saved:   account 1: 0 players, 0 creatures, written 1970-01-01"]
    );
    assert!(!text.contains("LANTERN"), "{text}");
    assert!(stderr(&out).is_empty());
    assert!(!sb.socket().exists());
}

#[test]
fn help_prints_the_usage_and_exits_clean() {
    let sb = Sandbox::new("help");
    for args in [&["--help"][..], &["-h"], &["help"]] {
        let out = sb.run(args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        let text = stdout(&out);
        assert!(text.contains("Usage:"), "{args:?}: {text}");
        assert!(text.contains("wowdps stop"), "{args:?}: {text}");
        assert!(stderr(&out).is_empty(), "{args:?} wrote to stderr");
    }
}

#[test]
fn a_bad_argument_prints_the_error_and_the_usage_with_status_2() {
    let sb = Sandbox::new("badarg");
    let out = sb.run(&["--nonsense"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("unknown argument \"--nonsense\""), "{err}");
    assert!(err.contains("Usage:"), "{err}");
    assert!(stdout(&out).is_empty());

    // An edition the TUI does not take is refused, naming those it does,
    // before any daemon is reached.
    let out = sb.run(&["--edition", "tbc"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(
        err.contains("unknown edition \"tbc\": --edition takes retail or forever"),
        "{err}"
    );
    assert!(err.contains("Usage:"), "{err}");
    assert!(!sb.socket().exists(), "a refused argument spawns no daemon");

    // The retired flag-mode spelling points at its subcommand.
    let out = sb.run(&["--stop"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("--stop is now a subcommand: wowdps stop"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn stop_and_status_without_a_daemon_say_so() {
    let sb = Sandbox::new("nodaemon");
    let out = sb.run(&["stop"]);
    assert!(out.status.success(), "stop is idempotent");
    assert!(
        stdout(&out).contains("no daemon running"),
        "{}",
        stdout(&out)
    );

    let out = sb.run(&["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("no daemon running"),
        "{}",
        stdout(&out)
    );
    assert!(!sb.socket().exists(), "status/stop never create a socket");
}

/// `name` resolved on the test runner's own `$PATH` — the sandbox's is empty
/// on purpose, so a script or launch that needs a host tool names it whole.
fn host_tool(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(name))
                .collect::<Vec<_>>()
        })
        .and_then(|cands| cands.into_iter().find(|p| p.is_file()))
}

#[test]
fn an_unknown_word_runs_the_external_binary_with_its_arguments() {
    let sb = Sandbox::new("external");
    // The script names echo whole: a /bin/sh like yash runs `echo` only when
    // one is on $PATH, and the sandbox's $PATH holds nothing but the script.
    let echo = host_tool("echo").expect("echo on the test runner's PATH");
    sb.script(
        "wowdps-gen-foo",
        &format!("#!/bin/sh\n{} \"gen-foo:$*\"\nexit 7\n", echo.display()),
        0o755,
    );
    let out = sb.run(&["gen-foo", "a", "--file", "b c"]);
    // exec() replaced the process: the script's own exit status comes back.
    assert_eq!(out.status.code(), Some(7), "{}", stderr(&out));
    assert_eq!(stdout(&out), "gen-foo:a --file b c\n");
}

#[test]
fn a_missing_external_binary_is_reported_with_the_usage() {
    let sb = Sandbox::new("noexternal");
    let out = sb.run(&["no-such-cmd-zz", "x"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(
        err.contains("'no-such-cmd-zz' is not a wowdps command (no wowdps-no-such-cmd-zz found)"),
        "{err}"
    );
    assert!(err.contains("Usage:"), "{err}");
}

#[test]
fn an_unrunnable_external_binary_reports_the_exec_error() {
    let sb = Sandbox::new("noexec");
    // Present on $PATH but not executable: exec fails with something other
    // than NotFound, which is a different message.
    sb.script("wowdps-gen-bar", "#!/bin/sh\nexit 0\n", 0o644);
    let out = sb.run(&["gen-bar"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("running wowdps-gen-bar failed"), "{err}");
    assert!(!err.contains("Usage:"), "not a usage error: {err}");
}

/// A socket that accepts and hangs up: the daemon socket exists but the
/// handshake cannot complete.
#[test]
fn status_reports_a_failed_handshake() {
    let sb = Sandbox::new("badhandshake");
    std::fs::create_dir_all(sb.socket().parent().unwrap()).unwrap();
    let listener = sock::bind(sb.socket()).unwrap();
    let server = std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            drop(stream);
        }
    });
    let out = sb.run(&["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("daemon socket exists but the handshake failed"),
        "{}",
        stdout(&out)
    );
    server.join().unwrap();
}

/// A socket that completes the handshake and then goes silent: status is
/// asked and never answered.
#[test]
fn status_reports_a_daemon_that_never_answers() {
    let sb = Sandbox::new("noanswer");
    std::fs::create_dir_all(sb.socket().parent().unwrap()).unwrap();
    let listener = sock::bind(sb.socket()).unwrap();
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Read the Hello before answering, and the GetStatus before
            // hanging up, so the client never hits a closed pipe mid-write.
            let (tag, body) = wire::read_frame(&mut stream).unwrap();
            assert!(matches!(
                ClientMsg::decode(tag, &body),
                Ok(ClientMsg::Hello { .. })
            ));
            let ack = DaemonMsg::HelloAck {
                proto: PROTO_VERSION,
                version: "fake".to_string(),
            };
            stream.write_all(&ack.encode()).unwrap();
            let (tag, body) = wire::read_frame(&mut stream).unwrap();
            assert!(matches!(
                ClientMsg::decode(tag, &body),
                Ok(ClientMsg::GetStatus { .. })
            ));
            // Hang up without answering: the client's reader sees EOF and
            // gives up well before its 5 s deadline.
            drop(stream);
        }
    });
    let out = sb.run(&["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("daemon did not answer"),
        "{}",
        stdout(&out)
    );
    server.join().unwrap();
}

#[test]
fn a_daemon_with_nothing_to_tail_fails_setup_and_logs_it() {
    let sb = Sandbox::new("nosource");
    // No --file/--logs, config has no logs_dir, $HOME holds no Steam
    // install: the daemon says so instead of tailing a made-up path.
    let out = sb.run(&["daemon"]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("daemon setup failed"), "{err}");
    assert!(err.contains("no logs_dir configured"), "{err}");
    let log = sb.daemon_log();
    assert!(log.contains("setup failed"), "daemon.log: {log:?}");
    assert!(!sb.socket().exists());
}

/// The foreground daemon on the fixture, driven by the other subcommands:
/// status against it, a second daemon refusing to double up, the gui
/// launcher's source-conflict check, and a clean stop.
#[test]
fn the_foreground_daemon_serves_status_refuses_a_twin_and_stops_clean() {
    let sb = Sandbox::new("daemon");
    let mut daemon = sb
        .wowdps(&["daemon", "--linger", "--file", FIXTURE])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn daemon");
    let daemon_guard = ForegroundDaemon {
        child: &mut daemon,
        socket: sb.socket(),
    };
    sb.wait_socket(true);

    let out = sb.run(&["status"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("wowdps daemon: running"), "{text}");
    assert!(
        text.contains(&format!("source:  file:{}", canonical_fixture())),
        "{text}"
    );
    assert!(text.contains("clients: 1"), "{text}");
    assert!(text.contains("game:    not running"), "{text}");
    assert!(text.contains("linger:  yes"), "{text}");
    assert!(text.contains("overlay: Absent"), "{text}");

    // A second foreground daemon finds the lock taken: not an error.
    let twin = sb.run(&["daemon", "--file", FIXTURE]);
    assert!(twin.status.success(), "{}", stderr(&twin));
    assert!(
        stderr(&twin).contains("a daemon is already running"),
        "{}",
        stderr(&twin)
    );

    // The gui launcher checks source agreement before launching anything.
    let other = sb.root.join("home/other.txt");
    std::fs::write(&other, "").unwrap();
    let out = sb.run(&["gui", "--file", other.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(
        err.contains("a daemon is already running against file:"),
        "{err}"
    );
    assert!(err.contains("run `wowdps stop` first"), "{err}");
    // A directory source disagrees just the same.
    let out = sb.run(&["gui", "--logs", sb.root.join("home").to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("not logs:"), "{}", stderr(&out));

    // Agreeing on the source, it launches wowdps-gui — which the sandbox's
    // $PATH does not hold. Only when no sibling build of the gui exists
    // beside the binary (that would be preferred, and would open a window).
    let sibling_gui = Path::new(BIN).parent().unwrap().join("wowdps-gui");
    if !sibling_gui.exists() {
        let out = sb.run(&["gui", "--file", FIXTURE]);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).contains("launching wowdps-gui failed"),
            "{}",
            stderr(&out)
        );
    }

    let out = sb.run(&["stop"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("daemon asked to stop"),
        "{}",
        stdout(&out)
    );
    let status = daemon_guard.child.wait().expect("daemon exits");
    assert!(status.success(), "daemon exit: {status:?}");
    sb.wait_socket(false);
    let log = sb.daemon_log();
    assert!(
        log.contains(&format!("starting on file:{}", canonical_fixture())),
        "daemon.log: {log:?}"
    );
    assert!(log.contains("clean exit"), "daemon.log: {log:?}");
}

/// The TUI client with no daemon running spawns one itself, then fails to
/// take the terminal (there is none: `setsid` detaches the controlling tty
/// and stdin is /dev/null) — the error path every headless launch takes.
/// The daemon it spawned is real, idle-exits on its own, and is stopped here
/// explicitly anyway.
#[cfg(target_os = "linux")]
#[test]
fn the_tui_client_spawns_the_daemon_then_reports_a_missing_terminal() {
    let sb = Sandbox::new("tui");
    // The sandbox's $PATH is empty on purpose: resolve setsid on ours.
    let Some(setsid) = host_tool("setsid") else {
        eprintln!("setsid not available: skipping");
        return;
    };
    let out = sb
        .command(setsid.to_str().unwrap())
        .args(["--wait", BIN, "--file", FIXTURE])
        .output()
        .expect("run under setsid");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.starts_with("wowdps: "), "{err}");
    assert!(
        !err.contains("starting the daemon failed"),
        "the daemon spawn itself must succeed: {err}"
    );
    assert!(sb.socket().exists(), "the client left its daemon running");

    // The self-spawned daemon is a real one: status answers, stop ends it.
    let status = sb.run(&["status"]);
    assert!(status.status.success(), "{}", stdout(&status));
    assert!(
        stdout(&status).contains("linger:  no"),
        "{}",
        stdout(&status)
    );
    let out = sb.run(&["stop"]);
    assert!(out.status.success());
    sb.wait_socket(false);
    sb.wait_log_contains("clean exit");
}

#[path = "../../test-support/scratch.rs"]
mod test_scratch;

#[cfg(target_os = "linux")]
mod child_processes {
    use std::collections::BTreeSet;
    unsafe extern "C" {
        fn prctl(option: i32, ...) -> i32;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
    }
    pub fn enable() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            // SAFETY: PR_SET_CHILD_SUBREAPER changes only this test process.
            assert_eq!(unsafe { prctl(36, 1usize, 0usize, 0usize, 0usize) }, 0);
        });
    }
    pub fn children() -> BTreeSet<i32> {
        std::fs::read_dir("/proc/self/task")
            .unwrap()
            .flatten()
            .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
            .flat_map(|s| {
                s.split_whitespace()
                    .filter_map(|p| p.parse().ok())
                    .collect::<Vec<i32>>()
            })
            .collect()
    }
    pub fn finish(socket: &std::path::Path) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            for pid in children() {
                let _ = reaped(pid);
            }
            if children().is_empty() {
                return;
            }
            super::shutdown(socket);
            if std::time::Instant::now() >= deadline {
                for pid in children() {
                    if !reaped(pid) {
                        kill_and_wait(pid);
                    }
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
    pub fn reaped(pid: i32) -> bool {
        let mut status = 0;
        // SAFETY: pid was obtained from our own Child or adopted children.
        let result = unsafe { waitpid(pid, &mut status, 1) };
        result == pid
            || (result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(10))
    }
    pub fn kill_and_wait(pid: i32) {
        // SAFETY: this still-unreaped process is our own child. Its pid cannot
        // be reused until waitpid reaps it; sandbox spawning is serialized.
        unsafe {
            kill(pid, 9);
        }
        let mut status = 0;
        while unsafe { waitpid(pid, &mut status, 0) } == -1 {
            if std::io::Error::last_os_error().raw_os_error() != Some(4) {
                break;
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn panic_after_daemon_spawn(detached: bool) {
    let mut pid = 0;
    let mut root = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let sb = Sandbox::new(if detached { "panic-tui" } else { "panic-fg" });
        root = Some(sb.root.clone());
        if detached {
            let setsid = host_tool("setsid").expect("setsid for detached-client test");
            let out = sb
                .command(setsid.to_str().unwrap())
                .args(["--wait", BIN, "--file", FIXTURE])
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(1));
            let children = child_processes::children();
            assert_eq!(children.len(), 1, "one adopted daemon");
            pid = *children.first().unwrap();
        } else {
            let child = sb
                .wowdps(&["daemon", "--linger", "--file", FIXTURE])
                .spawn()
                .unwrap();
            pid = child.id() as i32;
            drop(child);
        }
        sb.wait_socket(true);
        panic!("test assertion after daemon startup");
    }));
    assert!(result.is_err());
    assert!(pid > 0);
    let waited = child_processes::reaped(pid);
    if !waited {
        child_processes::kill_and_wait(pid);
    }
    let root = root.unwrap();
    let removed = !root.exists();
    if !removed {
        let _ = std::fs::remove_dir_all(&root);
    }
    assert!(waited, "unwinding must stop and reap the owned daemon");
    assert!(
        removed,
        "scratch is removed only after the daemon is reaped"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn foreground_daemon_is_waited_for_when_a_test_panics() {
    panic_after_daemon_spawn(false);
}

#[cfg(target_os = "linux")]
#[test]
fn self_spawned_daemon_is_waited_for_when_a_test_panics() {
    panic_after_daemon_spawn(true);
}

fn shutdown(socket: &Path) {
    if let Ok(mut stream) = sock::connect(socket) {
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = stream.write_all(&ClientMsg::Shutdown.encode());
    }
}

/// Borrowing keeps the exact Child available for its normal wait while Drop
/// also covers readiness/assertion failures on every Unix platform.
struct ForegroundDaemon<'a> {
    child: &'a mut std::process::Child,
    socket: PathBuf,
}

impl Drop for ForegroundDaemon<'_> {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            shutdown(&self.socket);
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
