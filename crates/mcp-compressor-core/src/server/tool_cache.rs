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
//! `ToolCache` uses a `tokio::sync::RwLock` for the cached data.  Readers share
//! that lock, while generation validation is briefly serialized with cache
//! publication and invalidation.  A write (populate or refresh) acquires an
//! exclusive lock.  Double-checked locking prevents redundant backend fetches
//! when multiple tasks race to populate the cache.  An invalidated in-flight
//! fetch retries instead of publishing stale tools.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

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
    cache: Arc<RwLock<Option<CachedTools>>>,
    populated: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    fetch_sequence: Arc<AtomicU64>,
    publication_lock: Arc<Mutex<()>>,
    #[cfg(test)]
    snapshot_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    publication_hook: Option<Arc<dyn Fn(u64, &[Tool]) + Send + Sync>>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct CachedTools {
    generation: u64,
    /// Ticket of the fetch that produced `tools`, taken before the fetch
    /// started. A fetch that started earlier holds older data and must never
    /// overwrite a snapshot published from a later fetch.
    sequence: u64,
    tools: Vec<Tool>,
}

impl<B: ToolBackend> ToolCache<B> {
    /// Create a new, empty (unpopulated) cache wrapping `backend`.
    ///
    /// `include`: if `Some`, only tools whose names are in this list are kept.
    /// `exclude`: if `Some`, tools whose names are in this list are removed.
    /// Both filters are applied if both are specified (include then exclude).
    pub fn new(backend: B, include: Option<Vec<String>>, exclude: Option<Vec<String>>) -> Self {
        Self {
            backend,
            cache: Arc::new(RwLock::new(None)),
            populated: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            fetch_sequence: Arc::new(AtomicU64::new(0)),
            publication_lock: Arc::new(Mutex::new(())),
            #[cfg(test)]
            snapshot_hook: None,
            #[cfg(test)]
            publication_hook: None,
            include,
            exclude,
        }
    }

    /// Return `true` if the cache has been populated (either by a previous
    /// `get_all` call or by `refresh`).
    pub fn is_populated(&self) -> bool {
        self.populated.load(Ordering::SeqCst)
    }

    /// Return all cached tools, fetching from the backend on first call.
    ///
    /// Subsequent calls return the in-memory cache without touching the
    /// backend (double-checked locking prevents redundant fetches).
    pub async fn get_all(&self) -> Result<Vec<Tool>, Error> {
        let cache = self.cache.read().await;
        let cached = cache
            .as_ref()
            .map(|cached| (cached.generation, cached.tools.clone()));
        let publication = self.lock_publication();
        let current_generation = self.generation.load(Ordering::SeqCst);
        if let Some((generation, tools)) = cached {
            if generation == current_generation {
                return Ok(tools);
            }
        }
        drop(publication);
        drop(cache);

        loop {
            let mut cache = self.cache.write().await;
            let cached = cache
                .as_ref()
                .map(|cached| (cached.generation, cached.tools.clone()));
            let publication = self.lock_publication();
            let current_generation = self.generation.load(Ordering::SeqCst);
            if let Some((generation, tools)) = cached {
                if generation == current_generation {
                    return Ok(tools);
                }
            }
            drop(publication);

            let sequence = self.next_fetch_sequence();
            let tools = self.fetch_filtered().await?;
            let snapshot = self.prepare_snapshot(&tools);
            let _publication = self.lock_publication();
            if self.generation.load(Ordering::SeqCst) != current_generation {
                continue;
            }
            self.publish(&mut cache, current_generation, sequence, snapshot);
            return Ok(tools);
        }
    }

    /// Return a single tool by name, or `None` if not found.
    pub async fn get(&self, name: &str) -> Result<Option<Tool>, Error> {
        Ok(self
            .get_all()
            .await?
            .into_iter()
            .find(|tool| tool.name == name))
    }

    /// Force a re-fetch from the backend, discarding the current cache.
    pub async fn refresh(&self) -> Result<(), Error> {
        loop {
            let generation = {
                let _publication = self.lock_publication();
                self.generation.load(Ordering::SeqCst)
            };
            let sequence = self.next_fetch_sequence();
            let tools = self.fetch_filtered().await?;
            let mut cache = self.cache.write().await;
            let _publication = self.lock_publication();
            if self.generation.load(Ordering::SeqCst) != generation {
                continue;
            }
            self.publish(&mut cache, generation, sequence, tools);
            return Ok(());
        }
    }

