use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
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
    value: UnsafeCell<MaybeUninit<T>>,
    counter: AtomicI64,
    current_id: AtomicI64,
}

impl<T> Drop for Cell<T> {
    fn drop(&mut self) {
        if self.should_drop() {
            unsafe {
                self.value.get_mut().assume_init_drop();
            }
        }
    }
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
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
        let old_id = self.current_id.load(Ordering::Relaxed);

        let mut old_value = None;
        unsafe {
            let dst = (*self.value.get()).as_mut_ptr();
            if old_id < 0 {
                dst.write(value);
            } else {
                old_value = Some(core::ptr::replace(dst, value));
            }
        }

        if id == 0 {
            self.counter.to_positive();
        }
        self.current_id.store(id, Ordering::Release);
        drop(old_value);
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

    pub fn queue_for_read(&self) {
        self.counter
            .fetch_update(Ordering::Release, Ordering::Acquire, |v| {
                if v <= 0 {
                    Some(v - 1)
                } else {
                    Some(v + 1)
                }
            })
            .expect("couldn't queue for read");
    }

    pub fn should_drop(&self) -> bool {
        self.current_id.load(Ordering::Acquire) >= 0
    }
}

impl<T> Cell<T>
where
    T: Clone,
{
    pub fn read(&self) -> T {
        unsafe { (*(*self.value.get()).as_ptr()).clone() }
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
