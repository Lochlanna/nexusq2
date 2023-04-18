//! This module defines the traits which can be used to implement wait strategies.
//! It also implements some of the base traits on types used by `NexusQ`
//! `NexusQ` comes with multiple pre implemented wait strategies which will probably
//! cover the requirements of most users however it's possible to implement your own
//! using the traits defined in this module. Custom wait strategies could be useful to users
//! developing for specialised systems.

#[cfg(feature = "backoff")]
pub mod backoff;
pub mod block;
pub mod hybrid;

use core::fmt::Debug;
use portable_atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Instant;
use thiserror::Error as ThisError;
use wake_me::State;

/// Errors that can occur while waiting for some event to occur
#[derive(Debug, ThisError)]
pub enum WaitError {
    /// Wait strategy timed out waiting for the condition.
    #[error("wait strategy timed out waiting for the condition")]
    Timeout,
}

/// This trait should be applied to a type that holds the current state of a pending
/// event. This enables an event to be registered and a handle to that registration to be
/// held in the form of the type that implements this trait. This allows us to poll that registration on
/// the event at a later state.
/// This is only used to implement async behaviour.
pub trait AsyncEventGuard {
    /// Poll the event to see if it has been triggered
    fn poll(&self, cx: &mut Context<'_>) -> Poll<()>;
}

/// A type that can be waited on by checking if the value matches the expected value
pub trait Waitable {
    /// The type of the value that is compared against.
    type Inner;

    /// Check to see if self matches the expected value
    ///
    /// # Arguments
    ///
    /// * `expected`: The expected value
    ///
    /// # Examples
    ///
    /// ```rust
    ///# use portable_atomic::AtomicUsize;
    ///# use nexusq2::wait_strategy::Waitable;
    /// let x = AtomicUsize::new(42);
    /// assert!(x.check(&42));
    /// assert!(!x.check(&21));
    /// ```
    fn check(&self, expected: &Self::Inner) -> bool;
}

/// Takeable types are container types which hold an inner value which can be taken out of the container.
/// An example of a takeable in an `Option<T>` where Option is the container and T is the inner value.
pub trait Takeable {
    /// The type that is taken from the container
    type Inner;
    /// The value that is stored in the container when it is empty
    const TAKEN: Self::Inner;

    /// Attempt to take the value from within self
    ///
    /// # Examples
    ///
    /// ```rust
    ///# use std::sync::atomic::Ordering;
    ///# use portable_atomic::{AtomicUsize};
    ///# use nexusq2::wait_strategy::Takeable;
    /// let x = AtomicUsize::new(42);
    /// assert_eq!(x.try_take().unwrap(), 42);
    /// assert!(x.try_take().is_none());
    /// assert_eq!(x.load(Ordering::Acquire), AtomicUsize::TAKEN);
    /// ```
    fn try_take(&self) -> Option<Self::Inner>;

    /// Put a value back into the container
    fn restore(&self, value: Self::Inner);
}

/// A type that can be notified when an event occurs
pub trait Notifiable {
    /// Notify all current listeners that an event has occurred
    fn notify_all(&self);
    /// Notify a single listener that an event has occurred
    fn notify_one(&self);
}

/// A type which has the ability to wait for a value to be set on some waitable value
pub trait Wait<W: Waitable>: Notifiable {
    /// Wait for the waitable to have the expected value.
    ///
    /// # Arguments
    ///
    /// * `waitable`: A waitable object
    /// * `expected_value`: The expected value of the waitable upon completion of the wait
    ///
    /// # Examples
    ///
    /// ```rust
    ///# use std::sync::Arc;
    ///# use std::thread;
    ///# use portable_atomic::{AtomicUsize, Ordering};
    ///# use nexusq2::wait_strategy::{hybrid::HybridWait, Wait, Waitable, Notifiable};
    /// let wait = HybridWait::new(50, 50);
    /// let x = AtomicUsize::new(0);
    /// //spawn a scoped thread that waits for x to be 1 using wait
    /// thread::scope(|s| {
    ///     let handle = s.spawn(||{
    ///         wait.wait_for(&x, &1);
    ///     });
    ///     //wait for the thread to start
    ///     thread::sleep(std::time::Duration::from_millis(50));
    ///     x.store(1, Ordering::Release);
    ///     //notify the wait strategy
    ///     thread::sleep(std::time::Duration::from_millis(50));
    ///     wait.notify_all();
    ///     handle.join().expect("couldn't join thread!")
    /// });
    /// ```
    fn wait_for(&self, waitable: &W, expected_value: &W::Inner);
    /// Wait for the waitable to have the expected value or until the deadline is reached.
    ///
    /// # Arguments
    ///
    /// * `waitable`: A reference to an object that can be waited on
    /// * `expected_value`: The expected value of the waitable upon completion of the wait
    /// * `deadline`: The time at which the wait will be aborted
    ///
    /// # Errors
    ///
    /// * [`WaitError::Timeout`]: The wait timed out before the waitable had the expected value
    ///
    /// # Examples
    ///
    /// ```rust
    ///# use std::sync::Arc;
    ///# use std::thread;
    ///# use std::time::{Duration, Instant};
    ///# use portable_atomic::AtomicUsize;
    ///# use nexusq2::wait_strategy::{hybrid::HybridWait, Wait, Waitable, Notifiable};
    /// let wait = HybridWait::new(50, 50);
    /// let x = AtomicUsize::new(0);
    /// //spawn a scoped thread that waits for x to be 1 using wait
    /// thread::scope(|s| {
    ///     let handle = s.spawn(||{
    ///         assert!(wait.wait_until(&x, &1, Instant::now() + Duration::from_millis(10)).is_err());
    ///     });
    ///     //wait for the thread to start
    ///     thread::sleep(Duration::from_millis(50));
    ///     assert!(handle.is_finished());
    /// });
    /// ```
    fn wait_until(
        &self,
        waitable: &W,
        expected_value: &W::Inner,
        deadline: Instant,
    ) -> Result<(), WaitError>;

