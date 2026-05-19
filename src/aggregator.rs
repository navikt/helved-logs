use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::model::{AlertView, Log};
use crate::slack::{PostedMessage, Slack};

pub struct Aggregate {
    container: String,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    count: u32,
    sample: Log,
    pods: HashSet<String>,
    trace_ids: HashSet<String>,
    posted: Option<PostedMessage>,
    last_edit: Option<Instant>,
    dirty: bool,
}

pub struct Aggregator {
    map: Mutex<HashMap<String, Aggregate>>,
    slack: Arc<Slack>,
    window: ChronoDuration,
    edit_throttle: StdDuration,
}

impl Aggregator {
    pub fn new(slack: Arc<Slack>, window_seconds: i64, edit_throttle_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            map: Mutex::new(HashMap::new()),
            slack,
            window: ChronoDuration::seconds(window_seconds),
            edit_throttle: StdDuration::from_millis(edit_throttle_ms),
        })
    }

    /// Spawn the periodic flush task. Tick edits throttled aggregates and evicts cold ones.
    pub fn spawn_flush(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(StdDuration::from_secs(1));
            loop {
                ticker.tick().await;
                self.flush_tick().await;
            }
        })
    }

    pub async fn ingest(&self, log: Log, container: String, pod: String) {
        let key = log.aggregation_key(&container);
        let now = Utc::now();
        let event_ts = log.parsed_timestamp().unwrap_or(now);
        let trace = log.trace_id().map(|s| s.to_string());

        // Decide path under lock; do slack IO afterwards (or via flush task).
        let mut map = self.map.lock().await;

        if let Some(agg) = map.get_mut(&key) {
            if now.signed_duration_since(agg.last_seen) < self.window {
                agg.count += 1;
                agg.last_seen = agg.last_seen.max(event_ts).max(now);
                agg.pods.insert(pod);
                if let Some(t) = trace {
                    agg.trace_ids.insert(t);
                }
                agg.sample = log;
                agg.dirty = true;
                return;
            }
            // Stale: evict and fall through to fresh post.
            map.remove(&key);
        }

        // Fresh aggregate. Insert placeholder first so concurrent ingests merge.
        let mut trace_ids = HashSet::new();
        if let Some(t) = trace {
            trace_ids.insert(t);
        }
        let mut pods = HashSet::new();
        pods.insert(pod);

        let agg = Aggregate {
            container: container.clone(),
            first_seen: event_ts.min(now),
            last_seen: event_ts.max(now),
            count: 1,
            sample: log,
            pods,
            trace_ids,
            posted: None,
            last_edit: None,
            dirty: false,
        };
        map.insert(key.clone(), agg);

        // Build view + post while still holding the lock so we don't double-post for the
        // same key on bursts. Volume is low so this is acceptable.
        let view = {
            let agg = map.get(&key).expect("just inserted");
            build_view(agg)
        };
        let blocks = view.to_blocks();
        let fallback = view.fallback_text();
        drop(map);

        match self.slack.post(blocks, &fallback).await {
            Ok(posted) => {
                let mut map = self.map.lock().await;
                if let Some(agg) = map.get_mut(&key) {
                    agg.posted = Some(posted);
                    agg.last_edit = Some(Instant::now());
                }
            }
            Err(e) => {
                log::error!("slack post failed for key {}: {}", key, e);
                // Drop the aggregate so the next event tries again.
                let mut map = self.map.lock().await;
                map.remove(&key);
            }
        }
    }

    async fn flush_tick(&self) {
        let now = Utc::now();
        let now_inst = Instant::now();

        // Collect work without holding lock across slack calls.
        let mut to_update: Vec<(String, serde_json::Value, String, PostedMessage)> = Vec::new();
        let mut to_evict: Vec<String> = Vec::new();

        {
            let map = self.map.lock().await;
            for (key, agg) in map.iter() {
                let cold = now.signed_duration_since(agg.last_seen) > self.window;
                if cold {
                    to_evict.push(key.clone());
                    continue;
                }
                if !agg.dirty {
                    continue;
                }
                let throttled = agg
                    .last_edit
                    .map(|t| now_inst.saturating_duration_since(t) < self.edit_throttle)
                    .unwrap_or(false);
                if throttled {
                    continue;
                }
                let Some(posted) = &agg.posted else {
                    continue;
                };
                let view = build_view(agg);
                to_update.push((key.clone(), view.to_blocks(), view.fallback_text(), posted.clone()));
            }
        }

        for (key, blocks, fallback, posted) in to_update {
            match self.slack.update(&posted, blocks, &fallback).await {
                Ok(()) => {
                    let mut map = self.map.lock().await;
                    if let Some(agg) = map.get_mut(&key) {
                        agg.dirty = false;
                        agg.last_edit = Some(Instant::now());
                    }
                }
                Err(e) => {
                    log::warn!("slack update failed for key {}: {}", key, e);
                    // Back off; leave dirty so we retry next tick (after throttle).
                    let mut map = self.map.lock().await;
                    if let Some(agg) = map.get_mut(&key) {
                        agg.last_edit = Some(Instant::now());
                    }
                }
            }
        }

        if !to_evict.is_empty() {
            let mut map = self.map.lock().await;
            for key in to_evict {
                if let Some(agg) = map.get(&key) {
                    if now.signed_duration_since(agg.last_seen) > self.window {
                        log::info!(
                            "evicting cold aggregate {} (count={}, container={})",
                            key,
                            agg.count,
                            agg.container
                        );
                        map.remove(&key);
                    }
                }
            }
        }
    }
}

fn build_view(agg: &Aggregate) -> AlertView<'_> {
    AlertView {
        sample: &agg.sample,
        container: &agg.container,
        count: agg.count,
        first_seen: agg.first_seen,
        last_seen: agg.last_seen,
        pods: &agg.pods,
        trace_ids: &agg.trace_ids,
    }
}
