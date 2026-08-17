//! Compile-out stubs when feature `memory` is off.
//!
//! Keeps `crate::session::memory::*` paths stable so session wiring typechecks
//! without linking `xai-grok-memory` / sqlite-vec. Operations no-op or return
//! clear disabled errors.

#![allow(dead_code, unused_variables)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use xai_grok_tools::types::memory_backend::{MemoryBackend, MemorySearchResult};

fn not_compiled() -> String {
    "product memory is not compiled into this build (missing feature `memory`)".into()
}

// ── observation (type-stable with xai_grok_memory::observation) ───────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySearchSource {
    Tool,
    Injection,
    CompactionRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRetrievalMode {
    FtsOnly,
    Hybrid,
    EmbeddingFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySearchOutcome {
    Results,
    Empty,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySearchErrorClass {
    IndexOpen,
    Fts,
    Vector,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemorySearchObservation {
    pub source: MemorySearchSource,
    pub mode: MemoryRetrievalMode,
    pub outcome: MemorySearchOutcome,
    pub query_length: usize,
    pub keyword_count: usize,
    pub result_count: usize,
    pub top_score: f64,
    pub min_score_threshold: f64,
    pub duration_ms: u64,
    pub is_vector_available: bool,
    pub error_class: Option<MemorySearchErrorClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryWatcherSyncObservation {
    pub dirty_file_count: usize,
    pub is_claimed: bool,
    pub reindexed_count: usize,
    pub embedded_count: usize,
    pub duration_ms: u64,
}

pub trait MemoryObservationSink: Send + Sync {
    fn observe_search(&self, observation: MemorySearchObservation);
    fn observe_watcher_sync(&self, observation: MemoryWatcherSyncObservation);
}

impl MemoryObservationSink for () {
    fn observe_search(&self, _: MemorySearchObservation) {}
    fn observe_watcher_sync(&self, _: MemoryWatcherSyncObservation) {}
}

pub fn noop_memory_observation_sink() -> std::sync::Arc<dyn MemoryObservationSink> {
    std::sync::Arc::new(())
}

// ── storage ──────────────────────────────────────────────────────────────

pub mod storage {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MemoryScope {
        Global,
        Workspace,
    }

    #[derive(Debug, Clone)]
    pub struct MemoryStorage {
        global_dir: PathBuf,
        workspace_dir: PathBuf,
        workspace_path: PathBuf,
        ephemeral: bool,
    }

    impl MemoryStorage {
        pub fn new(cwd: &Path, root_override: Option<&Path>) -> Self {
            let root = root_override
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("/dev/null/memory-disabled"));
            Self {
                global_dir: root.clone(),
                workspace_dir: root,
                workspace_path: cwd.to_path_buf(),
                ephemeral: true,
            }
        }

        pub fn new_flat(cwd: &Path, root: &Path) -> Self {
            Self {
                global_dir: root.to_path_buf(),
                workspace_dir: root.to_path_buf(),
                workspace_path: cwd.to_path_buf(),
                ephemeral: true,
            }
        }

        #[cfg(any(test, feature = "test-support"))]
        pub fn with_paths(global_dir: PathBuf, workspace_dir: PathBuf) -> Self {
            Self {
                global_dir,
                workspace_dir,
                workspace_path: PathBuf::from("/test/workspace"),
                ephemeral: true,
            }
        }

        pub fn global_dir(&self) -> &Path {
            &self.global_dir
        }
        pub fn workspace_dir(&self) -> &Path {
            &self.workspace_dir
        }
        pub fn workspace_path(&self) -> &Path {
            &self.workspace_path
        }
        pub fn is_ephemeral(&self) -> bool {
            self.ephemeral
        }
        pub fn total_chunk_count(&self) -> usize {
            0
        }
        pub fn global_memory_file(&self) -> PathBuf {
            self.global_dir.join("MEMORY.md")
        }
        pub fn workspace_memory_file(&self) -> PathBuf {
            self.workspace_dir.join("MEMORY.md")
        }
        pub fn classify_source(&self, _path: &Path) -> &'static str {
            "workspace"
        }
        pub fn sessions_dir(&self) -> PathBuf {
            self.workspace_dir.join("sessions")
        }
        pub fn write_daily_log(
            &self,
            _date: &str,
            _slug: &str,
            _session_id: &str,
            _content: &str,
            _overwrite: bool,
        ) -> std::io::Result<PathBuf> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                not_compiled(),
            ))
        }
        pub fn write_long_term(&self, _scope: MemoryScope, _content: &str) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                not_compiled(),
            ))
        }
        pub fn append_to_memory(&self, _scope: MemoryScope, _content: &str) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                not_compiled(),
            ))
        }
        pub fn read_file(
            &self,
            _path: &Path,
            _from: Option<usize>,
            _lines: Option<usize>,
        ) -> std::io::Result<String> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                not_compiled(),
            ))
        }
        pub fn list_memory_files(&self) -> std::io::Result<Vec<PathBuf>> {
            Ok(Vec::new())
        }
        pub fn ensure_initialized(&self) -> std::io::Result<()> {
            Ok(())
        }
        pub fn clear_workspace(&self) -> std::io::Result<bool> {
            Ok(false)
        }
        pub fn clear_global(&self) -> std::io::Result<bool> {
            Ok(false)
        }
        pub fn gc(&self, _max_age_days: u64) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    pub fn slugify(input: &str, max_len: usize) -> String {
        let s: String = input
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        s.chars().take(max_len).collect()
    }
}

