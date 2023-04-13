#[cfg(feature = "backoff")]
pub mod backoff;
pub mod hybrid;

use core::fmt::Debug;
use event_listener::EventListener;
use portable_atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use thiserror::Error as ThisError;

pub trait AsyncEventGuard {
    /// Poll to see if the event has been triggered
    ///
    /// # Arguments
    ///
    /// * `cx`: The async context. This is used to register the waker to be woken when the event is triggered
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>;
}

pub trait Waitable {
    type Inner;

    /// Check to see if self matches the expected value
    ///
    /// # Arguments
    ///
    /// * `expected`: The expected value
    ///
    /// # Examples
    ///
    /// ```
    ///# use portable_atomic::AtomicUsize;
    ///# use nexusq2::wait_strategy::Waitable;
    /// let x = AtomicUsize::new(42);
    /// assert!(x.check(&42));
    /// assert!(!x.check(&21));
    /// ```
    fn check(&self, expected: &Self::Inner) -> bool;
}

pub trait Takeable {
    type Inner;

    /// Attempt to take the value from within self
    ///
    /// # Examples
    ///
    /// ```
    ///# use portable_atomic::{AtomicPtr, AtomicUsize};
    ///# use nexusq2::wait_strategy::Takeable;
    /// let ptr = Box::into_raw(Box::new(42));
    /// let x = AtomicPtr::new(ptr);
    /// assert_eq!(x.try_take().unwrap(), ptr);
    /// assert!(x.try_take().is_none());
    ///# unsafe { Box::from_raw(ptr) };
    /// ```
    fn try_take(&self) -> Option<Self::Inner>;
}

pub trait Notifiable {
    /// Notify all current listeners that an event has occurred
    fn notify_all(&self);
    /// Notify a single listener that an event has occurred
    fn notify_one(&self);
}

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
    ///     thread::sleep(std::time::Duration::from_millis(50));
    ///     x.store(1, std::sync::atomic::Ordering::Release);
    ///     //notify the wait strategy
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
    /// ```
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
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
    ) -> Poll<()>;
}

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
    /// ```
    ///# use nexusq2::wait_strategy::{hybrid::HybridWait, Take, Takeable, Notifiable};
    ///# use portable_atomic::{AtomicPtr, Ordering};
    /// let wait = HybridWait::new(50, 50);
    /// let t = AtomicPtr::new(Box::into_raw(Box::new(1)));
    /// let ptr = wait.take_ptr(&t);
    /// assert!(!ptr.is_null());
    /// // We shouldn't be able to take the pointer while it's being held
    /// assert!(wait.take_ptr_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_err());
    /// // put the pointer back in the takeable container
    /// t.store(ptr, Ordering::Release);
    /// assert!(wait.take_ptr_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_ok());
    ///# unsafe{Box::from_raw(ptr);}
    /// ```
    fn take_ptr(&self, ptr: &T) -> T::Inner;
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
    /// ```
    ///# use nexusq2::wait_strategy::{hybrid::HybridWait, Take, Takeable, Notifiable};
    ///# use portable_atomic::{AtomicPtr, Ordering};
    /// let wait = HybridWait::new(50, 50);
    /// let t = AtomicPtr::new(Box::into_raw(Box::new(1)));
    /// let ptr = wait.take_ptr(&t);
    /// assert!(!ptr.is_null());
    /// // We shouldn't be able to take the pointer while it's being held
    /// assert!(wait.take_ptr_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_err());
    /// // put the pointer back in the takeable container
    /// t.store(ptr, Ordering::Release);
    /// assert!(wait.take_ptr_before(&t, std::time::Instant::now() + std::time::Duration::from_millis(5)).is_ok());
    ///# unsafe{Box::from_raw(ptr);}
    /// ```
    fn take_ptr_before(&self, ptr: &T, deadline: Instant) -> Result<T::Inner, WaitError>;

    /// Returns immediately with the valid inside takeable if there is one otherwise
    /// it registers the waker to wake the thread when the next notification is triggered.
    ///
    /// # Arguments
    ///
    /// * `cx`: The current context
    /// * `ptr`: A reference to an object that can be taken from
    /// * `event_listener`: A reference to an event listener that will be used to register the waker
    fn poll_ptr(
        &self,
        cx: &mut Context<'_>,
        ptr: &T,
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
    ) -> Poll<T::Inner>;
}

impl Waitable for AtomicUsize {
    type Inner = usize;

    fn check(&self, expected: &Self::Inner) -> bool {
        self.load(Ordering::Acquire).eq(expected)
    }
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

impl AsyncEventGuard for EventListener {
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.poll(cx)
    }
}

#[derive(Debug, ThisError)]
pub enum WaitError {
    #[error("wait strategy timed out waiting for the condition")]
    Timeout,
}
