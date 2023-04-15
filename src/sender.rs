use crate::prelude::FastMod;
use crate::wait_strategy::{AsyncEventGuard, Takeable};
use crate::{cell, NexusQ};
use alloc::sync::Arc;
use core::fmt::{Debug, Formatter};
use futures_util::Sink;
use portable_atomic::Ordering;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use thiserror::Error as ThisError;

/// Errors that can be produced by the send methods on a `NexusQ` sender.
/// All errors return the value that was intended to be sent so no data is lost.
#[derive(Debug, ThisError, Ord, PartialOrd, Eq, PartialEq, Clone, Copy)]
pub enum SendError<T> {
    /// There are no free slots in the channel.
    #[error("channel is full")]
    Full(T),
    /// Failed to send the value before the timeout.
    #[error("timeout while waiting for write slot to become available")]
    Timeout(T),
    /// There are no more receivers and therefore the channel is disconnected.
    /// Continued use will always return this error.
    #[error("there are no more receivers. The channel is disconnected")]
    Disconnected(Option<T>),
}

trait MessageId {
    fn valid(&self) -> bool;
}

impl MessageId for usize {
    fn valid(&self) -> bool {
        self.ne(&Self::MAX)
    }
}

/// The pending state of an async send operation.
#[derive(Default)]
struct AsyncState {
    cell_index: Option<usize>,
    claimed: usize,
    event_guard: Option<Pin<Box<dyn AsyncEventGuard>>>,
}

