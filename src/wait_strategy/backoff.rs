//! This wait strategy uses the crossbeam-utils backoff strategy to wait for a condition to be met.
//! The non-async methods will never block relying entirely on the backoff strategy. The backoff
//! strategy will spin for a small number of times before yielding the thread. Refer to the
//! [`crossbeam-utils::Backoff`] documentation for more details.
//!
//! When using this wait strategy asyncronously, the backoff strategy is not used and it defaults
//! to immediately using a waker to put the task to sleep via the [`HybridWait`] strategy configured with
//! zero spins and zero yields.

use super::{
    block::BlockStrategy, AsyncEventGuard, Notifiable, Take, Takeable, Wait, WaitError, Waitable,
};
use crossbeam_utils::Backoff;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

/// Backoff wait uses incremental backoff to wait for a condition to be met.
/// This is achieved via the [`crossbeam-utils::Backoff`] type from the `crossbeam-utils` crate.
/// For async no backoff is applied and the waker is used to put the task to sleep if the condition is
/// not met immediately.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct BackoffWait {
    block: BlockStrategy,
}

impl Clone for BackoffWait {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Default for BackoffWait {
    fn default() -> Self {
        Self {
            block: BlockStrategy::new(),
        }
    }
}

impl Notifiable for BackoffWait {
    fn notify_all(&self) {
        self.block.notify_all();
    }

    fn notify_one(&self) {
        self.block.notify_one();
    }
}

impl<W> Wait<W> for BackoffWait
where
    W: Waitable,
{
    fn wait_for(&self, waitable: &W, expected_value: &W::Inner) {
        let backoff = Backoff::new();
        loop {
            if waitable.check(expected_value) {
                return;
            }
            backoff.snooze();
        }
    }

    fn wait_until(
        &self,
        waitable: &W,
        expected_value: &W::Inner,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        // Same as wait_for except we early out if Instant::now() > deadline
        let backoff = Backoff::new();
        loop {
            if waitable.check(expected_value) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            backoff.snooze();
        }
    }

    fn poll(
        &self,
        cx: &mut Context<'_>,
        waitable: &W,
        expected_value: &W::Inner,
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
    ) -> Poll<()> {
        Wait::poll(&self.block, cx, waitable, expected_value, event_listener)
    }
}

impl<T> Take<T> for BackoffWait
where
    T: Takeable,
{
    fn take(&self, ptr: &T) -> T::Inner {
        let backoff = Backoff::new();
        loop {
            if let Some(val) = ptr.try_take() {
                return val;
            }
            backoff.snooze();
        }
    }

    fn try_take(&self, ptr: &T) -> Option<T::Inner> {
        ptr.try_take()
    }

    fn take_before(&self, ptr: &T, deadline: Instant) -> Result<T::Inner, WaitError> {
        let backoff = Backoff::new();
        loop {
            if let Some(val) = ptr.try_take() {
                return Ok(val);
            }
            if Instant::now() >= deadline {
                return Err(WaitError::Timeout);
            }
            backoff.snooze();
        }
    }

    fn poll(
        &self,
        cx: &mut Context<'_>,
        ptr: &T,
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
    ) -> Poll<T::Inner> {
        Take::poll(&self.block, cx, ptr, event_listener)
    }
}
