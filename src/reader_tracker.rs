use crate::wait_strategy::WaitStrategy;
use crate::{wait_strategy, FastMod};
use core::sync::atomic::{AtomicI64, AtomicUsize, Ordering::*};

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
            tail: AtomicI64::new(0),
            wait_strategy: Default::default(),
        }
    }

    pub fn register(&self) {
        let token = self.tokens.get(0).expect("tokens was 0 sized");
        token.fetch_add(1, SeqCst);
    }

    pub fn update_position(&self, from: i64, to: i64) {
        debug_assert!(to >= 0);
        debug_assert!(to >= from);

        if from == to || from < 0 {
            return;
        }
        debug_assert_eq!(to - from, 1);

        let from_index = (from as usize).fast_mod(self.tokens.len());
        let to_index = (to as usize).fast_mod(self.tokens.len());

        let to_token;
        let from_token;

        unsafe {
            to_token = self.tokens.get_unchecked(to_index);
            from_token = self.tokens.get_unchecked(from_index);
        }

        to_token.fetch_add(1, Release);
        let previous = from_token.fetch_sub(1, AcqRel);

        if previous == 1 {
            let tail = self.tail.load(Acquire);
            if tail == from && from_token.load(Acquire) == 0 {
                self.tail.store(to, Release);
                self.wait_strategy.notify();
            }
        }
    }

    pub fn wait_for_tail(&self, min_tail_value: i64) -> i64 {
        let v = self
            .wait_strategy
            .wait_for_at_least(&self.tail, min_tail_value);
        debug_assert!(self.current_tail_position() >= min_tail_value);
        v
    }

    pub fn current_tail_position(&self) -> i64 {
        self.tail.load(Acquire)
    }
}
