use std::sync::atomic::{AtomicI64, Ordering};

trait ToPositive {
    fn to_positive(&self);
}

impl ToPositive for AtomicI64 {
    #[cfg(target_endian = "little")]
    fn to_positive(&self) {
        self.fetch_and(0x7FFF_FFFF_FFFF_FFFF, Ordering::Release);
    }
    #[cfg(target_endian = "big")]
    fn to_positive(&self) {
        self.fetch_and(0x7FFF_FFFF_FFFF_FFFE, Ordering::Release);
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
            counter: AtomicI64::new(-1),
        }
    }
}

impl<T> Cell<T> {
    pub fn claim_for_write(&self) {
        while self
            .counter
            .compare_exchange_weak(1, -1, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            // we are waiting for readers here!
            core::hint::spin_loop()
        }
    }
    pub fn write(&mut self, value: T) {
        self.value = Some(value);
    }
    pub fn finish_write(&self) {
        self.counter.to_positive();
    }

    pub fn claim_read(&self) {
        let mut current = self.counter.load(Ordering::Acquire);
        debug_assert_ne!(current, 0);
        if current > 0 {
            self.counter.fetch_add(1, Ordering::Release);
            return;
        }
        while let Err(new_current) =
            self.counter
                .compare_exchange(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
        {
            debug_assert_ne!(new_current, 0);
            if new_current > 0 {
                // writer is finished so it's safe to add now
                self.counter.fetch_add(1, Ordering::Release);
                return;
            }
            current = new_current;
        }
        // We managed to subtract one the value is most likely still negative
        while self.counter.load(Ordering::Acquire) < 0 {
            core::hint::spin_loop();
        }
        // writer is done and switch it back to positive so we are good to go!
    }
}

impl<T> Cell<T>
where
    T: Clone,
{
    pub fn clone_value(&self) -> T {
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
