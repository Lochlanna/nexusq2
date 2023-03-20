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
    current_id: AtomicI64,
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        Self {
            value: None,
            counter: AtomicI64::new(0),
            current_id: AtomicI64::new(-1),
        }
    }
}

impl<T> Cell<T> {
    pub fn wait_for_write(&self) {
        while self.counter.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }
    }

    pub fn write_and_publish(&mut self, value: T, id: i64) {
        let old_value = self.value.replace(value);
        if id == 0 {
            self.counter.to_positive();
        }
        self.current_id.store(id, Ordering::Release);
        drop(old_value);
    }

    pub fn initial_finish_write(&self) {
        debug_assert!(self.counter.load(Ordering::Acquire) < 0);
    }

    pub fn finish_read(&self) {
        let old = self.counter.fetch_sub(1, Ordering::Release);
        assert!(old > 0);
    }

    pub fn wait_for_published(&self, expected_published_id: i64) {
        loop {
            let published = self.current_id.load(Ordering::Acquire);
            if published == expected_published_id {
                break;
            }
            debug_assert!(published <= expected_published_id);
            core::hint::spin_loop();
        }
    }

    pub fn claim_for_read(&self) {
        let old = self.counter.fetch_add(1, Ordering::Relaxed);
        debug_assert!(old >= 0);
    }

    pub fn initial_queue_for_read(&self) {
        let old = self.counter.fetch_sub(1, Ordering::Release);
        debug_assert!(old <= 0);
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
