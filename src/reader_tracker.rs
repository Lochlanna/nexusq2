use crate::wait_strategy;
use crate::wait_strategy::WaitStrategy;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

#[derive(Debug)]
pub struct ReaderTracker {
    tokens: Vec<AtomicUsize>,
    tail: AtomicI64,
    wait_strategy: wait_strategy::HybridWaitStrategy,
}

impl ReaderTracker {
    pub fn new(size: usize) -> Self {
        let mut tokens = Vec::with_capacity(size);
        tokens.resize_with(size, Default::default);
        Self {
            tokens,
            tail: Default::default(),
            wait_strategy: Default::default(),
        }
    }

    pub fn register(&self) {
        let token = self.tokens.get(0).expect("tokens was 0 sized");
        token.fetch_add(1, Ordering::SeqCst);
    }

    pub fn update_position(&self, from: usize, to: usize) {
        debug_assert!(to - from == 1);

        let from_index = from % self.tokens.len();
        let to_index = to % self.tokens.len();

        if let Some(to_token) = self.tokens.get(to_index) {
            to_token.fetch_add(1, Ordering::SeqCst);
        } else {
            panic!("index out of range on to token!");
        }

        let previous = if let Some(from_token) = self.tokens.get(from_index) {
            from_token.fetch_sub(1, Ordering::SeqCst)
        } else {
            panic!("index out of range on to token!");
        };

        if previous == 1
            && self
                .tail
                .compare_exchange(from as i64, to as i64, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
        {
            self.wait_strategy.notify();
        }
    }

    pub fn wait_for_tail(&self, min_tail_value: i64) -> i64 {
        let v = self
            .wait_strategy
            .wait_for_at_least(&self.tail, min_tail_value);
        debug_assert!(self.tail.load(Ordering::Relaxed) >= min_tail_value);
        v
    }

    pub fn current_tail_position(&self) -> i64 {
        self.tail.load(SeqCst)
    }
}
