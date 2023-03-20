use std::sync::atomic::{AtomicI64, Ordering};

trait ToPositive {
    fn to_positive(&self);
}

impl ToPositive for AtomicI64 {
    //TODO is there an optimisation that can be made here?
    #[allow(clippy::cast_possible_wrap)]
    fn to_positive(&self) {
        while self
            .fetch_update(Ordering::Release, Ordering::Acquire, |v| Some(-v))
            .is_err()
        {
            core::hint::spin_loop();
        }
    }
}

#[derive(Debug)]
pub struct Cell<T> {
    value: Option<T>,
    counter: AtomicI64,
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        Self {
            value: None,
            counter: AtomicI64::new(0),
        }
    }
}

impl<T> Cell<T> {
    pub fn wait_for_write(&self) {
        while self.counter.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }
    }

    pub fn write(&mut self, value: T) {
        self.value = Some(value);
    }

    pub fn finish_write(&self) {
        self.counter.to_positive();
    }

    pub fn finish_read(&self) {
        let old = self.counter.fetch_sub(1, Ordering::Release);
        assert!(old > 0);
    }

    pub fn claim_for_read(&self) {
        let old = self.counter.fetch_add(1, Ordering::Relaxed);
        debug_assert!(old >= 0);
    }

    pub fn initial_queue_for_read(&self) {
        self.counter.fetch_sub(1, Ordering::Release);
        debug_assert!(self.counter.load(Ordering::Acquire) < 0);
    }
}

impl<T> Cell<T>
where
    T: Clone,
{
    pub fn read(&self) -> T {
        // hopefully this will compile down....
        self.value.as_ref().unwrap().clone()
    }
}

#[cfg(test)]
mod cell_tests {
    use super::*;
    use pretty_assertions_sorted::assert_eq;

    #[test]
    fn test_make_positive() {}
}
