use super::{AsyncEventGuard, Notifiable, Take, Takeable, Wait, WaitError, Waitable};
use core::fmt::Debug;
use event_listener::EventListener;
use std::pin::{pin, Pin};
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use std::time::Instant;

/// Hybrid wait can use some combination of spinning, yielding, and blocking to wait for a condition
/// to be met.
/// Blocking is achieved via the [`event_listener`] crate. Event listener will use std (in the form of a mutex) if enabled
/// otherwise it falls back on a spinlock.
///
/// That means that [`HybridWait`] is usable in no-std environments.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct HybridWait {
    num_spin: u64,
    num_yield: u64,
    event: event_listener::Event,
}

impl Clone for HybridWait {
    fn clone(&self) -> Self {
        Self::new(self.num_spin, self.num_yield)
    }
}

impl HybridWait {
    /// Create a new [`HybridWait`]. [`HybridWait`] is a wait strategy that will spin for a number of times.
    /// If the condition is not met it will yield the cpu for a number of times. If the condition is
    /// still not met it will wait on an event listener.
    ///
    /// # Arguments
    ///
    /// * `num_spin`: number of times to spin
    /// * `num_yield`: number of times to yield the cpu
    ///
    /// returns: [`HybridWait`]
    ///
    /// # Examples
    ///
    /// ```
    ///# use std::sync::Arc;
    ///# use std::thread;
    ///# use portable_atomic::AtomicUsize;
    ///# use nexusq2::wait_strategy::{hybrid::HybridWait, Wait, Waitable, Notifiable};
    /// let wait = HybridWait::new(50, 50);
    /// let x = AtomicUsize::new(0);
    /// //spawn a scoped thread that waits for x to be 1 using wait
    /// thread::scope(|s| {
    ///     let handle = s.spawn(||{
    ///         wait.wait_for(&x, &1);
    ///     });
    ///     //wait for the thread to start
    ///     thread::sleep(std::time::Duration::from_millis(10));
    ///     //check that the thread is not finished
    ///     assert!(!handle.is_finished());
    ///     //set x to 1
    ///     x.store(1, std::sync::atomic::Ordering::Release);
    ///     //notify the wait strategy
    ///     wait.notify_all();
    ///     //check that the thread is finished
    ///     thread::sleep(std::time::Duration::from_millis(10));
    ///     assert!(handle.is_finished());
    /// });
    /// ```
    #[must_use]
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
        Self::new(50, 0)
    }
}

impl Notifiable for HybridWait {
    fn notify_all(&self) {
        portable_atomic::fence(Ordering::Acquire);
        self.event.notify_relaxed(usize::MAX);
    }
    fn notify_one(&self) {
        self.event.notify_relaxed(1);
    }
}

impl<W> Wait<W> for HybridWait
where
    W: Waitable,
{
    fn wait_for(&self, waitable: &W, expected_value: &W::Inner) {
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
        let mut listen_guard = pin!(EventListener::new(&self.event));
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
        expected_value: &W::Inner,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        for _ in 0..self.num_spin {
            if waitable.check(expected_value) {
                return Ok(());
            }
            if Instant::now() >= deadline {
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
        let mut listen_guard = pin!(EventListener::new(&self.event));
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
        expected_value: &W::Inner,
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
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
        loop {
            if waitable.check(expected_value) {
                *event_listener = None;
                return Poll::Ready(());
            }
            let poll = listen_guard.as_mut().poll_event(cx);
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
        let mut listen_guard = pin!(EventListener::new(&self.event));
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
        for _ in 0..self.num_spin {
            if let Some(v) = ptr.try_take() {
                return Ok(v);
            }
            if Instant::now() >= deadline {
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
        let mut listen_guard = pin!(EventListener::new(&self.event));
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
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
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

        loop {
            if let Some(ptr) = ptr.try_take() {
                *event_listener = None;
                return Poll::Ready(ptr);
            }
            let poll = listen_guard.as_mut().poll_event(cx);
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
}
