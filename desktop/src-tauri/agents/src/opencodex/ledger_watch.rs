//! The "ledger grew" signal for the open statistics views (spec "The data contract").
//!
//! Only the ledger's size and modification time are looked at, once per
//! [`LedgerTiming::look_every`], and only while a statistics view is open: each of the
//! views' requests renews a lease, and the open views ask at least once per lease
//! (`web/src/agents/use-statistics.ts`). When the lease runs out the looking thread
//! ends; the next request starts it again. So with no statistics view open, nothing
//! polls and nothing is signalled.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::LEDGER;

/// How often the ledger is looked at, and how long one request keeps it watched.
#[derive(Clone, Copy, Debug)]
pub struct LedgerTiming {
    pub look_every: Duration,
    pub lease: Duration,
}

impl Default for LedgerTiming {
    fn default() -> Self {
        LedgerTiming {
            look_every: Duration::from_secs(1),
            lease: Duration::from_secs(60),
        }
    }
}

/// Watches `usage.jsonl` for growth while statistics views keep asking.
pub struct LedgerWatch {
    shared: Arc<Shared>,
}

struct Shared {
    ledger: PathBuf,
    timing: LedgerTiming,
    sink: Box<dyn Fn() + Send + Sync>,
    /// Until when the ledger is watched, and whether the looking thread runs. One
    /// lock, so a renewal never races the thread's decision to end.
    lease: Mutex<Lease>,
}

#[derive(Default)]
struct Lease {
    until: Option<Instant>,
    running: bool,
}

impl LedgerWatch {
    /// A watch over the ledger in `opencodex_dir` that calls `sink` when it grows.
    /// Making it reads nothing and starts nothing.
    pub fn new(
        opencodex_dir: &Path,
        timing: LedgerTiming,
        sink: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        LedgerWatch {
            shared: Arc::new(Shared {
                ledger: opencodex_dir.join(LEDGER),
                timing,
                sink: Box::new(sink),
                lease: Mutex::new(Lease::default()),
            }),
        }
    }

    /// A statistics view asked: keep watching for another lease, starting to look
    /// if nothing is looking now.
    pub fn renew(&self) {
        let mut lease = self.shared.lease.lock().unwrap();
        lease.until = Some(Instant::now() + self.shared.timing.lease);
        if lease.running {
            return;
        }
        let shared = self.shared.clone();
        let spawned = std::thread::Builder::new()
            .name("opencodex-ledger-watch".into())
            .spawn(move || shared.look());
        match spawned {
            Ok(_) => lease.running = true,
            Err(error) => eprintln!("opencodex: could not start watching the ledger: {error}"),
        }
    }
}

impl Shared {
    fn look(&self) {
        let mut last = mark(&self.ledger);
        loop {
            std::thread::sleep(self.timing.look_every);
            {
                let mut lease = self.lease.lock().unwrap();
                if lease.until.is_none_or(|until| Instant::now() >= until) {
                    lease.running = false;
                    return;
                }
            }
            let now = mark(&self.ledger);
            if now != last {
                last = now;
                (self.sink)();
            }
        }
    }
}

/// The ledger's size and modification time, or nothing when it is not there.
fn mark(ledger: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = std::fs::metadata(ledger).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}
