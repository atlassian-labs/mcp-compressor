//! `ToolCache` — lazily populated, refresh-on-demand tool schema store.
//!
//! The cache is populated on the first call to [`ToolCache::get_all`] and
//! reused for subsequent calls.  Explicit [`ToolCache::refresh`] forces a
//! re-fetch from the backend.
//!
//! Include/exclude filters are applied at population time, so every read
//! after the initial fetch sees only the filtered view.
//!
//! # Concurrency
//!
//! `ToolCache` uses a `tokio::sync::RwLock` for the cached data.  Multiple
//! concurrent readers do not block each other.  A write (populate or refresh)
//! acquires an exclusive lock.  Double-checked locking prevents redundant
//! backend fetches when multiple tasks race to populate the cache.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::compression::engine::Tool;
use crate::Error;

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Abstraction over the upstream MCP client used in tests and production.
///
/// In production this is backed by the official Rust MCP SDK client.
/// In tests it is a `MockBackend`.
///
/// Async fn in traits requires Rust ≥ 1.75 (stable in our toolchain).
pub trait ToolBackend: Send + Sync {
    /// Fetch the current tool list from the backend server.
    fn list_tools(&self) -> impl std::future::Future<Output = Result<Vec<Tool>, Error>> + Send;
}

// ---------------------------------------------------------------------------
// ToolCache
// ---------------------------------------------------------------------------

/// Lazily-populated, thread-safe tool schema cache.
///
/// Owns a `ToolBackend` (generic parameter `B`) and an optional include/exclude
/// filter that is applied when the cache is populated.
pub struct ToolCache<B: ToolBackend> {
    backend: B,
    cache: Arc<RwLock<Option<Vec<Tool>>>>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

impl<B: ToolBackend> ToolCache<B> {
    /// Create a new, empty (unpopulated) cache wrapping `backend`.
    ///
    /// `include`: if `Some`, only tools whose names are in this list are kept.
    /// `exclude`: if `Some`, tools whose names are in this list are removed.
    /// Both filters are applied if both are specified (include then exclude).
    pub fn new(
        backend: B,
        include: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
    ) -> Self {
        todo!()
    }

    /// Return `true` if the cache has been populated (either by a previous
    /// `get_all` call or by `refresh`).
    pub fn is_populated(&self) -> bool {
        todo!()
    }

    /// Return all cached tools, fetching from the backend on first call.
    ///
    /// Subsequent calls return the in-memory cache without touching the
    /// backend (double-checked locking prevents redundant fetches).
    pub async fn get_all(&self) -> Result<Vec<Tool>, Error> {
        todo!()
    }

    /// Return a single tool by name, or `None` if not found.
    pub async fn get(&self, name: &str) -> Result<Option<Tool>, Error> {
        todo!()
    }

    /// Force a re-fetch from the backend, discarding the current cache.
    pub async fn refresh(&self) -> Result<(), Error> {
        todo!()
    }

    /// Invalidate (clear) the cache without re-fetching.
    ///
    /// The next call to `get_all` or `get` will re-fetch from the backend.
    pub fn invalidate(&self) {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ------------------------------------------------------------------
    // Mock backend
    // ------------------------------------------------------------------

    /// Simple mock that records how many times `list_tools` has been called.
    #[derive(Clone)]
    struct MockBackend {
        tools: Vec<Tool>,
        call_count: Arc<AtomicU32>,
    }

    impl MockBackend {
        fn new(tools: Vec<Tool>) -> Self {
            Self { tools, call_count: Arc::new(AtomicU32::new(0)) }
        }

        fn call_count(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl ToolBackend for MockBackend {
        async fn list_tools(&self) -> Result<Vec<Tool>, Error> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.tools.clone())
        }
    }

    // Helper: build a named tool with no description.
    fn make_tool(name: &str) -> Tool {
        Tool::new(name, None::<String>, json!({ "type": "object", "properties": {} }))
    }

    // ------------------------------------------------------------------
    // Initial state
    // ------------------------------------------------------------------

    /// A freshly created cache is not populated.
    #[tokio::test]
    async fn new_cache_is_not_populated() {
        let backend = MockBackend::new(vec![]);
        let cache = ToolCache::new(backend, None, None);
        assert!(!cache.is_populated());
    }

    // ------------------------------------------------------------------
    // get_all — fetch on first call
    // ------------------------------------------------------------------

    /// get_all() calls the backend exactly once on first access.
    #[tokio::test]
    async fn get_all_fetches_from_backend_on_first_call() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let call_count = backend.call_count.clone();
        let cache = ToolCache::new(backend, None, None);
        let _ = cache.get_all().await.unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    /// get_all() returns the expected tools.
    #[tokio::test]
    async fn get_all_returns_expected_tools() {
        let backend = MockBackend::new(vec![make_tool("fetch"), make_tool("search")]);
        let cache = ToolCache::new(backend, None, None);
        let tools = cache.get_all().await.unwrap();
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"fetch"));
        assert!(names.contains(&"search"));
    }

