use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const AUTH_FILE: &str = "chatgpt-auth.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatgptAuth {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residency: Option<String>,
}

impl ChatgptAuth {
    pub(crate) fn expires_in_secs(&self) -> Option<u64> {
        let expires_at = self.expires_at?;
        let remaining = expires_at.signed_duration_since(Utc::now());
        Some(remaining.num_seconds().max(0) as u64)
    }

    pub(crate) fn needs_refresh(&self) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false;
        };
        let skew =
            chrono::Duration::seconds(xai_grok_login::PROVIDER_TOKEN_EXPIRY_SKEW_SECS as i64);
        expires_at <= Utc::now() + skew
    }
}

pub(crate) fn auth_path() -> PathBuf {
    crate::util::grok_home::grok_home().join(AUTH_FILE)
}

pub(crate) fn load() -> anyhow::Result<ChatgptAuth> {
    load_from(&auth_path())
}

fn load_from(path: &Path) -> anyhow::Result<ChatgptAuth> {
    let bytes = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("no ChatGPT credentials; run grok chatgpt-login")
        } else {
            anyhow::anyhow!("failed to read ChatGPT credentials: {e}")
        }
    })?;
    let auth: ChatgptAuth = serde_json::from_slice(&bytes)
        .map_err(|e| anyhow::anyhow!("invalid ChatGPT credentials file: {e}"))?;
    if auth.access_token.trim().is_empty() {
        anyhow::bail!("ChatGPT credentials file has an empty access_token");
    }
    Ok(auth)
}

/// The sidecar is never removed: replacing the credentials must not replace the lock inode.
fn open_lock(path: &Path) -> anyhow::Result<std::fs::File> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid credential path"))?;
    std::fs::create_dir_all(parent)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path.with_extension("lock"))?)
}

fn try_lock(path: &Path) -> anyhow::Result<std::fs::File> {
    let file = open_lock(path)?;
    file.try_lock_exclusive()
        .map_err(|_| anyhow::anyhow!("ChatGPT credentials are busy; retry after token refresh"))?;
    Ok(file)
}

