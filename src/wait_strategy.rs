use core::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering::Acquire;

#[derive(Debug)]
pub struct Hybrid {
    num_spin: u64,
    num_yield: u64,
    lock: parking_lot::Mutex<i64>,
    event: parking_lot::Condvar,
}

impl Hybrid {
    pub const fn new(_: u64, _: u64, starting_value: i64) -> Self {
        Self {
            num_spin: 0,
            num_yield: 0,
            lock: parking_lot::Mutex::new(starting_value),
            event: parking_lot::Condvar::new(),
        }
    }
}

impl Hybrid {
    pub fn wait_for_at_least(&self, variable: &AtomicI64, min_value: i64) -> i64 {
        let mut current_value;
        for _ in 0..self.num_spin {
            current_value = variable.load(Acquire);
            if current_value >= min_value {
                return current_value;
            }
            core::hint::spin_loop();
        }
        for _ in 0..self.num_yield {
            current_value = variable.load(Acquire);
            if current_value >= min_value {
                return current_value;
            }
            std::thread::yield_now();
        }

        let mut current_value = variable.load(Acquire);
        if current_value >= min_value {
            return current_value;
        }
        let mut guard = self.lock.lock();
        self.event.wait_while(&mut guard, |_| {
            current_value = variable.load(Acquire);
            current_value < min_value
        });

        current_value
    }
    pub fn notify(&self, value: i64) {
        let mut guard = self.lock.lock();
        if *guard >= value {
            //don't need to notify. Whoever wrote the larger value will do it!
            return;
        }
        *guard = value;
        self.event.notify_all();
    }
}
