use std::sync::atomic::{AtomicI64, Ordering};

trait ToPositive {
    fn to_positive(&self);
}

impl ToPositive for AtomicI64 {
    //TODO is there an optimisation that can be made here?
    fn to_positive(&self) {
        while self
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| Some(-v))
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
            counter: AtomicI64::new(-1),
        }
    }
}

impl<T> Cell<T> {
    pub fn claim_for_write(&self) {
        if self.counter.load(Ordering::Acquire) < 0 {
            return;
        }
        while self
            .counter
            .compare_exchange_weak(1, -1, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            // we are waiting for readers here!
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
        self.counter.fetch_sub(1, Ordering::Release);
    }

    pub fn wait_for_read(&self) {
        while self.counter.load(Ordering::Acquire) < 0 {
            core::hint::spin_loop();
        }
    }

    pub fn claim_for_read(&self) {
        self.wait_for_read();
        let old = self.counter.fetch_add(1, Ordering::Release);
        debug_assert!(old > 0);
    }

    pub fn initial_queue_for_read(&self) {
        //TODO this will need to do some fancy fancy to check if it's already been flipped
        debug_assert!(self.counter.load(Ordering::Acquire) < 0);
        self.counter.fetch_sub(1, Ordering::Release);
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
