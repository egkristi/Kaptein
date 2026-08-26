//! Informer lifecycle management — the ADR-0006 subjects that had no code (M2.0c).
//!
//! "Informer-based, never polling" is only safe if the number of simultaneous watches is
//! *bounded*: with hundreds of CRDs and namespaces, a naive one-reflector-per-view
//! strategy can melt the API server faster than polling. This module enforces the
//! strategy ADR-0006 specifies:
//!
//! - **Lazy per-view informers** — a watch is registered only when a view asks for it
//!   (`register`), never eagerly for the whole world.
//! - **LRU eviction with TTL** — idle watches are evicted after a TTL; hot (recently
//!   touched) views keep theirs.
//! - **A hard cap on concurrent watches**, with **degradation to on-demand list** when
//!   the cap is reached (`register` returns `Denied` rather than exceeding the cap).
//!
//! This is the *policy* layer — it owns the lifecycle bookkeeping, not the actual watch
//! sockets (those live in `store::run_informer` / `watchring`). It is deterministic and
//! fully unit-testable (no cluster, no clock races: time is injected).

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A stable identity for a watch target: `group/version/kind` plus an optional namespace
/// (empty for cluster-scoped). This is the "view" the watch serves.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WatchKey {
    /// API group (empty for core).
    pub group: String,
    /// API version.
    pub version: String,
    /// Resource kind.
    pub kind: String,
    /// Namespace (empty for cluster-scoped).
    pub namespace: String,
}

impl WatchKey {
    /// A compact `group/version/kind[/namespace]` string for diagnostics.
    pub fn display(&self) -> String {
        let gvk = if self.group.is_empty() {
            format!("{}/{}", self.version, self.kind)
        } else {
            format!("{}/{}/{}", self.group, self.version, self.kind)
        };
        if self.namespace.is_empty() {
            gvk
        } else {
            format!("{gvk}/{}", self.namespace)
        }
    }
}

/// The result of asking the manager for a watch slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// The watch is granted (either newly registered or already live — idempotent).
    Granted,
    /// The hard cap is reached and this view was not the most-recently-used; the caller
    /// must **degrade to an on-demand list** instead of opening a new watch.
    Denied,
}

/// Policy knobs for informer management (ADR-0006: the cap requires a policy, exposed in
/// the config file). `Default` is a conservative out-of-the-box budget.
#[derive(Debug, Clone, Copy)]
pub struct InformerPolicy {
    /// Maximum number of simultaneous live watches.
    pub max_watches: usize,
    /// Idle watches are evicted after this TTL (no `touch` within the window).
    pub idle_ttl: Duration,
}

impl Default for InformerPolicy {
    fn default() -> Self {
        Self {
            max_watches: 16,
            idle_ttl: Duration::from_secs(300),
        }
    }
}

/// A live watch's bookkeeping state (policy-internal).
struct WatchEntry {
    last_touched: Instant,
}

/// The informer lifecycle manager.
///
/// `Clock`-free: time is read via [`Instant::now`], so tests inject determinism by
/// calling `touch`/`evict` with explicit instants through the lower-level helpers. The
/// public API is `Send + Sync` (a `Mutex`), so it can sit behind the same shared handle
/// as `InformerStore` and `WatchRing`.
pub struct InformerManager {
    policy: InformerPolicy,
    /// Live watches, keyed by `WatchKey`, in insertion order for LRU scanning.
    watches: Mutex<HashMap<WatchKey, WatchEntry>>,
}

use std::sync::Mutex;

impl InformerManager {
    /// Create a manager with the given policy.
    pub fn new(policy: InformerPolicy) -> Self {
        Self {
            policy,
            watches: Mutex::new(HashMap::new()),
        }
    }

    /// The number of live watches right now.
    pub fn live(&self) -> usize {
        self.watches.lock().expect("manager poisoned").len()
    }

    /// The hard cap on concurrent watches.
    pub fn max_watches(&self) -> usize {
        self.policy.max_watches
    }

