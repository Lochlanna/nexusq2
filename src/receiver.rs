use crate::prelude::FastMod;
use crate::wait_strategy::AsyncEventGuard;
use crate::{cell::Cell, NexusDetails, NexusQ};
use alloc::sync::Arc;
use core::fmt::{Debug, Formatter};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use std::time::Instant;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError, PartialOrd, PartialEq, Ord, Eq, Clone, Copy)]
pub enum RecvError {
    /// The operation timed out.
    #[error("timeout while waiting for next value to become available")]
    Timeout,
    /// There is no unread data to be received
    #[error("there's no new data available to be read")]
    NoNewData,
}

pub struct Receiver<T> {
    nexus: Arc<NexusQ<T>>,
    nexus_details: NexusDetails<T>,
    cursor: usize,
    previous_cell: *const Cell<T>,
    // this is only used for async!
    current_event: Option<Pin<Box<dyn AsyncEventGuard>>>,
}

impl<T> Debug for Receiver<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        // write all members of receiver. For current event write Some or None but not the value of Some (as the value is not Debug)
        f.debug_struct("Receiver")
            .field("nexus", &self.nexus)
            .field("nexus_details", &self.nexus_details)
            .field("cursor", &self.cursor)
            .field("previous_cell", &self.previous_cell)
            .field(
                "current_event",
                if self.current_event.is_some() {
                    &"Some"
                } else {
                    &"None"
                },
            )
            .finish()
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for Receiver<T> {}

impl<T> Receiver<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let nexus_details = nexus.get_details();
        let previous_cell = nexus_details.buffer_raw;
        Self::register(nexus_details.buffer_raw);
        nexus.num_receivers.add(1, Ordering::Relaxed);
        Self {
            nexus,
            nexus_details,
            cursor: 1,
            previous_cell,
            current_event: None,
        }
    }
    fn register(buffer: *const Cell<T>) {
        unsafe {
            (*buffer).move_to();
        }
    }

    /// Returns a new Sender that can be used to send data to the channel this receiver is connected to.
    #[must_use]
    pub fn new_sender(&self) -> crate::Sender<T> {
        crate::Sender::new(Arc::clone(&self.nexus))
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        debug_assert!(self.current_event.is_none());
        unsafe {
            (*self.previous_cell).move_to();
            self.nexus.num_receivers.add(1, Ordering::Relaxed);
        }
        Self {
            nexus: Arc::clone(&self.nexus),
            nexus_details: self.nexus.get_details(),
            cursor: self.cursor,
            previous_cell: self.previous_cell,
            current_event: None,
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.nexus.num_receivers.sub(1, Ordering::Relaxed);
        unsafe {
            (*self.previous_cell).move_from();
        }
    }
}