async fn lock(path: &Path) -> anyhow::Result<std::fs::File> {
    let file = open_lock(path)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "ChatGPT credential lock timed out"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn save_to(path: &Path, auth: &ChatgptAuth) -> anyhow::Result<()> {
    if load_from(path).ok().as_ref() == Some(auth) {
        crate::util::secure_file::ensure_owner_only_permissions(path)?;
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid credential path"))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    // Tighten the empty temporary file before any secret bytes are written, including on Windows.
    crate::util::secure_file::ensure_owner_only_permissions(temp.path())?;
    temp.write_all(&serde_json::to_vec_pretty(auth)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| {
        anyhow::anyhow!("failed to publish ChatGPT credentials: {}", error.error)
    })?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn save(auth: &ChatgptAuth) -> anyhow::Result<()> {
    let path = auth_path();
    let _lock = try_lock(&path)?;
    save_to(&path, auth)
}

pub(crate) async fn save_login(auth: &ChatgptAuth) -> anyhow::Result<()> {
    let path = auth_path();
    let _lock = lock(&path).await?;
    save_to(&path, auth)
}

pub(crate) fn clear() -> anyhow::Result<()> {
    let path = auth_path();
    let _lock = try_lock(&path)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow::anyhow!("failed to remove ChatGPT credentials: {e}")),
    }
}

pub(crate) async fn fresh() -> anyhow::Result<ChatgptAuth> {
    refresh_with(&auth_path(), super::oauth::ensure_fresh).await
}

async fn refresh_with<F, Fut>(path: &Path, refresh: F) -> anyhow::Result<ChatgptAuth>
where
    F: FnOnce(ChatgptAuth) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<ChatgptAuth>>,
{
    let _lock = lock(path).await?;
    let auth = load_from(path)?;
    let fresh = refresh(auth.clone()).await?;
    // Also reject an external writer that does not participate in our locking protocol.
    anyhow::ensure!(
        load_from(path)? == auth,
        "ChatGPT authentication changed during refresh"
    );
    if fresh != auth {
        save_to(path, &fresh)?;
    }
    Ok(fresh)
}

/// A shared credential source, never an authoritative in-memory cache.
#[derive(Debug, Clone)]
pub(crate) struct SharedStore {
    path: PathBuf,
}

impl SharedStore {
    pub(crate) fn new() -> Self {
        Self { path: auth_path() }
    }

    pub(crate) fn snapshot(&self) -> Option<ChatgptAuth> {
        load_from(&self.path).ok()
    }

    pub(crate) async fn fresh(&self) -> anyhow::Result<ChatgptAuth> {
        refresh_with(&self.path, super::oauth::ensure_fresh).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_test_support::EnvGuard;

    fn auth(token: &str) -> ChatgptAuth {
        ChatgptAuth {
            access_token: token.into(),
            refresh_token: Some("refresh".into()),
            expires_at: None,
            account_id: token.into(),
            id_token: None,
            residency: None,
        }
    }

    #[tokio::test]
    async fn refresh_reloads_after_waiting_for_another_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        save_to(&path, &auth("old")).unwrap();
        let (first, second) = tokio::join!(
            biased;
            refresh_with(&path, |mut auth| async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                auth.access_token = "rotated".into();
                Ok(auth)
            }),
            refresh_with(&path, |auth| async move {
                assert_eq!(auth.access_token, "rotated");
                Ok(auth)
            }),
        );
        assert_eq!(first.unwrap().access_token, "rotated");
        assert_eq!(second.unwrap().access_token, "rotated");
    }

    #[tokio::test]
    async fn refresh_cannot_resurrect_deleted_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        save_to(&path, &auth("old")).unwrap();
        let result = refresh_with(&path, |auth| {
            std::fs::remove_file(&path).unwrap();
            std::future::ready(Ok(auth))
        })
        .await;
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn refresh_cannot_overwrite_replacement_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        save_to(&path, &auth("old")).unwrap();
        let result = refresh_with(&path, |old| {
            save_to(&path, &auth("new-account")).unwrap();
            std::future::ready(Ok(old))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(load_from(&path).unwrap().account_id, "new-account");
    }

    #[tokio::test]
    async fn unchanged_refresh_does_not_rewrite_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        save_to(&path, &auth("same")).unwrap();
        let past = filetime::FileTime::from_unix_time(1, 0);
        filetime::set_file_mtime(&path, past).unwrap();
        refresh_with(&path, |auth| async { Ok(auth) })
            .await
            .unwrap();
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&path).unwrap()),
            past
        );
    }

    #[test]
    fn snapshots_follow_logout_and_account_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        let store = SharedStore { path: path.clone() };
        save_to(&path, &auth("old")).unwrap();
        assert_eq!(store.snapshot().unwrap().account_id, "old");
        save_to(&path, &auth("new")).unwrap();
        assert_eq!(store.snapshot().unwrap().account_id, "new");
        std::fs::remove_file(&path).unwrap();
        assert!(store.snapshot().is_none());
    }

    #[test]
    fn lock_contends_across_independent_handles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        let guard = try_lock(&path).unwrap();
        assert!(try_lock(&path).is_err());
        drop(guard);
        assert!(try_lock(&path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_replaces_world_readable_inode_with_owner_only_file() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        save_to(&path, &auth("old")).unwrap();
        let old = std::fs::File::open(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        save_to(&path, &auth("new")).unwrap();
        let current = std::fs::metadata(&path).unwrap();
        assert_ne!(old.metadata().unwrap().ino(), current.ino());
        assert_eq!(current.permissions().mode() & 0o777, 0o600);
        assert_eq!(load_from(&path).unwrap().access_token, "new");
    }

    #[tokio::test]
    async fn cancelled_refresh_releases_lock_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(AUTH_FILE);
        save_to(&path, &auth("old")).unwrap();
        let result = tokio::time::timeout(
            Duration::from_millis(10),
            refresh_with(&path, |_| std::future::pending()),
        )
        .await;
        assert!(result.is_err());
        assert!(try_lock(&path).is_ok());
        assert_eq!(load_from(&path).unwrap().access_token, "old");
    }

    #[test]
    fn chatgpt_lock_child() {
        let Some(path) = std::env::var_os("GROK_TEST_CHATGPT_LOCK_PATH") else {
            return;
        };
        let expect_locked = std::env::var_os("GROK_TEST_CHATGPT_LOCK_HELD").is_some();
        assert_eq!(try_lock(Path::new(&path)).is_err(), expect_locked);
    }

    #[test]
    #[serial_test::serial]
    fn lock_serializes_other_processes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("GROK_HOME", dir.path());
        let path = dir.path().join(AUTH_FILE);
        let run_child = |held: bool| {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .arg("agent::chatgpt::store::tests::chatgpt_lock_child")
                .arg("--exact")
                .env("GROK_TEST_CHATGPT_LOCK_PATH", &path)
                .env_remove("GROK_TEST_CHATGPT_LOCK_HELD");
            if held {
                command.env("GROK_TEST_CHATGPT_LOCK_HELD", "1");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        };
        let guard = try_lock(&path).unwrap();
        run_child(true);
        drop(guard);
        run_child(false);
    }

    #[test]
    #[serial_test::serial]
    fn roundtrip_owner_only() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("GROK_HOME", home.path());
        let auth = ChatgptAuth {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
            account_id: "acct-1".into(),
            id_token: None,
            residency: Some("us".into()),
        };
        save(&auth).unwrap();
        let loaded = load().unwrap();
        assert_eq!(loaded.access_token, "access");
        assert_eq!(loaded.account_id, "acct-1");
        assert_eq!(loaded.residency.as_deref(), Some("us"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(auth_path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        clear().unwrap();
        assert!(load().is_err());
    }
}
