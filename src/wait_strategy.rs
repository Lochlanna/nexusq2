use crate::FastMod;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::time::Instant;
use thiserror::Error as ThisError;

pub trait Waitable {
    type Inner: Copy;
    fn check(&self, expected: Self::Inner) -> bool;
}

impl Waitable for AtomicUsize {
    type Inner = usize;

    fn check(&self, expected: Self::Inner) -> bool {
        self.load(Ordering::Acquire) == expected
    }
}

pub trait Takeable {
    type Inner: Copy;
    fn try_take(&self) -> Option<Self::Inner>;
}

impl<T> Takeable for AtomicPtr<T> {
    type Inner = *mut T;

    fn try_take(&self) -> Option<Self::Inner> {
        let v = self.swap(core::ptr::null_mut(), Ordering::Acquire);
        if v.is_null() {
            return None;
        }
        Some(v)
    }
}

pub trait WaitStrategy {
    fn wait_for<W: Waitable>(&self, waitable: &W, expected_value: W::Inner);
    fn wait_until<W: Waitable>(
        &self,
        value: &W,
        expected_value: W::Inner,
        deadline: Instant,
    ) -> Result<(), WaitError>;
    fn take_ptr<T: Takeable>(&self, ptr: &T) -> T::Inner;
    fn take_ptr_before<T: Takeable>(
        &self,
        ptr: &T,
        deadline: Instant,
    ) -> Result<T::Inner, WaitError>;
    fn notify_all(&self);
    fn notify_one(&self);
}

#[derive(Debug, ThisError)]
pub enum WaitError {
    #[error("wait strategy timed out waiting for the condition")]
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
        Self::new(50, 50)
    }
}

impl WaitStrategy for HybridWait {
    fn wait_for<W: Waitable>(&self, waitable: &W, expected_value: W::Inner) {
        for _ in 0..self.num_spin {
            if waitable.check(expected_value) {
                return;
            }
            core::hint::spin_loop();
        }
        for _ in 0..self.num_yield {
            if waitable.check(expected_value) {
                return;
            }
            std::thread::yield_now();
        }
        loop {
            if waitable.check(expected_value) {
                return;
            }
            let listen_guard = self.event.listen();
            if waitable.check(expected_value) {
                return;
            }
            listen_guard.wait();
        }
    }

    fn wait_until<W: Waitable>(
        &self,
        waitable: &W,
        expected_value: W::Inner,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        for n in 0..self.num_spin {
            if waitable.check(expected_value) {
                return Ok(());
            }
            // We don't want to do this every time during busy spin as it will slow us down a lot
            if n.fast_mod(256) == 0 && Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            core::hint::spin_loop();
        }
        for _ in 0..self.num_yield {
            if waitable.check(expected_value) {
                return Ok(());
            }
            // Since we're yielding the cpu anyway this is fine
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            std::thread::yield_now();
        }
        loop {
            if waitable.check(expected_value) {
                return Ok(());
            }
            let listen_guard = self.event.listen();
            if waitable.check(expected_value) {
                return Ok(());
            }
            if !listen_guard.wait_deadline(deadline) {
                return Err(WaitError::Timeout);
            }
        }
    }

    fn take_ptr<T: Takeable>(&self, ptr: &T) -> T::Inner {
        for _ in 0..self.num_spin {
            if let Some(v) = ptr.try_take() {
                return v;
            }
            core::hint::spin_loop();
        }
        for _ in 0..self.num_yield {
            if let Some(v) = ptr.try_take() {
                return v;
            }
            std::thread::yield_now();
        }
        loop {
            if let Some(v) = ptr.try_take() {
                return v;
            }
            let listen_guard = self.event.listen();
            if let Some(v) = ptr.try_take() {
                return v;
            }
            listen_guard.wait();
        }
    }

    fn take_ptr_before<T: Takeable>(
        &self,
        ptr: &T,
        deadline: Instant,
    ) -> Result<T::Inner, WaitError> {
        for n in 0..self.num_spin {
            if let Some(v) = ptr.try_take() {
                return Ok(v);
            }
            if n.fast_mod(256) == 0 && Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            core::hint::spin_loop();
        }
        for _ in 0..self.num_yield {
            if let Some(v) = ptr.try_take() {
                return Ok(v);
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            std::thread::yield_now();
        }
        loop {
            if let Some(v) = ptr.try_take() {
                return Ok(v);
            }
            let listen_guard = self.event.listen();
            if let Some(v) = ptr.try_take() {
                return Ok(v);
            }
            if !listen_guard.wait_deadline(deadline) {
                return Err(WaitError::Timeout);
            }
        }
    }

    fn notify_all(&self) {
        self.event.notify(usize::MAX);
    }

    fn notify_one(&self) {
        self.event.notify(1);
    }
}
