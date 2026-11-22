//! Which daemon binary a GUI starts when none is running.

use std::path::{Path, PathBuf};

/// The daemon binary to spawn when none is running: the sibling `wowdps`
/// from the same build, else whatever PATH resolves.
pub fn daemon_bin() -> PathBuf {
    beside(std::env::current_exe().ok().as_deref())
}

fn beside(exe: Option<&Path>) -> PathBuf {
    exe.and_then(Path::parent)
        .map(|dir| dir.join(format!("wowdps{}", std::env::consts::EXE_SUFFIX)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("wowdps"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_is_the_sibling_else_the_name_on_path() {
        let dir =
            crate::test_scratch::root().join(format!("wowdps-sibling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gui = dir.join("wowdps-gui");
        assert_eq!(
            beside(Some(&gui)),
            PathBuf::from("wowdps"),
            "no sibling yet"
        );
        let daemon = dir.join(format!("wowdps{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&daemon, "").unwrap();
        assert_eq!(beside(Some(&gui)), daemon);
        assert_eq!(beside(None), PathBuf::from("wowdps"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
