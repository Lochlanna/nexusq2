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

#[derive(Debug, ThisError, Ord, PartialOrd, Eq, PartialEq, Clone, Copy)]
pub enum SendError<T> {
    #[error("channel is full")]
    Full(T),
    #[error("timeout while waiting for write slot to become available")]
    Timeout(T),
    #[error("there are no more receivers. The channel is disconnected")]
    Disconnected(T),
}

#[derive(Debug, ThisError)]
pub enum AsyncSendError {
    #[error("there are no more receivers. The channel is disconnected")]
    Disconnected,
}

/// The pending state of an async send operation.
#[derive(Debug)]
struct AsyncState<T> {
    current_cell: *mut cell::Cell<T>,
    claimed: usize,
    current_event: Option<Pin<Box<event_listener::EventListener>>>,
}

impl<T> Default for AsyncState<T> {
    fn default() -> Self {
        Self {
            current_cell: core::ptr::null_mut(),
            claimed: 0,
            current_event: None,
        }
    }
}

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    nexus_details: NexusDetails<T>,
    // Only used for async send
    async_state: AsyncState<T>,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let nexus_details = nexus.get_details();
        Self {
            nexus,
            nexus_details,
            async_state: AsyncState::default(),
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        debug_assert!(self.async_state.current_event.is_none());
        debug_assert_eq!(self.async_state.current_cell, core::ptr::null_mut());
        Self {
            nexus: Arc::clone(&self.nexus),
            nexus_details: self.nexus_details,
            async_state: AsyncState::default(),
        }
    }
}

impl<T> Sender<T>
where
    T: Send,
{
    /// Send a value to the channel. This function will block until the value is sent.
    ///
    /// # Arguments
    ///
    /// * `value`: The value to be sent to the channel
    ///
    /// # Errors
    /// - [`SendError::Disconnected`] There are no more receivers. The channel is disconnected
    ///
    /// # Examples
    ///
    /// ```
    ///# use nexusq2::make_channel;
    /// let (sender, mut receiver) = make_channel(5).expect("Failed to make channel");
    /// sender.send(1).expect("Failed to send");
    /// sender.send(2).expect("Failed to send");
    /// assert_eq!(receiver.recv(), 1);
    /// assert_eq!(receiver.recv(), 2);
    /// ```
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        unsafe {
            if (*self.nexus_details.num_receivers).load(Ordering::Relaxed) == 0 {
                return Err(SendError::Disconnected(value));
            }
            let cell =
                (*self.nexus_details.tail_wait_strategy).take_ptr(&(*self.nexus_details.tail));

            (*cell).wait_for_write_safe();

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            (*self.nexus_details.tail).store(next_cell, Ordering::Release);

            (*self.nexus_details.tail_wait_strategy).notify_one();

            (*cell).write_and_publish(value, claimed);
            Ok(())
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
    /// - [`SendError::Disconnected`] There are no more receivers. The channel is disconnected
    /// # Examples
    /// ```
    ///# use nexusq2::{make_channel, SendError};
    /// let (mut sender, mut receiver) = make_channel(3).expect("couldn't construct channel");
    /// sender.try_send(1).expect("this should be fine");
    /// sender.try_send(2).expect("this should be fine");
    /// sender.try_send(3).expect("this should be fine");
    /// assert_eq!(sender.try_send(4), Err(SendError::Full(4)));
    /// assert_eq!(receiver.recv(), 1);
    /// assert_eq!(receiver.recv(), 2);
    /// assert_eq!(receiver.recv(), 3);
    /// ```
    pub fn try_send(&self, value: T) -> Result<(), SendError<T>> {
        unsafe {
            if (*self.nexus_details.num_receivers).load(Ordering::Relaxed) == 0 {
                return Err(SendError::Disconnected(value));
            }
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
    /// - [`SendError::Disconnected`] There are no more receivers. The channel is disconnected
    /// # Examples
    /// ```
    ///# use std::time::{Duration, Instant};
    ///# use nexusq2::{make_channel, SendError};
    /// let (mut sender, mut receiver) = make_channel(3).expect("couldn't construct channel");
    /// sender.try_send_before(1, Instant::now() + Duration::from_secs(1)).expect("this should be fine");
    /// sender.try_send_before(2, Instant::now() + Duration::from_secs(1)).expect("this should be fine");
    /// sender.try_send_before(3, Instant::now() + Duration::from_secs(1)).expect("this should be fine");
    /// assert_eq!(sender.try_send_before(4, Instant::now() + Duration::from_millis(10)), Err(SendError::Timeout(4)));
    /// assert_eq!(receiver.recv(), 1);
    /// assert_eq!(receiver.recv(), 2);
    /// assert_eq!(receiver.recv(), 3);
    /// ```
    pub fn try_send_before(&self, value: T, deadline: Instant) -> Result<(), SendError<T>> {
        unsafe {
            if (*self.nexus_details.num_receivers).load(Ordering::Relaxed) == 0 {
                return Err(SendError::Disconnected(value));
            }
            if deadline < Instant::now() {
                return Err(SendError::Timeout(value));
            }
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
    type Error = AsyncSendError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.nexus.num_receivers.load(Ordering::Relaxed) == 0 {
            return Poll::Ready(Err(AsyncSendError::Disconnected));
        }
        let mut_self = Pin::get_mut(self);
        unsafe {
            //claim the pointer first
            if mut_self.async_state.current_cell.is_null() {
                let poll = (*mut_self.nexus_details.tail_wait_strategy).poll_ptr(
                    cx,
                    &(*mut_self.nexus_details.tail),
                    &mut mut_self.async_state.current_event,
                );
                if let Poll::Ready(ptr) = poll {
                    debug_assert!(mut_self.async_state.current_event.is_none());
                    mut_self.async_state.current_cell = ptr;
                } else {
                    debug_assert!(mut_self.async_state.current_event.is_some());
                    return Poll::Pending;
                }
            }

            //wait for the cell to become available for writing
            debug_assert!(!mut_self.async_state.current_cell.is_null());
            match (*mut_self.async_state.current_cell)
                .poll_write_safe(cx, &mut mut_self.async_state.current_event)
            {
                Poll::Ready(_) => {
                    debug_assert!(mut_self.async_state.current_event.is_none());
                    let claimed = (*mut_self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);
                    mut_self.async_state.claimed = claimed;

                    let next_index = (claimed + 1).fast_mod(mut_self.nexus_details.buffer_length);
                    let next_cell = mut_self.nexus_details.buffer_raw.add(next_index);

                    (*mut_self.nexus_details.tail).store(next_cell, Ordering::Release);
                    (*mut_self.nexus_details.tail_wait_strategy).notify_one();
                    Poll::Ready(Ok(()))
                }
                Poll::Pending => {
                    debug_assert!(mut_self.async_state.current_event.is_some());
                    Poll::Pending
                }
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        debug_assert!(!self.async_state.current_cell.is_null());
        debug_assert!(self.async_state.current_event.is_none());
        unsafe {
            (*self.async_state.current_cell).write_and_publish(item, self.async_state.claimed);
        }
        self.get_mut().async_state.current_cell = core::ptr::null_mut();
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
