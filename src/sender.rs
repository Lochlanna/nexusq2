use crate::NexusDetails;
use crate::{cell::Cell, FastMod, NexusQ};
use alloc::sync::Arc;
use std::sync::atomic::{Ordering};

#[derive(Debug)]
pub enum SendError<T> {
    Full(T),
    Closed
}

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    nexus_details: NexusDetails<T>
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let nexus_details = nexus.get_details();
        Self {
            nexus,
            nexus_details
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self::new(Arc::clone(&self.nexus))
    }
}

impl<T> Sender<T>
where
    T: Send,
{
    pub fn send(&mut self, value: T) {
        unsafe {
            self.unsafe_send(value);
        }
    }

    /// # Arguments
    ///
    /// * `value`: The value to be broadcast on the channel
    ///
    /// # Errors
    /// * `SenderError::Full`: cannot put value onto queue as there's no free space
    pub fn try_send(&mut self, value: T) -> Result<(), SendError<T>> {
        unsafe { self.try_unsafe_send(value) }
    }
    unsafe fn unsafe_send(&mut self, value: T) {

        let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.nexus_details.buffer_length);

        let cell = self.nexus_details.buffer_raw.add(index);

        self.wait_for_write(claimed, cell);

        (*cell).write_and_publish(value, claimed);
    }

    unsafe fn wait_for_write(&mut self, claimed: i64, cell: *mut Cell<T>) {
        (*self.nexus_details.tail_wait_strategy).wait_for(&(*self.nexus_details.tail), claimed - 1);

        (*cell).wait_for_readers();

        (*self.nexus_details.tail).store(claimed, Ordering::Release);
        (*self.nexus_details.tail_wait_strategy).notify();
    }

    unsafe fn try_unsafe_send(&mut self, value: T) -> Result<(), SendError<T>> {
        let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.nexus_details.buffer_length);

        let cell = self.nexus_details.buffer_raw.add(index);

        if (*self.nexus_details.tail).load(Ordering::Acquire) != claimed - 1 || !(*cell).safe_to_write() {
            return Err(SendError::Full(value));
        }

        (*self.nexus_details.tail).store(claimed, Ordering::Release);
        (*self.nexus_details.tail_wait_strategy).notify();

        (*cell).write_and_publish(value, claimed);
        Ok(())
    }
}
