use crate::wait_strategy::{HybridWait, WaitError};
use core::fmt::Debug;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Cell<T> {
    value: UnsafeCell<Option<T>>,
    counter: AtomicUsize,
    current_id: AtomicUsize,
    wait_strategy: HybridWait,
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        Self {
            value: UnsafeCell::new(None),
            counter: AtomicUsize::new(0),
            current_id: AtomicUsize::new(usize::MAX),
            wait_strategy: HybridWait::default(),
        }
    }
}

//wait functions
impl<T> Cell<T> {
    pub fn wait_for_write_safe(&self) {
        self.wait_strategy.wait_for(&self.counter, 0);
    }
    pub fn wait_for_write_safe_with_timeout(&self, timeout: Duration) -> Result<(), WaitError> {
        self.wait_strategy
            .wait_until(&self.counter, 0, Instant::now() + timeout)
    }
    pub fn wait_for_write_safe_until(&self, deadline: Instant) -> Result<(), WaitError> {
        self.wait_strategy.wait_until(&self.counter, 0, deadline)
    }

    pub fn wait_for_published(&self, expected_published_id: usize) {
        self.wait_strategy
            .wait_for(&self.current_id, expected_published_id);
    }
    pub fn wait_for_published_until(
        &self,
        expected_published_id: usize,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        self.wait_strategy
            .wait_until(&self.current_id, expected_published_id, deadline)
    }
    pub fn wait_for_published_with_timeout(
        &self,
        expected_published_id: usize,
        timeout: Duration,
    ) -> Result<(), WaitError> {
        self.wait_strategy.wait_until(
            &self.current_id,
            expected_published_id,
            Instant::now() + timeout,
        )
    }

    pub fn get_published(&self) -> usize {
        self.current_id.load(Ordering::Acquire)
    }
}

//write side functions
impl<T> Cell<T> {
    pub fn safe_to_write(&self) -> bool {
        self.counter.load(Ordering::Acquire) == 0
    }

    pub unsafe fn write_and_publish(&self, value: T, id: usize) {
        let dst = self.value.get();
        let old_value = core::ptr::replace(dst, Some(value));
        self.current_id.store(id, Ordering::Release);
        self.wait_strategy.notify();
        drop(old_value);
    }
}

//read side functions
impl<T> Cell<T> {
    pub fn move_from(&self) {
        let old = self.counter.fetch_sub(1, Ordering::Release);
        assert!(old >= 1);
        self.wait_strategy.notify();
    }

    pub fn move_to(&self) {
        self.counter.fetch_add(1, Ordering::Relaxed);
    }
}
impl<T> Cell<T>
where
    T: Clone,
{
    pub unsafe fn read(&self) -> T {
        (*self.value.get()).as_ref().unwrap_unchecked().clone()
    }
}