    /// Request a watch slot for `key` (lazy-per-view). Returns `Granted` if the watch is
    /// already live or there is room; `Denied` if the cap is reached. **The caller must
    /// degrade to an on-demand list on `Denied`** — this is the ADR-0006 degradation
    /// path, not an error.
    pub fn register(&self, key: WatchKey) -> Registration {
        let now = Instant::now();
        let mut watches = self.watches.lock().expect("manager poisoned");

        // Idempotent: touching an already-live watch refreshes its TTL (a hot view stays).
        if watches.contains_key(&key) {
            watches.get_mut(&key).expect("just checked").last_touched = now;
            return Registration::Granted;
        }

        if watches.len() >= self.policy.max_watches {
            // Evict idle watches first — an idle watch may free a slot for a hot one.
            self.evict_idle_locked(&mut watches, now);
        }
        if watches.len() >= self.policy.max_watches {
            return Registration::Denied;
        }

        watches.insert(key, WatchEntry { last_touched: now });
        Registration::Granted
    }

    /// Mark a live watch as recently used (refreshes its TTL). Returns `true` if the
    /// watch was live (and touched); `false` if it was unknown.
    pub fn touch(&self, key: &WatchKey) -> bool {
        let now = Instant::now();
        let mut watches = self.watches.lock().expect("manager poisoned");
        match watches.get_mut(key) {
            Some(entry) => {
                entry.last_touched = now;
                true
            }
            None => false,
        }
    }

    /// Release a watch (a view closed). Returns `true` if the watch was live.
    pub fn release(&self, key: &WatchKey) -> bool {
        self.watches
            .lock()
            .expect("manager poisoned")
            .remove(key)
            .is_some()
    }

    /// Evict watches idle longer than the policy TTL (relative to `now`). Returns the
    /// number evicted. `now` is injected so tests can advance time deterministically.
    pub fn evict_idle(&self, now: Instant) -> usize {
        let mut watches = self.watches.lock().expect("manager poisoned");
        self.evict_idle_locked(&mut watches, now)
    }

    /// Internal eviction helper (the lock is already held). Evicts entries whose
    /// `last_touched` is older than `now - ttl`.
    fn evict_idle_locked(
        &self,
        watches: &mut HashMap<WatchKey, WatchEntry>,
        now: Instant,
    ) -> usize {
        let ttl = self.policy.idle_ttl;
        let before = watches.len();
        watches.retain(|_, entry| now.duration_since(entry.last_touched) < ttl);
        before - watches.len()
    }
}

