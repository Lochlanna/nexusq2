use crate::FastMod;
use core::fmt::Debug;
use event_listener::EventListener;
use portable_atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use thiserror::Error as ThisError;

pub trait Waitable {
    type Inner: Copy + Debug;
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

pub trait Wait<W: Waitable>: Debug {
    fn wait_for(&self, waitable: &W, expected_value: W::Inner);
    fn wait_until(
        &self,
        value: &W,
        expected_value: W::Inner,
        deadline: Instant,
    ) -> Result<(), WaitError>;
    fn poll(
        &self,
        cx: &mut Context<'_>,
        waitable: &W,
        expected_value: W::Inner,
        event_listener: &mut Option<Pin<Box<event_listener::EventListener>>>,
    ) -> Poll<()>;
    fn notify_all(&self);
}

pub trait Take<T: Takeable>: Debug {
    fn take_ptr(&self, ptr: &T) -> T::Inner;
    fn take_ptr_before(&self, ptr: &T, deadline: Instant) -> Result<T::Inner, WaitError>;
    fn poll_ptr(
        &self,
        cx: &mut Context<'_>,
        ptr: &T,
        event_listener: &mut Option<Pin<Box<event_listener::EventListener>>>,
    ) -> Poll<T::Inner>;
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

impl<W> Wait<W> for HybridWait
where
    W: Waitable,
{
    fn wait_for(&self, waitable: &W, expected_value: W::Inner) {
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
        if waitable.check(expected_value) {
            return;
        }
        let mut listen_guard = Box::pin(event_listener::EventListener::new(&self.event));
        loop {
            listen_guard.as_mut().listen();
            if waitable.check(expected_value) {
                return;
            }
            listen_guard.as_mut().wait();
            if waitable.check(expected_value) {
                return;
            }
        }
    }

    fn wait_until(
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
        if waitable.check(expected_value) {
            return Ok(());
        }
        let mut listen_guard = Box::pin(event_listener::EventListener::new(&self.event));
        loop {
            listen_guard.as_mut().listen();
            if waitable.check(expected_value) {
                return Ok(());
            }
            if !listen_guard.as_mut().wait_deadline(deadline) {
                return Err(WaitError::Timeout);
            }
            if waitable.check(expected_value) {
                return Ok(());
            }
        }
    }

    fn poll(
        &self,
        cx: &mut Context<'_>,
        waitable: &W,
        expected_value: W::Inner,
        event_listener: &mut Option<Pin<Box<EventListener>>>,
    ) -> Poll<()> {
        if waitable.check(expected_value) {
            *event_listener = None;
            return Poll::Ready(());
        }
        #[allow(clippy::option_if_let_else)]
        let mut listen_guard = match event_listener {
            None => event_listener.insert(self.event.listen()),
            Some(lg) => lg,
        };
        debug_assert!(listen_guard.listens_to(&self.event));
        loop {
            if waitable.check(expected_value) {
                *event_listener = None;
                return Poll::Ready(());
            }
            let poll = listen_guard.as_mut().poll(cx);
            match poll {
                Poll::Ready(_) => {
                    if waitable.check(expected_value) {
                        *event_listener = None;
                        return Poll::Ready(());
                    }
                    listen_guard = event_listener.insert(self.event.listen());
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }

    fn notify_all(&self) {
        self.event.notify(usize::MAX);
    }
}

impl<T> Take<T> for HybridWait
where
    T: Takeable,
{
    fn take_ptr(&self, ptr: &T) -> T::Inner {
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
        if let Some(v) = ptr.try_take() {
            return v;
        }
        let mut listen_guard = Box::pin(event_listener::EventListener::new(&self.event));
        loop {
            listen_guard.as_mut().listen();
            if let Some(v) = ptr.try_take() {
                return v;
            }
            listen_guard.as_mut().wait();
            if let Some(v) = ptr.try_take() {
                return v;
            }
        }
    }

    fn take_ptr_before(&self, ptr: &T, deadline: Instant) -> Result<T::Inner, WaitError> {
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
        if let Some(v) = ptr.try_take() {
            return Ok(v);
        }
        let mut listen_guard = Box::pin(event_listener::EventListener::new(&self.event));
        loop {
            listen_guard.as_mut().listen();
            if let Some(v) = ptr.try_take() {
                return Ok(v);
            }
            if !listen_guard.as_mut().wait_deadline(deadline) {
                return Err(WaitError::Timeout);
            }
            if let Some(v) = ptr.try_take() {
                return Ok(v);
            }
        }
    }

    fn poll_ptr(
        &self,
        cx: &mut Context<'_>,
        ptr: &T,
        event_listener: &mut Option<Pin<Box<event_listener::EventListener>>>,
    ) -> Poll<T::Inner> {
        if let Some(ptr) = ptr.try_take() {
            *event_listener = None;
            return Poll::Ready(ptr);
        }
        #[allow(clippy::option_if_let_else)]
        let mut listen_guard = match event_listener {
            None => event_listener.insert(self.event.listen()),
            Some(lg) => lg,
        };
        debug_assert!(listen_guard.listens_to(&self.event));

        loop {
            if let Some(ptr) = ptr.try_take() {
                *event_listener = None;
                return Poll::Ready(ptr);
            }
            let poll = listen_guard.as_mut().poll(cx);
            match poll {
                Poll::Ready(_) => {
                    if let Some(ptr) = ptr.try_take() {
                        *event_listener = None;
                        return Poll::Ready(ptr);
                    }
                    listen_guard = event_listener.insert(self.event.listen());
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }

    fn notify_one(&self) {
        self.event.notify(1);
    }
}
