use crate::cell::Cell;
use crate::wait_strategy::WaitError;
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
    Timeout(#[from] WaitError),
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
            (*self.nexus_details.tail).store(self.current_cell, Ordering::Relaxed);
            (*self.nexus_details.tail_wait_strategy).notify();
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
            while self.current_cell.is_null() {
                self.current_cell =
                    (*self.nexus_details.tail).swap(core::ptr::null_mut(), Ordering::Relaxed);
            }

            (*self.current_cell).wait_for_write_safe();

            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let next_index = (claimed + 1).fast_mod(self.nexus_details.buffer_length);
            let next_cell = self.nexus_details.buffer_raw.add(next_index);

            let cell = self.current_cell;
            self.current_cell = (*self.nexus_details.tail).swap(next_cell, Ordering::Relaxed);

            {
                // if we fail in here it is bad times. The only reason we can/should fail in here
                // is if the thread this is running on is forcibly aborted
                //TODO is there a cleanup we can do in drop to recover
                (*self.nexus_details.tail_wait_strategy).notify();

                (*cell).write_and_publish(value, claimed);
            }
        }
    }

    pub fn try_send(&self, value: T) -> Result<(), SendError<T>> {
        todo!()
    }

    pub fn try_send_before(&self, value: T, deadline: Instant) -> Result<(), SendError<T>> {
        todo!()
    }
}