pub use storage::{MemoryScope, MemoryStorage};

// ── watcher ──────────────────────────────────────────────────────────────

pub mod watcher {
    use super::*;

    #[derive(Debug)]
    pub struct MemoryFileWatcher;

    impl MemoryFileWatcher {
        pub fn start(_memory_dir: &Path) -> Option<Self> {
            None
        }
        pub fn is_dirty(&self) -> bool {
            false
        }
        pub fn take_dirty(&self) -> Vec<PathBuf> {
            Vec::new()
        }
    }
}

// ── backend ──────────────────────────────────────────────────────────────

pub mod backend {
    use super::watcher::MemoryFileWatcher;
    use super::*;

    #[derive(Clone, Default)]
    pub struct EndpointScopedCredentials;

    impl std::fmt::Debug for EndpointScopedCredentials {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("EndpointScopedCredentials").finish()
        }
    }

    impl EndpointScopedCredentials {
        pub fn none() -> Self {
            Self
        }
        pub fn is_empty(&self) -> bool {
            true
        }
        pub fn for_endpoint(
            _endpoint: &str,
            _is_trusted: impl FnOnce(&str) -> bool,
            _auth_credentials: Option<Arc<dyn xai_grok_auth::AuthCredentialProvider>>,
            _api_key_provider: Option<xai_grok_tools::types::SharedApiKeyProvider>,
        ) -> Self {
            Self::none()
        }
    }

    #[derive(Clone)]
    pub struct MemoryBackendParams {
        pub session_id: String,
        pub embed_config: Option<xai_grok_config_types::MemoryEmbeddingConfig>,
        pub embed_base_url: String,
        pub embed_api_key: Option<String>,
        pub search_config: xai_grok_config_types::MemorySearchConfig,
        pub watcher: Option<Arc<MemoryFileWatcher>>,
        pub stale_claim_secs: i64,
        pub search_source: MemorySearchSource,
        pub observation_sink: Arc<dyn MemoryObservationSink>,
        pub embedding_credentials: EndpointScopedCredentials,
    }

    impl MemoryBackendParams {
        pub async fn make_embedding_provider(
            &self,
        ) -> Option<super::embedding::ApiEmbeddingProvider> {
            None
        }
    }

    pub struct MemoryBackendImpl {
        pub search_counter: Arc<std::sync::atomic::AtomicU64>,
    }

    impl MemoryBackendImpl {
        pub fn new(_db_path: PathBuf, _storage: MemoryStorage) -> Self {
            Self {
                search_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            }
        }

        pub fn from_session_params(_storage: MemoryStorage, _params: &MemoryBackendParams) -> Self {
            Self::new(PathBuf::new(), MemoryStorage::new(Path::new("/"), None))
        }
    }

    #[async_trait::async_trait]
    impl MemoryBackend for MemoryBackendImpl {
        async fn search(
            &self,
            _query: &str,
            _max_results: usize,
            _min_score: f64,
        ) -> Result<Vec<MemorySearchResult>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }

        fn get(
            &self,
            _path: &str,
            _from: Option<usize>,
            _lines: Option<usize>,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Err(not_compiled().into())
        }

        fn total_chunks(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
            Ok(0)
        }
    }
}

