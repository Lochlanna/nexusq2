use crate::wait_strategy::Hybrid;
use core::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug)]
pub struct ProducerTracker {
    claimed: AtomicI64,
    published: AtomicI64,
    wait_strategy: Hybrid,
}

impl Default for ProducerTracker {
    fn default() -> Self {
        Self {
            claimed: AtomicI64::new(0),
            published: AtomicI64::new(-1),
            wait_strategy: Hybrid::new(100, 0, -1),
        }
    }
}

impl ProducerTracker {
    pub fn claim(&self) -> i64 {
        self.claimed.fetch_add(1, Ordering::Relaxed)
    }
    pub fn publish(&self, value: i64) {
        debug_assert_eq!(self.published.load(Ordering::Acquire), value - 1);
        self.published.store(value, Ordering::Release);
    }
    pub fn current_published(&self) -> i64 {
        self.published.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod producer_tracker_tests {
    use super::*;
    use pretty_assertions_sorted::assert_eq;

    #[test]
    fn basic_test() {
        let producer_tracker = ProducerTracker::default();
        assert_eq!(producer_tracker.current_published(), -1);
        assert_eq!(producer_tracker.claim(), 0);
        producer_tracker.publish(0);
        assert_eq!(producer_tracker.current_published(), 0);
        producer_tracker.publish(1);
        assert_eq!(producer_tracker.current_published(), 1);
    }
}
