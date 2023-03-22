use crate::FastMod;
use core::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug)]
pub enum WaitError {
    Timeout,
}

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
        Self::new(100_000, 0)
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

    pub fn wait_for_with_timeout(
        &self,
        variable: &AtomicI64,
        expected: i64,
        timeout: Duration,
    ) -> Result<(), WaitError> {
        self.wait_for_with_deadline(variable, expected, Instant::now() + timeout)
    }

    pub fn wait_for_with_deadline(
        &self,
        variable: &AtomicI64,
        expected: i64,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        for n in 0..self.num_spin {
            if variable.load(Ordering::Acquire) == expected {
                return Ok(());
            }
            // We don't want to do this every time during busy spin as it will slow us down a lot
            if n.fast_mod(256) == 0 && Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            core::hint::spin_loop();
        }
        for _ in 0..self.num_yield {
            if variable.load(Ordering::Acquire) == expected {
                return Ok(());
            }
            // Since we're yielding the cpu anyway this is fine
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            std::thread::yield_now();
        }
        loop {
            if variable.load(Ordering::Acquire) == expected {
                return Ok(());
            }
            let listen_guard = self.event.listen();
            if variable.load(Ordering::Acquire) == expected {
                return Ok(());
            }
            if !listen_guard.wait_deadline(deadline) {
                return Err(WaitError::Timeout);
            }
        }
    }
    pub fn notify(&self) {
        self.event.notify(usize::MAX);
    }
}
