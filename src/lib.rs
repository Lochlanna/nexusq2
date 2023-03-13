#![allow(dead_code)]

mod wait_strategy;

use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

#[derive(Debug)]
struct ProducerTracker {
    claimed: AtomicI64,
    published: AtomicI64,
}

impl Default for ProducerTracker {
    fn default() -> Self {
        Self {
            claimed: Default::default(),
            published: AtomicI64::new(-1),
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
            core::hint::spin_loop();
        }
    }
}

#[derive(Debug)]
struct ReaderTracker {
    tokens: Vec<AtomicUsize>,
    tail: AtomicI64,
}

#[derive(Debug)]
pub struct NexusQ<T> {
    buffer: *mut Vec<T>,
    producer_tracker: ProducerTracker,
    reader_tracker: ReaderTracker,
}

#[cfg(test)]
mod tests {}
