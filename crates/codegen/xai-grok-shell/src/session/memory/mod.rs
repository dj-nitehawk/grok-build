//! Memory system shim.
//!
//! When feature `memory` is on, re-exports the standalone `xai-grok-memory`
//! crate under historical `crate::session::memory::*` paths.
//!
//! When off (fork slim default), [`disabled`] provides type-stable no-ops so
//! session wiring still compiles without linking product memory / sqlite-vec.
//!
//! Only `hooks` stays here always: session glue (depends on sampling /
//! session_compact) and is not part of the relocatable core engine.

pub mod hooks;

#[cfg(feature = "memory")]
pub use xai_grok_memory::{
    EndpointScopedCredentials, MemoryBackendImpl, MemoryBackendParams, MemoryIndex,
    MemoryObservationSink, MemoryRetrievalMode, MemoryScope, MemorySearchErrorClass,
    MemorySearchObservation, MemorySearchOutcome, MemorySearchSource, MemoryStorage,
    MemoryWatcherSyncObservation, archive, backend, chunker, dream, dream_lock,
    embed_missing_chunks, embedding, flush, index, init_sqlite_vec, mmr,
    noop_memory_observation_sink, query_expansion, schema, search, storage, text_utils,
    watcher,
};

#[cfg(not(feature = "memory"))]
mod disabled;

#[cfg(not(feature = "memory"))]
pub use disabled::{
    EndpointScopedCredentials, MemoryBackendImpl, MemoryBackendParams, MemoryIndex,
    MemoryObservationSink, MemoryRetrievalMode, MemoryScope, MemorySearchErrorClass,
    MemorySearchObservation, MemorySearchOutcome, MemorySearchSource, MemoryStorage,
    MemoryWatcherSyncObservation, archive, backend, chunker, dream, dream_lock,
    embed_missing_chunks, embedding, flush, index, init_sqlite_vec, mmr,
    noop_memory_observation_sink, query_expansion, schema, search, storage, text_utils,
    watcher,
};
