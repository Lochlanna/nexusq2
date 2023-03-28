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

#[derive(Debug, Clone)]
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

impl<T> Sender<T>
where
    T: Send,
{
    /// Send a value to the channel. This method will block until a slot becomes available. This
    /// is the most efficient way to send to the channel as using this function eliminates racing
    /// for the next available slot.
    pub fn send(&self, value: T) {
        unsafe {
            let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

            let index = claimed.fast_mod(self.nexus_details.buffer_length);

            let cell = self.nexus_details.buffer_raw.add(index);

            let target = claimed.wrapping_sub(1);
            (*self.nexus_details.tail_wait_strategy).wait_for(&(*self.nexus_details.tail), target);

            (*cell).wait_for_write_safe();

            (*self.nexus_details.tail).store(claimed, Ordering::Release);
            (*self.nexus_details.tail_wait_strategy).notify();

            (*cell).write_and_publish(value, claimed);
        }
    }
}