pub use backend::{EndpointScopedCredentials, MemoryBackendImpl, MemoryBackendParams};

// ── index ────────────────────────────────────────────────────────────────

pub mod index {
    use super::*;

    #[derive(Debug, Default, Clone)]
    pub struct MemoryIndexConfig;

    #[derive(Debug, Default, Clone, Copy)]
    pub struct ReindexResult {
        pub added: usize,
        pub updated: usize,
        pub removed: usize,
    }

    pub struct MemoryIndex;

    impl MemoryIndex {
        pub fn open_or_create(
            _db_path: &Path,
            _storage: MemoryStorage,
            _config: xai_grok_config_types::MemoryIndexConfig,
            _dimensions: usize,
        ) -> Result<Self, String> {
            Ok(Self)
        }

        pub fn vec_available(&self) -> bool {
            false
        }

        pub fn reindex_file(
            &mut self,
            _path: &Path,
            _source: &str,
        ) -> Result<ReindexResult, String> {
            Ok(ReindexResult::default())
        }

        pub fn delete_path(&mut self, _path: &Path) -> Result<usize, String> {
            Ok(0)
        }

        pub fn chunks_without_embeddings(&self) -> Result<Vec<(String, String)>, String> {
            Ok(Vec::new())
        }

        pub fn upsert_embedding(&self, _chunk_id: &str, _embedding: &[f32]) -> Result<(), String> {
            Ok(())
        }

        pub fn vector_search(
            &self,
            _embedding: &[f32],
            _limit: usize,
        ) -> Result<Vec<(String, f64)>, String> {
            Ok(Vec::new())
        }
    }

    pub fn init_sqlite_vec() {}
}

pub use index::{MemoryIndex, init_sqlite_vec};

// ── embedding ────────────────────────────────────────────────────────────

pub mod embedding {
    use super::*;

    #[async_trait::async_trait]
    pub trait EmbeddingProvider: Send + Sync {
        async fn embed_batch(
            &self,
            texts: &[&str],
        ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>>;
        fn model_name(&self) -> &str;
        fn dimensions(&self) -> usize;
    }

    pub struct ApiEmbeddingProvider;

