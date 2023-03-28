use crate::{cell::Cell, FastMod, NexusDetails, NexusQ};
use alloc::sync::Arc;
use std::time::Instant;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("timeout while waiting for next value to become available")]
    Timeout(#[from] crate::wait_strategy::WaitError),
    #[error("there's no new data available to be read")]
    NoNewData,
}

#[derive(Debug)]
pub struct Receiver<T> {
    nexus: Arc<NexusQ<T>>,
    nexus_details: NexusDetails<T>,
    cursor: usize,
    previous_cell: *const Cell<T>,
}

unsafe impl<T> Send for Receiver<T> {}

impl<T> Receiver<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let nexus_details = nexus.get_details();
        let previous_cell = nexus_details.buffer_raw;
        Self::register(nexus_details.buffer_raw);
        Self {
            nexus,
            nexus_details,
            cursor: 1,
            previous_cell,
        }
    }
    fn register(buffer: *const Cell<T>) {
        unsafe {
            (*buffer).move_to();
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        unsafe {
            (*self.previous_cell).move_to();
        }
        Self {
            nexus: Arc::clone(&self.nexus),
            nexus_details: self.nexus.get_details(),
            cursor: self.cursor,
            previous_cell: self.previous_cell,
        }
    }
}

impl<T> Receiver<T>
where
    T: Clone,
{
    /// Wait for the next value to become available for up to the deadline time.
    /// If the next value is available before the deadline it's read otherwise an
    /// error is returned.
    ///
    /// # Errors
    /// - [`Error::Timeout`] The deadline was hit before a new value became available
    pub fn try_recv_until(&mut self, deadline: Instant) -> Result<T, Error> {
        unsafe {
            let current_cell = self.get_current_cell();

            (*current_cell).wait_for_published_until(self.cursor, deadline)?;

            Ok(self.do_read(current_cell))
        }
    }

    /// Wait for the next value to become available and read it.
    /// This function should never fail
    pub fn recv(&mut self) -> T {
        unsafe {
            let current_cell = self.get_current_cell();

            (*current_cell).wait_for_published(self.cursor);

            self.do_read(current_cell)
        }
    }

    /// Attempts to immediately read the next value. If a new value is not available immediately an
    /// error is returned
    ///
    /// # Errors
    /// - [`Error::NoNewData`] There was no unread data in the channel
    pub fn try_recv(&mut self) -> Result<T, Error> {
        unsafe {
            let current_cell = self.get_current_cell();
            if (*current_cell).get_published() != self.cursor {
                return Err(Error::NoNewData);
            }
            Ok(self.do_read(current_cell))
        }
    }
}

impl<T> Receiver<T>
where
    T: Clone,
{
    unsafe fn do_read(&mut self, current_cell: *const Cell<T>) -> T {
        (*current_cell).move_to();
        (*self.previous_cell).move_from();

        self.previous_cell = current_cell;
        self.cursor = self.cursor.wrapping_add(1);

        (*current_cell).read()
    }

    unsafe fn get_current_cell(&mut self) -> *const Cell<T> {
        let current_index = self.cursor.fast_mod(self.nexus_details.buffer_length);
        self.nexus_details.buffer_raw.add(current_index)
    }
}
