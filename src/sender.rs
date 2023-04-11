use crate::wait_strategy::WaitStrategy;
use crate::{cell, NexusDetails};
use crate::{FastMod, NexusQ};
use alloc::sync::Arc;
use core::fmt::Debug;
use futures::Sink;
use portable_atomic::Ordering;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum SendError<T> {
    #[error("channel is full")]
    Full(T),
    #[error("timeout while waiting for write slot to become available")]
    Timeout(T),
}

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    nexus_details: NexusDetails<T>,
    // These are for async only
    current_cell: *mut cell::Cell<T>,
    claimed: usize,
    current_event: Option<Pin<Box<event_listener::EventListener>>>,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let nexus_details = nexus.get_details();
        Self {
            nexus,
            nexus_details,
            current_cell: core::ptr::null_mut(),
            claimed: 0,
            current_event: None,
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        debug_assert!(self.current_event.is_none());
        debug_assert_eq!(self.current_cell, core::ptr::null_mut());
        Self {
            nexus: Arc::clone(&self.nexus),
            nexus_details: self.nexus_details,
            current_cell: core::ptr::null_mut(),
            claimed: 0,
            current_event: None,
        }
    }
}

impl<T> Sender<T>
where
    T: Send,
{
    /// Send a value to the channel. This method will block until a slot becomes available.
    pub fn send(&self, value: T) {
        unsafe {
            let cell =
                (*self.nexus_details.tail_wait_strategy).take_ptr(&(*self.nexus_details.tail));

            (*cell).wait_for_write_safe();

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            (*self.nexus_details.tail).store(next_cell, Ordering::Release);

            (*self.nexus_details.tail_wait_strategy).notify_one();

            (*cell).write_and_publish(value, claimed);
        }
    }

    /// Attempt to send a value to the channel immediately with no waiting. The given value is
    /// returned on failure
    ///
    /// # Arguments
    ///
    /// * `value`: The value to be sent to the channel
    ///
    /// # Errors
    /// - [`SendError::Full`] The channel is currently full and cannot accept a new value. The value given
    /// to the send function is returned in the error.
    /// # Examples
    ///
    /// ```
    /// let (mut sender, _) = nexusq2::make_channel(3);
    /// sender.try_send(1).expect("this should be fine");
    /// sender.try_send(2).expect("this should be fine");
    /// sender.try_send(3).expect("this should be fine");
    /// assert!(sender.try_send(4).is_err())
    /// ```
    pub fn try_send(&self, value: T) -> Result<(), SendError<T>> {
        unsafe {
            let cell = (*self.nexus_details.tail).swap(core::ptr::null_mut(), Ordering::Acquire);

            if cell.is_null() {
                return Err(SendError::Full(value));
            }
            if !(*cell).safe_to_write() {
                (*self.nexus_details.tail).store(cell, Ordering::Release);
                return Err(SendError::Full(value));
            }

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            (*self.nexus_details.tail).store(next_cell, Ordering::Release);

            (*self.nexus_details.tail_wait_strategy).notify_one();

            (*cell).write_and_publish(value, claimed);

            Ok(())
        }
    }

    /// Attempts to send the value before the deadline.
    /// If the deadline is hit the given value is returned in the error.
    ///
    /// Note that there will be a slight amount of error involved with the deadline so don't rely
    /// on a high level of accuracy. If you need a high level of accuracy do some testing of this function
    /// first to see if it's accurate enough for your needs.
    ///
    /// # Arguments
    ///
    /// * `value`: The value to be sent to the channel
    /// * `deadline`: The instant in time by which the value should be sent. Note there will be a small
    /// amount of error involved with the deadline.
    ///
    /// # Errors
    /// - [`SendError::Timeout`] The value couldn't be sent before the deadline.
    /// The value is contained within the error.
    pub fn try_send_before(&self, value: T, deadline: Instant) -> Result<(), SendError<T>> {
        unsafe {
            let Ok(cell) = (*self.nexus_details.tail_wait_strategy)
                .take_ptr_before(&(*self.nexus_details.tail), deadline) else { return Err(SendError::Timeout(value)) };

            if (*cell).wait_for_write_safe_before(deadline).is_err() {
                (*self.nexus_details.tail).store(cell, Ordering::Release);
                return Err(SendError::Timeout(value));
            }

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            (*self.nexus_details.tail).store(next_cell, Ordering::Release);

            (*self.nexus_details.tail_wait_strategy).notify_one();

            (*cell).write_and_publish(value, claimed);
        }
        Ok(())
    }
}

impl<T> Sink<T> for Sender<T>
where
    T: Send,
{
    type Error = SendError<T>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut_self = Pin::get_mut(self);
        unsafe {
            if mut_self.current_cell.is_null() {
                let poll = (*mut_self.nexus_details.tail_wait_strategy).poll_ptr(
                    cx,
                    &(*mut_self.nexus_details.tail),
                    &mut mut_self.current_event,
                );
                if let Poll::Ready(ptr) = poll {
                    debug_assert!(mut_self.current_event.is_none());
                    mut_self.current_cell = ptr;
                } else {
                    debug_assert!(mut_self.current_event.is_some());
                    return Poll::Pending;
                }
            }
            debug_assert!(!mut_self.current_cell.is_null());
            match (*mut_self.current_cell).poll_write_safe(cx, &mut mut_self.current_event) {
                Poll::Ready(_) => {
                    debug_assert!(mut_self.current_event.is_none());
                    let claimed = (*mut_self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);
                    mut_self.claimed = claimed;

                    let next_index = (claimed + 1).fast_mod(mut_self.nexus_details.buffer_length);
                    let next_cell = mut_self.nexus_details.buffer_raw.add(next_index);

                    (*mut_self.nexus_details.tail).store(next_cell, Ordering::Release);
                    (*mut_self.nexus_details.tail_wait_strategy).notify_one();
                    Poll::Ready(Ok(()))
                }
                Poll::Pending => {
                    debug_assert!(mut_self.current_event.is_some());
                    Poll::Pending
                }
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        debug_assert!(!self.current_cell.is_null());
        debug_assert!(self.current_event.is_none());
        unsafe {
            (*self.current_cell).write_and_publish(item, self.claimed);
        }
        self.get_mut().current_cell = core::ptr::null_mut();
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
