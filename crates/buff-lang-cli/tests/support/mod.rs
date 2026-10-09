//! Shared CLI test support: self-healing temp-root hygiene (ITER-53D).
//!
//! Every CLI test binary creates a per-process temp root
//! `<temp>/<name>-<pid>/`. Roots from crashed or finished runs used to
//! accumulate forever (1,868 leaked dirs / 723 MB measured on one dev
//! machine before this module existed). Each `temp_root()` call now
//! sweeps STALE sibling roots of the same prefix (mtime older than
//! `MAX_ROOT_AGE`) before creating the current one, so the directory
//! self-heals on the next run instead of growing unboundedly. Fresh
//! roots are never touched — a same-day sibling may belong to a live
//! parallel binary, and CI containers are ephemeral anyway.

use std::path::PathBuf;
use std::time::Duration;

/// Roots older than this are considered abandoned.
pub const MAX_ROOT_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Remove `<temp>/<prefix>-*` directories not modified within `max_age`.
/// The CURRENT process's own root (`<prefix>-<pid>`) is never touched.
/// Best-effort: unreadable or locked entries are skipped.
pub fn sweep_stale_roots(prefix: &str, max_age: Duration) {
    let temp = std::env::temp_dir();
    let mine = format!("{prefix}-{}", std::process::id());
    let Ok(entries) = std::fs::read_dir(&temp) else {
        return;
    };
    let cutoff = std::time::SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(prefix) || name == mine {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if modified <= cutoff {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Standard per-process temp root with stale-sweep hygiene: returns
/// `<temp>/<name>-<pid>/` (created), after sweeping stale siblings.
/// (`dead_code` allowed: each including test binary uses a different
/// subset of this module's helpers.)
#[allow(dead_code)]
pub fn temp_root(name: &str) -> PathBuf {
    sweep_stale_roots(name, MAX_ROOT_AGE);
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_removes_stale_but_spares_current_root() {
        let temp = std::env::temp_dir();
        let stale = temp.join(format!("buff-sweeptest-stale-{}", std::process::id()));
        std::fs::create_dir_all(&stale).expect("create stale root");
        std::fs::write(stale.join("marker.txt"), "x").expect("marker");
        let mine = temp.join(format!("buff-sweeptest-{}", std::process::id()));
        std::fs::create_dir_all(&mine).expect("create current root");

        // Zero age => cutoff is now => everything except the current
        // process's own root is stale.
        sweep_stale_roots("buff-sweeptest", Duration::ZERO);

        assert!(!stale.exists(), "stale root must be swept");
        assert!(mine.exists(), "current root must survive");
        let _ = std::fs::remove_dir_all(&mine);
    }
}