    /// Returns immediately if waitable matches expected value otherwise it registers the waker to
    /// wake the thread when the next notification is triggered
    ///
    /// # Arguments
    ///
    /// * `cx`: The current context
    /// * `waitable`: A reference to an object that can be waited on
    /// * `expected_value`: The expected value of the waitable upon completion of the wait
    /// * `event_listener`: A reference to an event listener that will be used to register the waker
    fn poll(
        &self,
        cx: &mut Context<'_>,
        waitable: &W,
        expected_value: &W::Inner,
        event_listener: &mut Option<Box<dyn AsyncEventGuard>>,
    ) -> Poll<()>;
}

/// A type which has the ability to wait for a value to be taken from some takeable value
pub trait Take<T: Takeable>: Notifiable {
    /// Wait for the takeable container to contain a value. Take the value, replacing it with the
    /// default value. This method will block indefinitely.
    ///
    /// # Arguments
    ///
    /// * `ptr`: A reference to an object that can be taken from
    ///
    /// # Examples
    ///
    /// ```rust
    ///# use nexusq2::wait_strategy::{hybrid::HybridWait, Take, Takeable, Notifiable};
    ///# use portable_atomic::{AtomicUsize, Ordering};
    /// let wait = HybridWait::new(50, 50);
    /// let t = AtomicUsize::new(42);
    /// let val = wait.take(&t);
    /// assert_eq!(val, 42);
    /// assert_eq!(t.load(Ordering::Acquire), AtomicUsize::TAKEN);
    /// // We shouldn't be able to take the pointer while it's being held
    /// assert!(wait.take_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_err());
    /// // put the pointer back in the takeable container
    /// t.restore(21);
    /// assert!(wait.take_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_ok());
    /// ```
    fn take(&self, takeable: &T) -> T::Inner;

    /// Try to take the value immediately without waiting
    fn try_take(&self, takeable: &T) -> Option<T::Inner>;
    /// Wait for the takeable container to contain a value. Take the value, replacing it with the
    /// default value. This method will block until the deadline is reached.
    ///
    /// # Arguments
    ///
    /// * `ptr`: A reference to an object that can be taken from
    /// * `deadline`: The time at which the wait will be aborted
    ///
    /// # Errors
    ///
    /// - [`WaitError::Timeout`]: The wait timed out before the takeable container contained a value
    ///
    /// # Examples
    ///
    /// ```rust
    ///# use nexusq2::wait_strategy::{hybrid::HybridWait, Take, Takeable, Notifiable};
    ///# use portable_atomic::{AtomicUsize, Ordering};
    /// let wait = HybridWait::new(50, 50);
    /// let t = AtomicUsize::new(42);
    /// let val = wait.take(&t);
    /// assert_eq!(val, 42);
    /// assert_eq!(t.load(Ordering::Acquire), AtomicUsize::TAKEN);
    /// // We shouldn't be able to take the pointer while it's being held
    /// assert!(wait.take_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_err());
    /// // put the pointer back in the takeable container
    /// t.restore(21);
    /// assert!(wait.take_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_ok());
    /// ```
    fn take_before(&self, takeable: &T, deadline: Instant) -> Result<T::Inner, WaitError>;

    /// Returns immediately with the valid inside takeable if there is one otherwise
    /// it registers the waker to wake the thread when the next notification is triggered.
    ///
    /// # Arguments
    ///
    /// * `cx`: The current context
    /// * `ptr`: A reference to an object that can be taken from
    /// * `event_listener`: A reference to an event listener that will be used to register the waker
    fn poll(
        &self,
        cx: &mut Context<'_>,
        takeable: &T,
        event_listener: &mut Option<Box<dyn AsyncEventGuard>>,
    ) -> Poll<T::Inner>;
}

impl Waitable for AtomicUsize {
    type Inner = usize;

    fn check(&self, expected: &Self::Inner) -> bool {
        self.load(Ordering::Acquire).eq(expected)
    }
}

impl Takeable for AtomicUsize {
    type Inner = usize;
    const TAKEN: Self::Inner = usize::MAX;

    fn try_take(&self) -> Option<Self::Inner> {
        let v = self.swap(Self::TAKEN, Ordering::Acquire);
        if v == Self::TAKEN {
            return None;
        }
        Some(v)
    }

    fn restore(&self, value: Self::Inner) {
        self.store(value, Ordering::Release);
    }
}

impl AsyncEventGuard for wake_me::WaitGuard {
    fn poll(&self, _: &mut Context<'_>) -> Poll<()> {
        match self.get_state() {
            State::Waiting => Poll::Pending,
            State::Notified => Poll::Ready(()),
            State::Dropped => panic!("WaitGuard was dropped"),
        }
    }
}