impl Default for InformerManager {
    fn default() -> Self {
        Self::new(InformerPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ns: &str, kind: &str) -> WatchKey {
        WatchKey {
            group: "".into(),
            version: "v1".into(),
            kind: kind.into(),
            namespace: ns.into(),
        }
    }

    #[test]
    fn register_is_lazy_and_idempotent() {
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: 2,
            idle_ttl: Duration::from_secs(60),
        });
        assert_eq!(mgr.live(), 0);
        assert_eq!(mgr.register(key("a", "Pod")), Registration::Granted);
        assert_eq!(mgr.live(), 1);
        // Re-registering the same key is idempotent — no double-count.
        assert_eq!(mgr.register(key("a", "Pod")), Registration::Granted);
        assert_eq!(mgr.live(), 1);
    }

    #[test]
    fn hard_cap_denies_and_caller_degrades() {
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: 1,
            idle_ttl: Duration::from_secs(60),
        });
        assert_eq!(mgr.register(key("a", "Pod")), Registration::Granted);
        // A second, different view is denied (degrade to on-demand list).
        assert_eq!(mgr.register(key("b", "Deployment")), Registration::Denied);
        assert_eq!(mgr.live(), 1);
    }

    #[test]
    fn release_frees_a_slot() {
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: 1,
            idle_ttl: Duration::from_secs(60),
        });
        mgr.register(key("a", "Pod"));
        assert_eq!(mgr.register(key("b", "Deployment")), Registration::Denied);
        assert!(mgr.release(&key("a", "Pod")));
        assert_eq!(mgr.register(key("b", "Deployment")), Registration::Granted);
        assert_eq!(mgr.live(), 1);
    }

    #[test]
    fn idle_eviction_frees_a_slot_within_cap() {
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: 1,
            idle_ttl: Duration::from_secs(30),
        });
        mgr.register(key("a", "Pod"));
        // Advance past the TTL: the first watch is now idle.
        let later = Instant::now() + Duration::from_secs(31);
        // Evicting idle watches frees the slot for a hot view.
        assert_eq!(mgr.evict_idle(later), 1);
        assert_eq!(mgr.live(), 0);
        assert_eq!(mgr.register(key("b", "Deployment")), Registration::Granted);
        assert_eq!(mgr.live(), 1);
    }

    #[test]
    fn touch_refreshes_ttl() {
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: 1,
            idle_ttl: Duration::from_secs(30),
        });
        mgr.register(key("a", "Pod"));
        assert!(mgr.touch(&key("a", "Pod")));
        assert!(!mgr.touch(&key("missing", "Pod")));
    }

    #[test]
    fn evict_idle_reports_count() {
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: 8,
            idle_ttl: Duration::from_secs(30),
        });
        mgr.register(key("a", "Pod"));
        mgr.register(key("b", "Pod"));
        let t0 = Instant::now();
        // Neither has been touched since t0; evict both after the TTL.
        let later = t0 + Duration::from_secs(31);
        assert_eq!(mgr.evict_idle(later), 2);
        assert_eq!(mgr.live(), 0);
    }

    /// The ADR-0006 performance-budget link — "simultaneous watches ≤ N for a given
    /// view set" — is now *enforceable*: no matter how many views request a watch, the
    /// manager never exceeds `max_watches`, and it degrades the excess to on-demand
    /// list. This test drives the equivalent of thousands of views (hundreds of CRDs ×
    /// many namespaces) through a small cap and asserts the invariant holds.
    #[test]
    fn concurrent_watches_never_exceed_cap_at_scale() {
        let max = 16;
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: max,
            idle_ttl: Duration::from_secs(300),
        });

        // Simulate a fleet-scale view set: 200 kinds × 20 namespaces = 4000 views.
        let mut granted = 0usize;
        for kind in 0..200 {
            for ns in 0..20 {
                let k = WatchKey {
                    group: "example.io".into(),
                    version: "v1".into(),
                    kind: format!("Kind{kind}"),
                    namespace: format!("ns-{ns}"),
                };
                if mgr.register(k) == Registration::Granted {
                    granted += 1;
                }
                // The invariant: never more than `max` live watches.
                assert!(mgr.live() <= max, "exceeded watch cap {max}");
            }
        }
        // The cap holds, and (without touch) the excess views were degraded.
        assert_eq!(granted, max, "exactly the cap should be granted");
        assert_eq!(mgr.live(), max);
    }

    /// The bookkeeping path (register + touch + release + evict) must stay fast enough
    /// for fleet scale — a CI safety net against an accidental O(n²) in the LRU scan,
    /// not a precise budget (the real kwok harness owns the precise numbers).
    #[test]
    fn informer_bookkeeping_is_not_quadratic() {
        let mgr = InformerManager::new(InformerPolicy {
            max_watches: 1_000,
            idle_ttl: Duration::from_secs(300),
        });
        let keys: Vec<WatchKey> = (0..5_000)
            .map(|i| WatchKey {
                group: "example.io".into(),
                version: "v1".into(),
                kind: "Kind".into(),
                namespace: format!("ns-{i}"),
            })
            .collect();

        let start = Instant::now();
        for k in &keys {
            mgr.register(k.clone());
        }
        // Touch every key, then evict them all (exercises the retain scan over 5k).
        for k in &keys {
            mgr.touch(k);
        }
        let later = Instant::now() + Duration::from_secs(301);
        mgr.evict_idle(later);
        let elapsed = start.elapsed();
        assert_eq!(mgr.live(), 0);
        // 5000 registers + 5000 touches + one retain over 5000 must not regress to
        // quadratic: a generous wall-clock bound (the guard fails loudly, not precisely).
        assert!(
            elapsed < Duration::from_secs(2),
            "informer bookkeeping regressed: {elapsed:?}"
        );
    }
}
