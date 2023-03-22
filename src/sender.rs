use crate::wait_strategy::WaitError;
use crate::NexusDetails;
use crate::{cell::Cell, FastMod, NexusQ};
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

    pub fn try_send_until(&mut self, value: T, deadline: Instant) -> Result<(), SendError<T>> {
        unsafe { self.unsafe_try_send_until(value, deadline) }
    }

    pub fn try_send(&mut self, value: T) -> Result<(), SendError<T>> {
        unsafe { self.unsafe_try_send(value) }
    }
}

impl<T> Sender<T>
where
    T: Send,
{
    unsafe fn unsafe_send(&self, value: T) {
        unsafe {
            let (claimed, cell) = self.shared_setup();

            self.wait_for_write(claimed, cell);

            (*cell).write_and_publish(value, claimed);
        }
    }

    unsafe fn unsafe_try_send_until(
        &self,
        value: T,
        deadline: Instant,
    ) -> Result<(), SendError<T>> {
        unsafe {
            let (claimed, cell) = self.shared_setup();

            self.wait_for_write_until(claimed, cell, deadline)?;

            (*cell).write_and_publish(value, claimed);
            Ok(())
        }
    }

    unsafe fn unsafe_try_send(&self, value: T) -> Result<(), SendError<T>> {
        unsafe {
            let (claimed, cell) = self.shared_setup();

            if !self.try_wait(claimed, cell) {
                return Err(SendError::Full(value));
            }

            (*cell).write_and_publish(value, claimed);
            Ok(())
        }
    }
}

impl<T> Sender<T> {
    unsafe fn shared_setup(&self) -> (usize, *mut Cell<T>) {
        let claimed = (*self.nexus_details.claimed).fetch_add(1, Ordering::Relaxed);

        let index = claimed.fast_mod(self.nexus_details.buffer_length);

        let cell = self.nexus_details.buffer_raw.add(index);
        (claimed, cell)
    }

    unsafe fn wait_for_write(&self, claimed: usize, cell: *mut Cell<T>) {
        let target = claimed.wrapping_sub(1);
        (*self.nexus_details.tail_wait_strategy).wait_for(&(*self.nexus_details.tail), target);

        (*cell).wait_for_write_safe();

        (*self.nexus_details.tail).store(claimed, Ordering::Release);
        (*self.nexus_details.tail_wait_strategy).notify();
    }

    unsafe fn wait_for_write_until(
        &self,
        claimed: usize,
        cell: *mut Cell<T>,
        deadline: Instant,
    ) -> Result<(), SendError<T>> {
        let target = claimed.wrapping_sub(1);
        (*self.nexus_details.tail_wait_strategy).wait_until(
            &(*self.nexus_details.tail),
            target,
            deadline,
        )?;

        (*cell).wait_for_write_safe_until(deadline)?;

        (*self.nexus_details.tail).store(claimed, Ordering::Release);
        (*self.nexus_details.tail_wait_strategy).notify();
        Ok(())
    }

    unsafe fn try_wait(&self, claimed: usize, cell: *mut Cell<T>) -> bool {
        let target = claimed.wrapping_sub(1);
        if (*self.nexus_details.tail).load(Ordering::Acquire) != target || !(*cell).safe_to_write()
        {
            return false;
        }

        (*self.nexus_details.tail).store(claimed, Ordering::Release);
        (*self.nexus_details.tail_wait_strategy).notify();
        true
    }
}