    /// Invalidate (clear) the cache without re-fetching.
    ///
    /// The next call to `get_all` or `get` will re-fetch from the backend.
    pub fn invalidate(&self) {
        let _publication = self.lock_publication();
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.populated.store(false, Ordering::SeqCst);
    }

    fn lock_publication(&self) -> MutexGuard<'_, ()> {
        self.publication_lock
            .lock()
            .expect("tool cache publication lock poisoned")
    }

    /// Take the ticket identifying a fetch about to start.
    fn next_fetch_sequence(&self) -> u64 {
        self.fetch_sequence.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn prepare_snapshot(&self, tools: &[Tool]) -> Vec<Tool> {
        #[cfg(test)]
        if let Some(hook) = &self.snapshot_hook {
            hook();
        }
        tools.to_vec()
    }

    /// Publish `tools` unless the cache already holds a snapshot from a fetch
    /// that started later.
    ///
    /// `refresh` and `get_all` can fetch concurrently within the same
    /// generation. Without this check the slower fetch wins purely on
    /// completion order, so an older tool list could replace a newer one and
    /// stay cached until the next invalidation.
    ///
    /// Callers must hold both the cache write lock and the publication lock.
    fn publish(
        &self,
        cache: &mut Option<CachedTools>,
        generation: u64,
        sequence: u64,
        tools: Vec<Tool>,
    ) {
        if let Some(cached) = cache.as_ref() {
            if cached.generation == generation && cached.sequence > sequence {
                self.populated.store(true, Ordering::SeqCst);
                return;
            }
        }
        #[cfg(test)]
        if let Some(hook) = &self.publication_hook {
            hook(generation, &tools);
        }
        *cache = Some(CachedTools {
            generation,
            sequence,
            tools,
        });
        self.populated.store(true, Ordering::SeqCst);
    }

    async fn fetch_filtered(&self) -> Result<Vec<Tool>, Error> {
        Ok(apply_filters(
            self.backend.list_tools().await?,
            self.include.as_deref(),
            self.exclude.as_deref(),
        ))
    }
}

