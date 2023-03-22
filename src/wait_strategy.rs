use core::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

#[derive(Debug)]
pub struct HybridWait {
    num_spin: u64,
    num_yield: u64,
    event: event_listener::Event,
}

impl HybridWait {
    pub const fn new(num_spin: u64, num_yield: u64) -> Self {
        Self {
            num_spin,
            num_yield,
            event: event_listener::Event::new(),
        }
    }
}

impl Default for HybridWait {
    fn default() -> Self {
        Self::new(100000, 0)
    }
}

impl HybridWait {
    pub fn wait_for(&self, variable: &AtomicI64, expected: i64) {
        for _ in 0..self.num_spin {
            if variable.load(Ordering::Acquire) == expected {
                return;
            }
            core::hint::spin_loop();
        }
        for _ in 0..self.num_yield {
            if variable.load(Ordering::Acquire) == expected {
                return;
            }
            std::thread::yield_now();
        }
        loop {
            if variable.load(Ordering::Acquire) == expected {
                return;
            }
            let listen_guard = self.event.listen();
            if variable.load(Ordering::Acquire) == expected {
                return;
            }
            listen_guard.wait();
        }
    }
    pub fn notify(&self) {
        self.event.notify(usize::MAX);
    }
}
