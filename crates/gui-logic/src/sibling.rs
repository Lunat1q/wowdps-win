//! Which daemon binary a GUI starts when none is running, and the
//! extractor it runs first when the art caches are missing.

use std::path::{Path, PathBuf};

/// The daemon binary to spawn when none is running: the sibling `wowdps`
/// from the same build, else whatever PATH resolves.
pub fn daemon_bin() -> PathBuf {
    beside(std::env::current_exe().ok().as_deref())
}

/// The caches `wowdps-extract setup` makes that a start checks for. The
/// emblems are left out: they come from the launcher, which an install
/// may not have, and a start must not rerun setup for them each time.
pub const ART: &[&str] = &[
    "class-icons.bin",
    "spell-icons.bin",
    "spell-tips.bin",
    "talent-art.bin",
    "talents.json",
];

/// The extractor to run `setup` with before the window opens: the sibling
/// `wowdps-extract` (shipped beside the GUI on Windows), when one of
/// [`ART`] is missing. Never from PATH: a checkout's extractor is the
/// gen-*.sh scripts' to run.
pub fn art_setup() -> Option<PathBuf> {
    let missing = ART
        .iter()
        .any(|f| wowdps_proto::dirs::data_path(f).is_none_or(|p| !p.exists()));
    let exe = std::env::current_exe().ok()?;
    missing.then(|| extractor_beside(&exe)).flatten()
}

fn extractor_beside(exe: &Path) -> Option<PathBuf> {
    exe.parent()
        .map(|dir| dir.join(format!("wowdps-extract{}", std::env::consts::EXE_SUFFIX)))
        .filter(|p| p.exists())
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

    #[test]
    fn the_extractor_is_only_ever_the_sibling() {
        let dir =
            crate::test_scratch::root().join(format!("wowdps-extractor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gui = dir.join("wowdps-gui");
        assert_eq!(extractor_beside(&gui), None, "no sibling, no PATH");
        let extract = dir.join(format!("wowdps-extract{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&extract, "").unwrap();
        assert_eq!(extractor_beside(&gui), Some(extract));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
