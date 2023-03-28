use crate::NexusDetails;
use crate::{FastMod, NexusQ};
use alloc::sync::Arc;
use std::sync::atomic::Ordering;
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
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let nexus_details = nexus.get_details();
        Self {
            nexus,
            nexus_details,
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            nexus: Arc::clone(&self.nexus),
            nexus_details: self.nexus_details,
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
