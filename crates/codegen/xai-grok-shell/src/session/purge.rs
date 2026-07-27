//! Full history/log purge (`/purge`, `x.ai/session/purge`).
//!
//! Lives outside [`super::persistence`] so that large upstream persistence
//! rewrites do not conflict with this fork-only cleanup path.

use std::io;
use std::path::Path;
use std::sync::Arc;

use super::persistence::{delete_session_history, list_summaries};

/// Result of [`purge_all_history_and_logs`].
///
/// Counts are best-effort tallies of work performed. Failures for individual
/// sessions are collected in [`Self::errors`] rather than aborting the whole
/// purge so a single remote blip does not leave local history behind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Local session directories removed via [`delete_session_history`].
    pub sessions_removed: usize,
    /// Remote (writeback) copies removed.
    pub remote_removed: usize,
    /// Top-level entries removed under `~/.grok/sessions/` during the sweep
    /// (orphans, search index, leftover cwd groups).
    pub sessions_dir_entries_cleared: usize,
    /// Top-level entries removed under `~/.grok/logs/`.
    pub logs_dir_entries_cleared: usize,
    /// Per-session failures that did not stop the rest of the purge.
    pub errors: Vec<String>,
}

/// Permanently delete **all** local session history and logs under
/// [`crate::util::grok_home::grok_home`], optionally deleting remote
/// writeback copies for each known local session.
///
/// Order:
/// 1. List every local session (`list_summaries(None)`).
/// 2. For each, call [`delete_session_history`] (remote-first when
///    `needs_remote`). Per-session errors are recorded and the rest continue.
/// 3. Sweep remaining contents of `sessions/` (orphans + search index).
/// 4. Sweep contents of `logs/`.
///
/// Does **not** remove auth credentials, config, skills, plugins, or
/// cross-session memory. Safe to call when no sessions exist (no-op sweep).
pub async fn purge_all_history_and_logs(
    needs_remote: bool,
    auth_manager: Arc<crate::auth::AuthManager>,
) -> PurgeReport {
    let mut report = PurgeReport::default();

    match list_summaries(None).await {
        Ok(summaries) => {
            for summary in summaries {
                let session_id = summary.info.id.to_string();
                let cwd = summary.info.cwd.clone();
                match delete_session_history(
                    &session_id,
                    Some(cwd.as_str()),
                    needs_remote,
                    auth_manager.clone(),
                )
                .await
                {
                    Ok(deletion) => {
                        if deletion.local_removed {
                            report.sessions_removed += 1;
                        }
                        if deletion.remote_removed {
                            report.remote_removed += 1;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            session_id = %session_id,
                            "purge: failed to delete session; continuing"
                        );
                        report.errors.push(format!("session {session_id}: {e}"));
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "purge: failed to list sessions; sweeping dirs anyway");
            report.errors.push(format!("list sessions: {e}"));
        }
    }

    let home = crate::util::grok_home::grok_home();
    match clear_directory_contents(&home.join("sessions")) {
        Ok(n) => report.sessions_dir_entries_cleared = n,
        Err(e) => {
            tracing::warn!(error = %e, "purge: failed to clear sessions directory");
            report.errors.push(format!("clear sessions/: {e}"));
        }
    }
    match clear_directory_contents(&home.join("logs")) {
        Ok(n) => report.logs_dir_entries_cleared = n,
        Err(e) => {
            tracing::warn!(error = %e, "purge: failed to clear logs directory");
            report.errors.push(format!("clear logs/: {e}"));
        }
    }

    report
}

/// Remove every entry under `dir`, leaving the directory itself (recreated
/// if missing). Returns the number of top-level entries removed.
/// Missing `dir` is success with count 0.
fn clear_directory_contents(dir: &Path) -> io::Result<usize> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        return Ok(0);
    }
    if !dir.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let mut removed = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::clear_directory_contents;
    use std::fs;
    use std::path::Path;

    #[test]
    fn clear_directory_contents_removes_files_and_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("sessions");
        fs::create_dir_all(root.join("cwd-a").join("sid-1")).unwrap();
        fs::write(root.join("session_search.sqlite"), b"idx").unwrap();
        fs::write(root.join("cwd-a").join("sid-1").join("summary.json"), b"{}").unwrap();

        let n = clear_directory_contents(&root).unwrap();
        assert_eq!(n, 2, "cwd group + search index");
        assert!(root.is_dir(), "parent dir must remain");
        assert!(
            fs::read_dir(&root).unwrap().next().is_none(),
            "dir must be empty after clear"
        );
    }

    #[test]
    fn clear_directory_contents_creates_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("logs");
        assert!(!root.exists());
        let n = clear_directory_contents(&root).unwrap();
        assert_eq!(n, 0);
        assert!(root.is_dir());
    }

    #[test]
    fn clear_directory_contents_rejects_file_path() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-dir");
        fs::write(&file, b"x").unwrap();
        let err = clear_directory_contents(Path::new(&file)).unwrap_err();
        assert!(
            err.to_string().contains("not a directory"),
            "got: {err}"
        );
    }
}
