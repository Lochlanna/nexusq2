use crate::wait_strategy::{HybridWait, WaitError};
use core::fmt::Debug;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

trait ToPositive {
    fn to_positive(&self);
}

impl ToPositive for AtomicI64 {
    //TODO is there an optimisation that can be made here?
    #[cold]
    fn to_positive(&self) {
        let res = self.fetch_update(Ordering::Release, Ordering::Acquire, |v| Some(-v));
        debug_assert!(res.is_ok());
    }
}

#[derive(Debug)]
pub struct Cell<T> {
    value: UnsafeCell<Option<T>>,
    counter: AtomicI64,
    current_id: AtomicI64,
    wait_strategy: HybridWait,
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        Self {
            value: UnsafeCell::new(None),
            counter: AtomicI64::new(0),
            current_id: AtomicI64::new(-1),
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

    pub fn wait_for_published(&self, expected_published_id: i64) {
        self.wait_strategy
            .wait_for(&self.current_id, expected_published_id);
    }
    pub fn wait_for_published_until(
        &self,
        expected_published_id: i64,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        self.wait_strategy
            .wait_until(&self.current_id, expected_published_id, deadline)
    }
    pub fn wait_for_published_with_timeout(
        &self,
        expected_published_id: i64,
        timeout: Duration,
    ) -> Result<(), WaitError> {
        self.wait_strategy.wait_until(
            &self.current_id,
            expected_published_id,
            Instant::now() + timeout,
        )
    }
}

//write side functions
impl<T> Cell<T> {
    pub fn safe_to_write(&self) -> bool {
        self.counter.load(Ordering::Acquire) == 0
    }

    pub unsafe fn write_and_publish(&self, value: T, id: i64) {
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
        assert!(old > 0);
        self.wait_strategy.notify();
    }

    pub fn move_to(&self) {
        let old = self.counter.fetch_add(1, Ordering::Relaxed);
        debug_assert!(old >= 0);
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

#[cfg(test)]
mod cell_tests {
    use super::*;
    use pretty_assertions_sorted::assert_eq;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_make_positive() {
        for i in -100_000_i64..0 {
            let value = AtomicI64::new(i);
            let expected = i.abs();
            value.to_positive();
            let actual = value.load(Ordering::Relaxed);
            assert_eq!(expected, actual);
        }
    }
}
