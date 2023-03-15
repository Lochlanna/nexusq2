use crate::sync::atomic::{AtomicI64, Ordering};
use crate::wait_strategy;
use crate::wait_strategy::WaitStrategy;

#[derive(Debug)]
pub struct ProducerTracker {
    claimed: AtomicI64,
    published: AtomicI64,
    wait_strategy: wait_strategy::HybridWaitStrategy,
}

impl Default for ProducerTracker {
    fn default() -> Self {
        Self {
            claimed: AtomicI64::new(0),
            published: AtomicI64::new(-1),
            wait_strategy: Default::default(),
        }
    }
}

impl ProducerTracker {
    pub fn claim(&self) -> i64 {
        self.claimed.fetch_add(1, Ordering::SeqCst)
    }
    pub fn publish(&self, value: i64) {
        while self
            .published
            .compare_exchange_weak(value - 1, value, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            crate::hint::spin_loop();
        }
        self.wait_strategy.notify()
    }
    pub fn wait_for_publish(&self, min_published_value: i64) -> i64 {
        let v = self
            .wait_strategy
            .wait_for_at_least(&self.published, min_published_value);
        debug_assert!(self.published.load(Ordering::Relaxed) >= min_published_value);
        v
    }
    pub fn current_published(&self) -> i64 {
        self.published.load(Ordering::SeqCst)
    }
}
