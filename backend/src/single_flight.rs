//! Work done once per key at a time: callers asking for a key whose work is running wait for it
//! and share its result. The entry goes once the work is done, so a later call works again (a
//! persistent cache in front of the work, checked inside it, makes that later call cheap).

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::OnceCell;

pub struct SingleFlight<K, V> {
    running: parking_lot::Mutex<HashMap<K, Arc<OnceCell<Arc<V>>>>>,
    /// Calls that ran their work.
    runs: AtomicUsize,
    /// Calls that got the result of work another call ran.
    shared: AtomicUsize,
}

impl<K, V> Default for SingleFlight<K, V> {
    fn default() -> Self {
        SingleFlight {
            running: parking_lot::Mutex::new(HashMap::new()),
            runs: AtomicUsize::new(0),
            shared: AtomicUsize::new(0),
        }
    }
}

impl<K: Hash + Eq + Clone, V> SingleFlight<K, V> {
    /// `work`'s result for `key`, or that of the same key's work already running. Blocks the
    /// calling thread while it waits, so call it from a blocking task or a thread of its own.
    pub fn run(&self, key: K, work: impl FnOnce() -> V) -> Arc<V> {
        futures::executor::block_on(self.run_async(key, || std::future::ready(work())))
    }

    /// [`SingleFlight::run`] for async work, waiting without blocking the thread. When the call
    /// running the work is dropped before it finishes, a waiting call runs its own work instead.
    pub async fn run_async<F: Future<Output = V>>(&self, key: K, work: impl FnOnce() -> F) -> Arc<V> {
        let cell = self.running.lock().entry(key.clone()).or_default().clone();
        let mut ran = false;
        let out = cell
            .get_or_init(|| {
                ran = true;
                async { Arc::new(work().await) }
            })
            .await
            .clone();
        if ran {
            self.runs.fetch_add(1, Ordering::Relaxed);
        } else {
            self.shared.fetch_add(1, Ordering::Relaxed);
        }
        {
            let mut running = self.running.lock();
            if running.get(&key).is_some_and(|c| Arc::ptr_eq(c, &cell)) {
                running.remove(&key);
            }
        }
        out
    }

    /// How many calls so far ran their work.
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::Relaxed)
    }

    /// How many calls so far got the result of work another call ran.
    pub fn shared(&self) -> usize {
        self.shared.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn work_for_a_key_already_running_is_waited_for_and_shared() {
        let flight: SingleFlight<u32, u32> = SingleFlight::default();
        let work = |v: u32| {
            std::thread::sleep(Duration::from_millis(300));
            v
        };
        let start = Instant::now();
        let (a, b, c) = std::thread::scope(|s| {
            let a = s.spawn(|| flight.run(1, || work(10)));
            let b = s.spawn(|| flight.run(1, || work(20)));
            let c = s.spawn(|| flight.run(2, || work(30)));
            (a.join().unwrap(), b.join().unwrap(), c.join().unwrap())
        });
        // One run for key 1, shared by both callers; key 2 alongside it.
        assert!(Arc::ptr_eq(&a, &b), "{a} {b}");
        assert_eq!((*c, flight.runs(), flight.shared()), (30, 2, 1));
        assert!(start.elapsed() < Duration::from_millis(550), "{:?}", start.elapsed());
        // Once done, the key runs again.
        assert_eq!(*flight.run(1, || work(40)), 40);
        assert_eq!(flight.runs(), 3);
    }

    #[tokio::test]
    async fn async_work_is_shared_without_blocking_the_runtime() {
        // A current-thread runtime: a waiter that blocked the thread would stall the runner.
        let flight: SingleFlight<u32, u32> = SingleFlight::default();
        let work = |v: u32| async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            v
        };
        let (a, b) = tokio::join!(flight.run_async(1, || work(1)), flight.run_async(1, || work(2)));
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!((flight.runs(), flight.shared()), (1, 1));
    }

    #[tokio::test]
    async fn a_waiter_runs_the_work_when_the_runner_is_dropped() {
        let flight: SingleFlight<u32, u32> = SingleFlight::default();
        let runner = flight.run_async(1, || async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            1
        });
        // The runner starts, then is dropped part way.
        let _ = tokio::time::timeout(Duration::from_millis(20), runner).await;
        assert_eq!(*flight.run_async(1, || async { 2 }).await, 2);
        assert!(flight.running.lock().is_empty());
    }
}