    impl ApiEmbeddingProvider {
        pub fn from_session(
            _config: &xai_grok_config_types::MemoryEmbeddingConfig,
            _proxy_base_url: String,
            _auth_key: String,
        ) -> Option<Self> {
            None
        }
        pub fn from_config(
            _config: &xai_grok_config_types::MemoryEmbeddingConfig,
            _api_base: String,
            _client: reqwest_middleware::ClientWithMiddleware,
        ) -> Option<Self> {
            None
        }
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for ApiEmbeddingProvider {
        async fn embed_batch(
            &self,
            _texts: &[&str],
        ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
            Err(not_compiled().into())
        }
        fn model_name(&self) -> &str {
            "disabled"
        }
        fn dimensions(&self) -> usize {
            0
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub struct MockEmbeddingProvider {
        pub dimensions: usize,
    }

    #[cfg(any(test, feature = "test-support"))]
    #[async_trait::async_trait]
    impl EmbeddingProvider for MockEmbeddingProvider {
        async fn embed_batch(
            &self,
            texts: &[&str],
        ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
            Ok(texts.iter().map(|_| vec![0.0; self.dimensions]).collect())
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn dimensions(&self) -> usize {
            self.dimensions
        }
    }
}

// ── dream / dream_lock / archive / text_utils ────────────────────────────

pub mod dream_lock {
    use super::*;

    pub struct DreamLock;

    impl DreamLock {
        pub fn new(_workspace_dir: &Path) -> Self {
            Self
        }
        pub fn last_consolidated_at(&self) -> std::io::Result<Option<SystemTime>> {
            Ok(None)
        }
        pub fn try_acquire(&self, _stale_secs: u64) -> std::io::Result<Option<Option<SystemTime>>> {
            Ok(None)
        }
        pub fn rollback(&self, _prior: Option<SystemTime>) -> std::io::Result<()> {
            Ok(())
        }
        pub fn record_consolidation(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    pub fn sessions_since(
        _sessions_dir: &Path,
        _since: SystemTime,
        _exclude_sid8: Option<&str>,
    ) -> std::io::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

pub mod dream {
    use super::dream_lock::DreamLock;
    use super::*;

    pub const DREAM_SYSTEM_PROMPT: &str = "memory feature disabled";

    #[derive(Debug, PartialEq, Eq)]
    pub enum DreamGate {
        Open { sessions: Vec<String> },
        Disabled,
        TooSoon { hours_since: u64 },
        TooFewSessions { count: usize, required: u64 },
        Error(String),
    }

    pub fn check_dream_gates(
        _config: &xai_grok_config_types::MemoryDreamConfig,
        _lock: &DreamLock,
        _sessions_dir: &Path,
        _current_session_sid8: Option<&str>,
    ) -> DreamGate {
        DreamGate::Disabled
    }

    #[derive(Debug)]
    pub struct DreamResult {
        pub status: DreamStatus,
        pub sessions_eligible: usize,
        pub cleaned_stems: Vec<String>,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub enum DreamStatus {
        Skipped(String),
        Completed { chars_written: usize },
        NothingToConsolidate,
        Failed(String),
    }

    #[derive(Debug)]
    pub struct DreamMessage {
        pub content: String,
        pub processed_stems: Vec<String>,
    }

    pub fn build_dream_user_message(
        _sessions_dir: &Path,
        _stems: &[String],
        _existing_memory: Option<&str>,
    ) -> Option<DreamMessage> {
        None
    }

    pub fn process_dream_response(_response: &str) -> Option<String> {
        None
    }

    pub fn execute_dream(
        _lock: &DreamLock,
        _storage: &MemoryStorage,
        _response: &str,
        sessions_eligible: usize,
        _stale_lock_secs: u64,
        _sessions_dir: &Path,
        _processed_stems: &[String],
    ) -> DreamResult {
        DreamResult {
            status: DreamStatus::Skipped(not_compiled()),
            sessions_eligible,
            cleaned_stems: Vec::new(),
        }
    }
}

pub mod archive {
    use super::*;
    pub fn build_memory_archive(_storage: &MemoryStorage) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!(not_compiled()))
    }
}

pub mod text_utils {
    pub fn has_markdown_headers(text: &str) -> bool {
        text.lines().any(|l| l.trim_start().starts_with('#'))
    }
    pub fn is_no_reply(text: &str) -> bool {
        let n: String = text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        matches!(n.as_str(), "noreply")
    }
}

// ── flush (moved into xai-grok-memory; keep type-stable no-ops) ──────────

pub mod flush {
    use super::{MemoryIndex, embedding::EmbeddingProvider};
    use xai_grok_config_types::MemoryFlushConfig;

    pub const FLUSH_SYSTEM_PROMPT: &str = "memory feature disabled";
    pub const FLUSH_DELTA_SYSTEM_PROMPT: &str = "memory feature disabled";
    pub const SEMANTIC_DEDUP_SIMILARITY_THRESHOLD: f64 = 0.92;

    pub enum FlushResult {
        NothingToStore,
        Accepted(String),
        Rejected(String),
    }

    pub fn should_flush(
        _total_tokens: u64,
        _context_window: u64,
        _compact_threshold_percent: u8,
        _flush_config: &MemoryFlushConfig,
        _last_flush_compaction: u64,
        _current_compaction_count: u64,
    ) -> bool {
        false
    }

    pub fn process_flush_response(
        _response: &str,
        _config: &MemoryFlushConfig,
    ) -> FlushResult {
        FlushResult::NothingToStore
    }

    pub async fn is_semantically_duplicate(
        _content: &str,
        _index: &MemoryIndex,
        _embedding_provider: Option<&dyn EmbeddingProvider>,
        _threshold: f64,
    ) -> bool {
        false
    }
}

// ── leftover re-export names used as module paths ────────────────────────

pub mod chunker {}
pub mod mmr {}
pub mod query_expansion {}
pub mod schema {}
pub mod search {}

pub async fn embed_missing_chunks(
    _index: &MemoryIndex,
    _provider: &dyn embedding::EmbeddingProvider,
) -> usize {
    0
}
