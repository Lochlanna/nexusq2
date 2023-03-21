use std::cell::UnsafeCell;
use std::mem::forget;
use std::sync::atomic::{AtomicI64, Ordering};

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
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        Self {
            value: UnsafeCell::new(None),
            counter: AtomicI64::new(0),
            current_id: AtomicI64::new(-1),
        }
    }
}

impl<T> Cell<T> {
    pub fn wait_for_readers(&self) {
        while !self.safe_to_write() {
            core::hint::spin_loop();
        }
    }

    pub fn safe_to_write(&self) -> bool {
        self.counter.load(Ordering::Acquire) <= 0
    }

    pub fn write_and_publish(&self, value: T, id: i64) {
        let dst = self.value.get();
        let old_value;
        unsafe {
            old_value = core::ptr::replace(dst, Some(value));
        }
        let old_id = self.current_id.swap(id, Ordering::Release);

        if old_id >= 0 {
            unsafe {
                drop(old_value.unwrap_unchecked());
            }
        } else {
            forget(old_value);
        }
    }

    pub fn move_from(&self) {
        let old = self.counter.fetch_sub(1, Ordering::Release);
        assert!(old > 0);
    }

    pub fn wait_for_published(&self, expected_published_id: i64) {
        while self.current_id.load(Ordering::Acquire) != expected_published_id {
            core::hint::spin_loop();
        }
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
    pub fn read(&self) -> T {
        unsafe { (*self.value.get()).as_ref().unwrap_unchecked().clone() }
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