impl Debug for AsyncState {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        //write all members of AsyncState. For async_state write "Some" or "None" but not the value of Some (as the value is not Debug)
        f.debug_struct("AsyncState")
            .field("current_cell", &self.cell_index)
            .field("claimed", &self.claimed)
            .field(
                "async_state",
                if self.event_guard.is_some() {
                    &"Some"
                } else {
                    &"None"
                },
            )
            .finish()
    }
}
/// A send handle for the `NexusQ` channel.
/// This handle can be cloned and sent to other threads.
/// Senders cannot close the channel and can be created from receiver handles!
#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    buffer: Arc<[cell::Cell<T>]>,
    // Only used for async send
    async_state: AsyncState,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer = nexus.buffer.clone();
        Self {
            nexus,
            buffer,
            async_state: AsyncState::default(),
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        debug_assert!(self.async_state.event_guard.is_none());
        debug_assert!(self.async_state.cell_index.is_none());
        Self {
            nexus: Arc::clone(&self.nexus),
            buffer: Arc::clone(&self.buffer),
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
    /// # Errors
    /// - [`SendError::Disconnected`] There are no more receivers. The channel is disconnected
    ///
    /// # Examples
    ///
    /// ```rust
    ///# use nexusq2::make_channel;
    /// let (sender, mut receiver) = make_channel(5).expect("Failed to make channel");
    /// sender.send(1).expect("Failed to send");
    /// sender.send(2).expect("Failed to send");
    /// assert_eq!(receiver.recv(), 1);
    /// assert_eq!(receiver.recv(), 2);
    /// ```
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        unsafe {
            if self.nexus.num_receivers.load(Ordering::Relaxed) == 0 {
                return Err(SendError::Disconnected(Some(value)));
            }
            let cell_index = self.nexus.tail_wait_strategy.take(&self.nexus.tail);
            let cell = self.buffer.get_unchecked(cell_index);

            cell.wait_for_write_safe();

            let claimed = self.nexus.claimed.fetch_add(1, Ordering::Relaxed);

            self.nexus
                .tail
                .restore((cell_index + 1).fast_mod(self.buffer.len()));

            self.nexus.tail_wait_strategy.notify_one();

            cell.write_and_publish(value, claimed);
            Ok(())
        }
    }

    /// Attempt to send a value to the channel immediately with no waiting. The given value is
    /// returned on failure
    ///
    /// # Errors
    /// - [`SendError::Full`] The channel is currently full and cannot accept a new value. The value given
    /// to the send function is returned in the error.
    /// - [`SendError::Disconnected`] There are no more receivers. The channel is disconnected
    /// # Examples
    /// ```rust
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
            if self.nexus.num_receivers.load(Ordering::Relaxed) == 0 {
                return Err(SendError::Disconnected(Some(value)));
            }
            let Some(cell_index) = self.nexus.tail_wait_strategy.try_take(&self.nexus.tail) else {
                return Err(SendError::Full(value));
            };
            let cell = self.buffer.get_unchecked(cell_index);

            if !cell.safe_to_write() {
                self.nexus.tail.restore(cell_index);
                return Err(SendError::Full(value));
            }

            let claimed = (self.nexus.claimed).fetch_add(1, Ordering::Relaxed);

            self.nexus
                .tail
                .restore((cell_index + 1).fast_mod(self.buffer.len()));

            self.nexus.tail_wait_strategy.notify_one();

            cell.write_and_publish(value, claimed);

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
    /// # Errors
    /// - [`SendError::Timeout`] The value couldn't be sent before the deadline.
    /// - [`SendError::Disconnected`] There are no more receivers. The channel is disconnected
    /// # Examples
    /// ```rust
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
            if self.nexus.num_receivers.load(Ordering::Relaxed) == 0 {
                return Err(SendError::Disconnected(Some(value)));
            }
            if deadline < Instant::now() {
                return Err(SendError::Timeout(value));
            }

            let Ok(cell_index) = self.nexus.tail_wait_strategy
                .take_before(&(self.nexus.tail), deadline) else { return Err(SendError::Timeout(value)) };
            let cell = self.buffer.get_unchecked(cell_index);

            if cell.wait_for_write_safe_before(deadline).is_err() {
                (self.nexus.tail).restore(cell_index);
                return Err(SendError::Timeout(value));
            }

            let claimed = (self.nexus.claimed).fetch_add(1, Ordering::Relaxed);

            self.nexus
                .tail
                .restore((cell_index + 1).fast_mod(self.buffer.len()));

            self.nexus.tail_wait_strategy.notify_one();

            cell.write_and_publish(value, claimed);
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
        if self.nexus.num_receivers.load(Ordering::Relaxed) == 0 {
            return Poll::Ready(Err(SendError::Disconnected(None)));
        }
        let mut_self = Pin::get_mut(self);
        unsafe {
            //claim the id first
            let cell_index = match mut_self.async_state.cell_index {
                None => {
                    match mut_self.nexus.tail_wait_strategy.poll(
                        cx,
                        &mut_self.nexus.tail,
                        &mut mut_self.async_state.event_guard,
                    ) {
                        Poll::Ready(index) => {
                            mut_self.async_state.cell_index = Some(index);
                            index
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                Some(index) => index,
            };

            debug_assert!(mut_self.async_state.cell_index.is_some());

            let cell = mut_self.buffer.get_unchecked(cell_index);

            //wait for the cell to become available for writing
            match cell.poll_write_safe(cx, &mut mut_self.async_state.event_guard) {
                Poll::Ready(_) => {
                    debug_assert!(mut_self.async_state.event_guard.is_none());
                    let claimed = mut_self.nexus.claimed.fetch_add(1, Ordering::Relaxed);
                    mut_self.async_state.claimed = claimed;
                    mut_self
                        .nexus
                        .tail
                        .restore((cell_index + 1).fast_mod(mut_self.buffer.len()));
                    mut_self.nexus.tail_wait_strategy.notify_one();
                    Poll::Ready(Ok(()))
                }
                Poll::Pending => {
                    debug_assert!(mut_self.async_state.event_guard.is_some());
                    Poll::Pending
                }
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        debug_assert!(self.async_state.cell_index.is_some());
        debug_assert!(self.async_state.event_guard.is_none());

        let cell_index = self.async_state.cell_index.unwrap();
        unsafe {
            let cell = self.buffer.get_unchecked(cell_index);
            cell.write_and_publish(item, self.async_state.claimed);
        }
        self.get_mut().async_state.cell_index = None;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
