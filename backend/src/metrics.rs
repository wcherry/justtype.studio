//! The write path, measured, so the assumption the architecture rests on —
//! three or four writes a second, which one SQLite file absorbs without
//! noticing — is checked rather than believed.
//!
//! Three numbers:
//!
//! - **Writes per second**: requests that change something (POST, PUT, PATCH,
//!   DELETE) and the database calls they made.
//! - **Write latency**: how long each of those database calls took, waiting
//!   for a pooled connection included.
//! - **Lock wait**: how long a writer waits for SQLite's write lock. SQLite
//!   does that waiting inside its busy handler, which Diesel does not let us
//!   hook, so it is *sampled*: every few seconds a probe takes the write lock
//!   (`BEGIN IMMEDIATE`) and times how long that took, which is exactly the
//!   wait a real write would have had at that moment.
//!
//! Kept in memory for the last fifteen minutes. One process, like the rate
//! limiter; a second process would move this to a metrics store.
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

const KEPT: Duration = Duration::from_secs(15 * 60);
/// How often the lock probe runs.
pub const PROBE_EVERY: Duration = Duration::from_secs(5);

tokio::task_local! {
    /// Whether the request being served changes something. Set by the
    /// request middleware; read where database calls are timed.
    pub static WRITING: bool;
}

#[derive(Default)]
struct Series {
    /// (when, value in microseconds)
    points: VecDeque<(Instant, u64)>,
}

impl Series {
    fn push(&mut self, value: u64) {
        let now = Instant::now();
        self.points.push_back((now, value));
        while self
            .points
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > KEPT)
        {
            self.points.pop_front();
        }
    }

    fn within(&self, window: Duration) -> Vec<u64> {
        let now = Instant::now();
        self.points
            .iter()
            .filter(|(t, _)| now.duration_since(*t) <= window)
            .map(|(_, v)| *v)
            .collect()
    }
}

#[derive(Default)]
pub struct Metrics {
    write_requests: Mutex<Series>,
    db_writes: Mutex<Series>,
    lock_waits: Mutex<Series>,
    busy: std::sync::atomic::AtomicU64,
}

pub fn global() -> &'static Metrics {
    static METRICS: OnceLock<Metrics> = OnceLock::new();
    METRICS.get_or_init(Metrics::default)
}

/// Whether the current task is serving a write. False outside a request.
pub fn writing() -> bool {
    WRITING.try_with(|w| *w).unwrap_or(false)
}

impl Metrics {
    pub fn record_write_request(&self, elapsed: Duration) {
        lock(&self.write_requests).push(elapsed.as_micros() as u64);
    }

    pub fn record_db_write(&self, elapsed: Duration) {
        lock(&self.db_writes).push(elapsed.as_micros() as u64);
    }

    pub fn record_lock_wait(&self, elapsed: Duration) {
        lock(&self.lock_waits).push(elapsed.as_micros() as u64);
    }