impl<T> Receiver<T>
where
    T: Clone,
{
    /// Wait for the next value to become available and then read it. This method will block until
    /// a new value is available.
    ///
    /// # Examples
    /// ```
    ///# use std::time::{Duration, Instant};
    ///# use nexusq2::make_channel;
    /// let (mut sender, mut receiver) = make_channel::<usize>(3).expect("channel creation failed");
    /// sender.send(1).expect("send failed");
    /// assert_eq!(receiver.recv(), 1);
    /// ```
    pub fn recv(&mut self) -> T {
        unsafe {
            let current_cell = self.get_current_cell();

            (*current_cell).wait_for_published(self.cursor);

            self.do_read(current_cell)
        }
    }

    /// Attempt to read up to `max_results` values from the channel. If there are less than `max_results` values available
    /// then only the available values will be returned. If there are no values available then an empty vector will be returned.
    /// This method will not block. This should be somewhat faster than reading values one at a time.
    ///
    /// # Arguments
    /// * `max_results` - The maximum number of values to read. If there are less than `max_results` values available then only the available values will be returned.
    /// * `buffer` - A vector to store the read values in. This vector doesn't need to be empty. The read values will be appended to the end of the vector.
    ///
    /// # Returns
    /// The number of values read.
    ///
    /// # Examples
    /// ```
    ///# use std::time::{Duration, Instant};
    ///# use nexusq2::make_channel;
    /// let (mut sender, mut receiver) = make_channel::<usize>(10).expect("channel creation failed");
    /// sender.send(16).expect("send failed");
    /// sender.send(32).expect("send failed");
    /// sender.send(64).expect("send failed");
    /// sender.send(128).expect("send failed");
    /// let mut res = Vec::new();
    /// let expected = vec![16, 32, 64, 128];
    /// assert_eq!(receiver.try_recv_batch(4, &mut res), 4);
    /// assert_eq!(res, expected);
    ///# assert!(receiver.try_recv().is_err());
    ///# sender.send(256).expect("send failed");
    ///# assert_eq!(receiver.try_recv().expect("recv failed"), 256);
    /// ```
    pub fn try_recv_batch(&mut self, mut max_results: usize, buffer: &mut Vec<T>) -> usize {
        max_results = max_results.clamp(0, self.nexus_details.buffer_length - 1);
        if max_results == 0 {
            return 0;
        }
        unsafe {
            let mut current_claimed = (*self.nexus_details.claimed).load(Ordering::Acquire);
            let current_claimed_index = current_claimed.fast_mod(self.nexus_details.buffer_length);
            let current_claimed_cell = self.nexus_details.buffer_raw.add(current_claimed_index);
            if (*current_claimed_cell).get_published() < current_claimed {
                // This means that the current claimed cell has not been published yet.
                current_claimed -= 1;
            }
            if current_claimed <= self.cursor {
                return 0;
            }
            let num_available = current_claimed - self.cursor;
            debug_assert!(num_available > 0);

            let num_to_read = num_available.min(max_results);

            buffer.reserve(num_to_read);

            let mut cell = core::ptr::null();
            for i in 0..num_to_read {
                let cell_index = (self.cursor + i).fast_mod(self.nexus_details.buffer_length);
                cell = self.nexus_details.buffer_raw.add(cell_index);
                debug_assert_eq!((*cell).get_published(), self.cursor + i);
                buffer.push((*cell).read());
            }

            self.cursor += num_to_read;
            if !cell.is_null() {
                (*cell).move_to();
                (*self.previous_cell).move_from();
                self.previous_cell = cell;
            }

            num_to_read
        }
    }

    /// Wait for the next value to become available for up to the deadline time.
    /// If the next value is available before the deadline it's read otherwise an
    /// error is returned.
    ///
    /// # Errors
    /// - [`RecvError::Timeout`] The deadline was hit before a new value became available
    ///
    /// # Examples
    /// ```
    ///# use std::time::{Duration, Instant};
    ///# use nexusq2::{make_channel, RecvError};
    /// let (mut sender, mut receiver) = make_channel::<usize>(3).expect("channel creation failed");
    /// let deadline = Instant::now() + Duration::from_millis(100);
    /// assert_eq!(receiver.try_recv_until(deadline), Err(RecvError::Timeout));
    /// ```
    pub fn try_recv_until(&mut self, deadline: Instant) -> Result<T, RecvError> {
        unsafe {
            let current_cell = self.get_current_cell();

            if (*current_cell)
                .wait_for_published_until(self.cursor, deadline)
                .is_err()
            {
                return Err(RecvError::Timeout);
            };

            Ok(self.do_read(current_cell))
        }
    }

    /// Attempts to immediately read the next value. If a new value is not available immediately an
    /// error is returned
    ///
    /// # Errors
    /// - [`RecvError::NoNewData`] There was no unread data in the channel
    ///
    /// # Examples
    /// ```
    ///# use nexusq2::make_channel;
    ///# use nexusq2::RecvError;
    /// let (mut sender, mut receiver) = make_channel::<usize>(3).expect("channel creation failed");
    /// assert!(receiver.try_recv().is_err());
    /// sender.send(1).expect("send failed");
    /// assert_eq!(receiver.try_recv().expect("couldn't receive"), 1);
    /// assert_eq!(receiver.try_recv(), Err(RecvError::NoNewData));
    /// ```
    pub fn try_recv(&mut self) -> Result<T, RecvError> {
        unsafe {
            let current_cell = self.get_current_cell();
            if (*current_cell).get_published() != self.cursor {
                return Err(RecvError::NoNewData);
            }
            Ok(self.do_read(current_cell))
        }
    }

    unsafe fn do_read(&mut self, current_cell: *const Cell<T>) -> T {
        (*current_cell).move_to();
        (*self.previous_cell).move_from();

        self.previous_cell = current_cell;
        self.cursor = self.cursor.wrapping_add(1);

        (*current_cell).read()
    }

    unsafe fn get_current_cell(&self) -> *const Cell<T> {
        let current_index = self.cursor.fast_mod(self.nexus_details.buffer_length);
        self.nexus_details.buffer_raw.add(current_index)
    }
}

impl<T> futures::Stream for Receiver<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        unsafe {
            let current_cell = self.get_current_cell();
            let mut_self = Pin::get_mut(self);
            match (*current_cell).poll_published(cx, mut_self.cursor, &mut mut_self.current_event) {
                Poll::Ready(_) => Poll::Ready(Some(mut_self.do_read(current_cell))),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}