    /// The cache is populated after the first get_all() call.
    #[tokio::test]
    async fn cache_is_populated_after_first_get_all() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let cache = ToolCache::new(backend, None, None);
        let _ = cache.get_all().await.unwrap();
        assert!(cache.is_populated());
    }

    // ------------------------------------------------------------------
    // get_all — cache hit (second call)
    // ------------------------------------------------------------------

    /// The backend is called only once across multiple get_all() calls.
    #[tokio::test]
    async fn get_all_uses_cache_on_subsequent_calls() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let call_count = backend.call_count.clone();
        let cache = ToolCache::new(backend, None, None);
        let _ = cache.get_all().await.unwrap();
        let _ = cache.get_all().await.unwrap();
        let _ = cache.get_all().await.unwrap();
        // Backend must have been called exactly once
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "backend called more than once");
    }

    // ------------------------------------------------------------------
    // get — tool lookup
    // ------------------------------------------------------------------

    /// get() returns Some for a known tool name.
    #[tokio::test]
    async fn get_returns_some_for_known_tool() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let cache = ToolCache::new(backend, None, None);
        let tool = cache.get("fetch").await.unwrap();
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, "fetch");
    }

    /// get() returns None for an unknown tool name.
    #[tokio::test]
    async fn get_returns_none_for_unknown_tool() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let cache = ToolCache::new(backend, None, None);
        let tool = cache.get("nonexistent").await.unwrap();
        assert!(tool.is_none());
    }

    // ------------------------------------------------------------------
    // refresh
    // ------------------------------------------------------------------

    /// refresh() forces a re-fetch from the backend.
    #[tokio::test]
    async fn refresh_forces_re_fetch() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let call_count = backend.call_count.clone();
        let cache = ToolCache::new(backend, None, None);
        let _ = cache.get_all().await.unwrap(); // first fetch
        cache.refresh().await.unwrap();          // forces re-fetch
        assert_eq!(call_count.load(Ordering::SeqCst), 2, "expected 2 backend calls after refresh");
    }

    // ------------------------------------------------------------------
    // invalidate
    // ------------------------------------------------------------------

    /// invalidate() clears the cache; the next get_all() re-fetches.
    #[tokio::test]
    async fn invalidate_clears_cache() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let call_count = backend.call_count.clone();
        let cache = ToolCache::new(backend, None, None);
        let _ = cache.get_all().await.unwrap(); // fetch #1
        cache.invalidate();
        assert!(!cache.is_populated());
        let _ = cache.get_all().await.unwrap(); // fetch #2
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    // ------------------------------------------------------------------
    // Include / exclude filters
    // ------------------------------------------------------------------

    /// An include filter keeps only the named tools.
    #[tokio::test]
    async fn include_filter_keeps_only_named_tools() {
        let backend =
            MockBackend::new(vec![make_tool("fetch"), make_tool("search"), make_tool("upload")]);
        let cache = ToolCache::new(backend, Some(vec!["fetch".into()]), None);
        let tools = cache.get_all().await.unwrap();
        assert_eq!(tools.len(), 1, "expected only 'fetch'");
        assert_eq!(tools[0].name, "fetch");
    }

    /// An exclude filter removes the named tools.
    #[tokio::test]
    async fn exclude_filter_removes_named_tools() {
        let backend =
            MockBackend::new(vec![make_tool("fetch"), make_tool("search"), make_tool("upload")]);
        let cache = ToolCache::new(backend, None, Some(vec!["search".into()]));
        let tools = cache.get_all().await.unwrap();
        assert_eq!(tools.len(), 2, "expected 'fetch' and 'upload'");
        assert!(tools.iter().all(|t| t.name != "search"));
    }

    /// When both include and exclude filters are specified, include is applied
    /// first, then exclude is applied to the included set.
    #[tokio::test]
    async fn include_then_exclude_applied_in_order() {
        let backend =
            MockBackend::new(vec![make_tool("fetch"), make_tool("search"), make_tool("upload")]);
        // Include fetch+search, then exclude search → only fetch
        let cache = ToolCache::new(
            backend,
            Some(vec!["fetch".into(), "search".into()]),
            Some(vec!["search".into()]),
        );
        let tools = cache.get_all().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "fetch");
    }

    /// An include filter that matches no tools results in an empty list.
    #[tokio::test]
    async fn include_filter_no_matches_yields_empty() {
        let backend = MockBackend::new(vec![make_tool("fetch")]);
        let cache = ToolCache::new(backend, Some(vec!["nonexistent".into()]), None);
        let tools = cache.get_all().await.unwrap();
        assert!(tools.is_empty());
    }

    /// An exclude filter that matches all tools results in an empty list.
    #[tokio::test]
    async fn exclude_filter_all_tools_yields_empty() {
        let backend = MockBackend::new(vec![make_tool("fetch"), make_tool("search")]);
        let cache =
            ToolCache::new(backend, None, Some(vec!["fetch".into(), "search".into()]));
        let tools = cache.get_all().await.unwrap();
        assert!(tools.is_empty());
    }

    // ------------------------------------------------------------------
    // Edge cases
    // ------------------------------------------------------------------

    /// A backend with no tools yields an empty list.
    #[tokio::test]
    async fn empty_backend_yields_empty_list() {
        let backend = MockBackend::new(vec![]);
        let cache = ToolCache::new(backend, None, None);
        let tools = cache.get_all().await.unwrap();
        assert!(tools.is_empty());
    }
}
