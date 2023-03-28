use crate::cell::Cell;
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
    current_cell: *mut Cell<T>,
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let nexus_details = nexus.get_details();
        Self {
            nexus,
            nexus_details,
            current_cell: core::ptr::null_mut(),
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            nexus: Arc::clone(&self.nexus),
            nexus_details: self.nexus_details,
            current_cell: core::ptr::null_mut(),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.current_cell.is_null() {
            return;
        }
        // This could happen if send was being run just as this thread was being aborted
        unsafe {
            // return the cell and notify other threads that it's available again
            (*self.nexus_details.tail).store(self.current_cell, Ordering::Release);
            (*self.nexus_details.tail_wait_strategy).notify_one();
        }
    }
}

impl<T> Sender<T>
where
    T: Send,
{
    /// Send a value to the channel. This method will block until a slot becomes available. This
    /// is the most efficient way to send to the channel as using this function eliminates racing
    /// for the next available slot.
    pub fn send(&mut self, value: T) {
        unsafe {
            debug_assert!(self.current_cell.is_null());
            self.current_cell =
                (*self.nexus_details.tail_wait_strategy).take_ptr(&(*self.nexus_details.tail));

            (*self.current_cell).wait_for_write_safe();

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            let cell = self.current_cell;
            self.current_cell = (*self.nexus_details.tail).swap(next_cell, Ordering::Release);

            {
                // if we fail in here it is bad times. The only reason we can/should fail in here
                // is if the thread this is running on is forcibly aborted
                //TODO is there a cleanup we can do in drop to recover
                (*self.nexus_details.tail_wait_strategy).notify_one();

                (*cell).write_and_publish(value, claimed);
            }
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
    pub fn try_send(&mut self, value: T) -> Result<(), SendError<T>> {
        unsafe {
            debug_assert!(self.current_cell.is_null());
            self.current_cell =
                (*self.nexus_details.tail).swap(core::ptr::null_mut(), Ordering::Acquire);

            if self.current_cell.is_null() || !(*self.current_cell).safe_to_write() {
                self.current_cell =
                    (*self.nexus_details.tail).swap(self.current_cell, Ordering::Release);
                return Err(SendError::Full(value));
            }

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            let cell = self.current_cell;
            self.current_cell = (*self.nexus_details.tail).swap(next_cell, Ordering::Release);

            {
                // if we fail in here it is bad times. The only reason we can/should fail in here
                // is if the thread this is running on is forcibly aborted
                //TODO is there a cleanup we can do in drop to recover
                (*self.nexus_details.tail_wait_strategy).notify_one();

                (*cell).write_and_publish(value, claimed);
            }
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
    pub fn try_send_before(&mut self, value: T, deadline: Instant) -> Result<(), SendError<T>> {
        unsafe {
            debug_assert!(self.current_cell.is_null());
            self.current_cell = match (*self.nexus_details.tail_wait_strategy)
                .take_ptr_before(&(*self.nexus_details.tail), deadline)
            {
                Ok(cell) => cell,
                Err(_) => return Err(SendError::Timeout(value)),
            };

            if (*self.current_cell)
                .wait_for_write_safe_before(deadline)
                .is_err()
            {
                self.current_cell =
                    (*self.nexus_details.tail).swap(self.current_cell, Ordering::Release);
                return Err(SendError::Timeout(value));
            }

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            let cell = self.current_cell;
            self.current_cell = (*self.nexus_details.tail).swap(next_cell, Ordering::Release);

            {
                // if we fail in here it is bad times. The only reason we can/should fail in here
                // is if the thread this is running on is forcibly aborted
                //TODO is there a cleanup we can do in drop to recover
                (*self.nexus_details.tail_wait_strategy).notify_one();

                (*cell).write_and_publish(value, claimed);
            }
        }
        Ok(())
    }
}