    /// A write that gave up: SQLite's busy timeout ran out.
    pub fn record_busy(&self) {
        self.busy.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn summary(&self, window: Duration) -> WindowSummary {
        let seconds = window.as_secs_f64().max(1.0);
        let requests = lock(&self.write_requests).within(window);
        let writes = lock(&self.db_writes).within(window);
        let waits = lock(&self.lock_waits).within(window);
        WindowSummary {
            window_seconds: window.as_secs(),
            write_requests_per_second: round(requests.len() as f64 / seconds),
            db_writes_per_second: round(writes.len() as f64 / seconds),
            write_ms: Percentiles::of(&writes),
            lock_wait_ms: Percentiles::of(&waits),
            lock_samples: waits.len() as u64,
        }
    }

    pub fn busy_total(&self) -> u64 {
        self.busy.load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn lock(series: &Mutex<Series>) -> std::sync::MutexGuard<'_, Series> {
    series.lock().unwrap_or_else(|e| e.into_inner())
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowSummary {
    pub window_seconds: u64,
    /// Requests that changed something.
    pub write_requests_per_second: f64,
    /// Database calls made while serving them.
    pub db_writes_per_second: f64,
    pub write_ms: Percentiles,
    /// Sampled: how long the write lock took to get, every few seconds.
    pub lock_wait_ms: Percentiles,
    pub lock_samples: u64,
}

#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl Percentiles {
    fn of(micros: &[u64]) -> Self {
        if micros.is_empty() {
            return Self::default();
        }
        let mut sorted = micros.to_vec();
        sorted.sort_unstable();
        let at = |q: f64| {
            let i = ((sorted.len() as f64 - 1.0) * q).round() as usize;
            round(sorted[i] as f64 / 1000.0)
        };
        Self {
            p50: at(0.50),
            p95: at(0.95),
            p99: at(0.99),
            max: at(1.0),
        }
    }
}

/// Takes the write lock on its own connection, every few seconds, and times
/// how long that took. Runs for the life of the process.
pub fn spawn_lock_probe(pool: crate::db::Pool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PROBE_EVERY);
        loop {
            ticker.tick().await;
            let pool = pool.clone();
            let sampled = tokio::task::spawn_blocking(move || -> anyhow::Result<Duration> {
                use diesel::connection::SimpleConnection;
                let mut conn = pool.get()?;
                let started = Instant::now();
                conn.batch_execute("BEGIN IMMEDIATE")?;
                let waited = started.elapsed();
                conn.batch_execute("ROLLBACK")?;
                Ok(waited)
            })
            .await;
            match sampled {
                Ok(Ok(waited)) => global().record_lock_wait(waited),
                Ok(Err(e)) => {
                    // The probe itself ran out of patience: as real a busy
                    // event as a writer's.
                    if e.to_string().contains("locked") {
                        global().record_busy();
                    }
                    tracing::warn!(error = %e, "lock probe failed");
                }
                Err(e) => tracing::warn!(error = %e, "lock probe panicked"),
            }
        }
    });
}

/// Prometheus text format, for a real dashboard to scrape.
pub fn prometheus() -> String {
    let m = global();
    let mut out = String::new();
    for (label, window) in [("1m", Duration::from_secs(60)), ("15m", KEPT)] {
        let s = m.summary(window);
        out.push_str(&format!(
            "justtype_write_requests_per_second{{window=\"{label}\"}} {}\n",
            s.write_requests_per_second
        ));
        out.push_str(&format!(
            "justtype_db_writes_per_second{{window=\"{label}\"}} {}\n",
            s.db_writes_per_second
        ));
        for (q, v) in [
            ("0.5", s.write_ms.p50),
            ("0.95", s.write_ms.p95),
            ("0.99", s.write_ms.p99),
        ] {
            out.push_str(&format!(
                "justtype_db_write_ms{{window=\"{label}\",quantile=\"{q}\"}} {v}\n"
            ));
        }
        for (q, v) in [
            ("0.5", s.lock_wait_ms.p50),
            ("0.95", s.lock_wait_ms.p95),
            ("0.99", s.lock_wait_ms.p99),
        ] {
            out.push_str(&format!(
                "justtype_lock_wait_ms{{window=\"{label}\",quantile=\"{q}\"}} {v}\n"
            ));
        }
    }
    out.push_str(&format!("justtype_busy_total {}\n", m.busy_total()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_read_off_the_sorted_samples() {
        let micros: Vec<u64> = (1..=100).map(|ms| ms * 1000).collect();
        let p = Percentiles::of(&micros);
        assert_eq!((p.p50, p.p95, p.p99, p.max), (51.0, 95.0, 99.0, 100.0));
        assert_eq!(Percentiles::of(&[]).max, 0.0);
    }

    #[test]
    fn a_window_counts_only_what_fell_inside_it() {
        let m = Metrics::default();
        for _ in 0..30 {
            m.record_write_request(Duration::from_millis(3));
            m.record_db_write(Duration::from_millis(2));
        }
        let s = m.summary(Duration::from_secs(60));
        assert_eq!(s.write_requests_per_second, 0.5);
        assert_eq!(s.write_ms.p50, 2.0);
    }
}
