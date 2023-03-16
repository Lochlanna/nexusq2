use crate::wait_strategy;
use crate::wait_strategy::WaitStrategy;
use core::sync::atomic::{AtomicI64, Ordering};

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
        self.claimed.fetch_add(1, Ordering::Relaxed)
    }
    pub fn publish(&self, value: i64) {
        while self.published.load(Ordering::Acquire) != value - 1 {
            core::hint::spin_loop();
        }
        self.published.store(value, Ordering::Release);
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
        self.published.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod producer_tracker_tests {
    use super::*;
    use crate::test_shared::*;

    #[test]
    fn basic_test() {
        setup_logging();
        let producer_tracker = ProducerTracker::default();
        assert_eq!(producer_tracker.current_published(), -1);
        assert_eq!(producer_tracker.claim(), 0);
        producer_tracker.publish(0);
        assert_eq!(producer_tracker.current_published(), 0);
        producer_tracker.publish(1);
        assert_eq!(producer_tracker.current_published(), 1);
    }
}