fn apply_filters(
    tools: Vec<Tool>,
    include: Option<&[String]>,
    exclude: Option<&[String]>,
) -> Vec<Tool> {
    let include = include.map(|values| values.iter().collect::<HashSet<_>>());
    let exclude = exclude.map(|values| values.iter().collect::<HashSet<_>>());

    tools
        .into_iter()
        .filter(|tool| {
            include
                .as_ref()
                .is_none_or(|include| include.contains(&tool.name))
        })
        .filter(|tool| {
            exclude
                .as_ref()
                .is_none_or(|exclude| !exclude.contains(&tool.name))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::sync::Barrier;

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
            Self {
                tools,
                call_count: Arc::new(AtomicU32::new(0)),
            }
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

    #[derive(Clone)]
    struct BlockingBackend {
        stale_tools: Vec<Tool>,
        fresh_tools: Vec<Tool>,
        call_count: Arc<AtomicU32>,
        first_fetch_started: Arc<Barrier>,
        release_first_fetch: Arc<Barrier>,
    }

    impl BlockingBackend {
        fn new(
            stale_tools: Vec<Tool>,
            fresh_tools: Vec<Tool>,
        ) -> (Self, Arc<Barrier>, Arc<Barrier>, Arc<AtomicU32>) {
            let first_fetch_started = Arc::new(Barrier::new(2));
            let release_first_fetch = Arc::new(Barrier::new(2));
            let call_count = Arc::new(AtomicU32::new(0));
            (
                Self {
                    stale_tools,
                    fresh_tools,
                    call_count: Arc::clone(&call_count),
                    first_fetch_started: Arc::clone(&first_fetch_started),
                    release_first_fetch: Arc::clone(&release_first_fetch),
                },
                first_fetch_started,
                release_first_fetch,
                call_count,
            )
        }
    }

    impl ToolBackend for BlockingBackend {
        async fn list_tools(&self) -> Result<Vec<Tool>, Error> {
            let call = self.call_count.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                self.first_fetch_started.wait().await;
                self.release_first_fetch.wait().await;
                Ok(self.stale_tools.clone())
            } else {
                Ok(self.fresh_tools.clone())
            }
        }
    }

    #[derive(Clone)]
    struct FailingBackend;

    impl ToolBackend for FailingBackend {
        async fn list_tools(&self) -> Result<Vec<Tool>, Error> {
            Err(Error::Config("tool fetch failed".to_string()))
        }
    }

    // Helper: build a named tool with no description.
    fn make_tool(name: &str) -> Tool {
        Tool::new(
            name,
            None::<String>,
            json!({ "type": "object", "properties": {} }),
        )
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
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "backend called more than once"
        );
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
        cache.refresh().await.unwrap(); // forces re-fetch
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            2,
            "expected 2 backend calls after refresh"
        );
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

    #[tokio::test]
    async fn refresh_does_not_publish_tools_fetched_before_invalidation() {
        let (backend, first_fetch_started, release_first_fetch, call_count) =
            BlockingBackend::new(vec![make_tool("stale")], vec![make_tool("fresh")]);
        let publications = Arc::new(Mutex::new(Vec::new()));
        let recorded_publications = Arc::clone(&publications);
        let mut cache = ToolCache::new(backend, None, None);
        cache.publication_hook = Some(Arc::new(move |generation, tools| {
            recorded_publications
                .lock()
                .unwrap()
                .push((generation, tools[0].name.clone()));
        }));
        let cache = Arc::new(cache);
        let cache_for_refresh = Arc::clone(&cache);
        let refresh = tokio::spawn(async move { cache_for_refresh.refresh().await });

        first_fetch_started.wait().await;
        cache.invalidate();
        cache.invalidate();
        release_first_fetch.wait().await;
        refresh.await.unwrap().unwrap();

        let tools = cache.get_all().await.unwrap();
        assert_eq!(tools[0].name, "fresh");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        assert!(cache.is_populated());
        assert_eq!(*publications.lock().unwrap(), vec![(2, "fresh".into())]);
    }

    #[tokio::test]
    async fn get_all_does_not_return_tools_fetched_before_invalidation() {
        let (backend, first_fetch_started, release_first_fetch, call_count) =
            BlockingBackend::new(vec![make_tool("stale")], vec![make_tool("fresh")]);
        let publications = Arc::new(Mutex::new(Vec::new()));
        let recorded_publications = Arc::clone(&publications);
        let mut cache = ToolCache::new(backend, None, None);
        cache.publication_hook = Some(Arc::new(move |generation, tools| {
            recorded_publications
                .lock()
                .unwrap()
                .push((generation, tools[0].name.clone()));
        }));
        let cache = Arc::new(cache);
        let cache_for_get = Arc::clone(&cache);
        let get_all = tokio::spawn(async move { cache_for_get.get_all().await });

        first_fetch_started.wait().await;
        cache.invalidate();
        cache.invalidate();
        release_first_fetch.wait().await;
        let tools = get_all.await.unwrap().unwrap();

        assert_eq!(tools[0].name, "fresh");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        assert!(cache.is_populated());
        assert_eq!(*publications.lock().unwrap(), vec![(2, "fresh".into())]);
    }

    #[tokio::test]
    async fn slow_refresh_does_not_overwrite_a_newer_snapshot_in_the_same_generation() {
        let (backend, first_fetch_started, release_first_fetch, call_count) =
            BlockingBackend::new(vec![make_tool("stale")], vec![make_tool("fresh")]);
        let cache = Arc::new(ToolCache::new(backend, None, None));
        let cache_for_refresh = Arc::clone(&cache);
        // This refresh starts first, so its tool list is the older one.
        let refresh = tokio::spawn(async move { cache_for_refresh.refresh().await });

        first_fetch_started.wait().await;
        // A concurrent read starts later, completes first and publishes the
        // newer tool list without bumping the generation.
        let fresh = cache.get_all().await.unwrap();
        assert_eq!(fresh[0].name, "fresh");

        release_first_fetch.wait().await;
        refresh.await.unwrap().unwrap();

        let tools = cache.get_all().await.unwrap();
        assert_eq!(
            tools[0].name, "fresh",
            "a refresh that started earlier must not replace a newer snapshot"
        );
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        assert!(cache.is_populated());
    }
    #[tokio::test]
    async fn failed_refresh_leaves_invalidated_cache_unpopulated() {
        let cache = ToolCache::new(FailingBackend, None, None);
        cache.invalidate();

        let error = cache.refresh().await.unwrap_err();

        assert!(error.to_string().contains("tool fetch failed"));
        assert!(!cache.is_populated());
    }

    #[tokio::test]
    async fn concurrent_get_all_calls_share_one_fetch() {
        let (backend, first_fetch_started, release_first_fetch, call_count) =
            BlockingBackend::new(vec![make_tool("shared")], vec![make_tool("unused")]);
        let cache = Arc::new(ToolCache::new(backend, None, None));
        let first_cache = Arc::clone(&cache);
        let first = tokio::spawn(async move { first_cache.get_all().await });
        first_fetch_started.wait().await;

        let second = cache.get_all();
        tokio::pin!(second);
        assert!(futures::poll!(&mut second).is_pending());
        release_first_fetch.wait().await;

        let first_tools = first.await.unwrap().unwrap();
        let second_tools = second.await.unwrap();
        assert_eq!(first_tools[0].name, "shared");
        assert_eq!(second_tools[0].name, "shared");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn publish_moves_the_prepared_snapshot_without_cloning() {
        let tool_cache = ToolCache::new(MockBackend::new(vec![]), None, None);
        let tools = vec![make_tool("prepared")];
        let prepared_allocation = tools.as_ptr();
        let mut cache = None;

        let _publication = tool_cache.lock_publication();
        tool_cache.publish(&mut cache, 0, 1, tools);

        assert_eq!(cache.unwrap().tools.as_ptr(), prepared_allocation);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn snapshot_preparation_does_not_block_invalidation() {
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        use std::sync::Barrier as SyncBarrier;
        use std::time::Duration;

        let backend = MockBackend::new(vec![make_tool("prepared")]);
        let call_count = Arc::clone(&backend.call_count);
        let entered = Arc::new(SyncBarrier::new(2));
        let release = Arc::new(SyncBarrier::new(2));
        let invoked = Arc::new(AtomicBool::new(false));
        let hook_entered = Arc::clone(&entered);
        let hook_release = Arc::clone(&release);
        let hook_invoked = Arc::clone(&invoked);
        let mut cache = ToolCache::new(backend, None, None);
        cache.snapshot_hook = Some(Arc::new(move || {
            if !hook_invoked.swap(true, Ordering::SeqCst) {
                hook_entered.wait();
                hook_release.wait();
            }
        }));
        let cache = Arc::new(cache);
        let reader_cache = Arc::clone(&cache);
        let reader = tokio::spawn(async move { reader_cache.get_all().await });

        entered.wait();
        let invalidation_cache = Arc::clone(&cache);
        let (invalidated, invalidation_completed) = mpsc::channel();
        let invalidator = std::thread::spawn(move || {
            invalidation_cache.invalidate();
            invalidated.send(()).unwrap();
        });
        let completed_while_snapshot_was_blocked = invalidation_completed
            .recv_timeout(Duration::from_secs(2))
            .is_ok();
        release.wait();

        invalidator.join().unwrap();
        let tools = reader.await.unwrap().unwrap();
        assert!(completed_while_snapshot_was_blocked);
        assert_eq!(tools[0].name, "prepared");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    // ------------------------------------------------------------------
    // Include / exclude filters
    // ------------------------------------------------------------------

    /// An include filter keeps only the named tools.
    #[tokio::test]
    async fn include_filter_keeps_only_named_tools() {
        let backend = MockBackend::new(vec![
            make_tool("fetch"),
            make_tool("search"),
            make_tool("upload"),
        ]);
        let cache = ToolCache::new(backend, Some(vec!["fetch".into()]), None);
        let tools = cache.get_all().await.unwrap();
        assert_eq!(tools.len(), 1, "expected only 'fetch'");
        assert_eq!(tools[0].name, "fetch");
    }

    /// An exclude filter removes the named tools.
    #[tokio::test]
    async fn exclude_filter_removes_named_tools() {
        let backend = MockBackend::new(vec![
            make_tool("fetch"),
            make_tool("search"),
            make_tool("upload"),
        ]);
        let cache = ToolCache::new(backend, None, Some(vec!["search".into()]));
        let tools = cache.get_all().await.unwrap();
        assert_eq!(tools.len(), 2, "expected 'fetch' and 'upload'");
        assert!(tools.iter().all(|t| t.name != "search"));
    }

    /// When both include and exclude filters are specified, include is applied
    /// first, then exclude is applied to the included set.
    #[tokio::test]
    async fn include_then_exclude_applied_in_order() {
        let backend = MockBackend::new(vec![
            make_tool("fetch"),
            make_tool("search"),
            make_tool("upload"),
        ]);
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
        let cache = ToolCache::new(backend, None, Some(vec!["fetch".into(), "search".into()]));
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
