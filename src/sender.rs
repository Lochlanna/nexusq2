use crate::{cell::Cell, FastMod, NexusQ};
use alloc::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug)]
pub enum TrySendError<T> {
    Full(T),
}

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    claimed: *const AtomicI64,
    tail: *const AtomicI64,
    buffer_raw: *mut Cell<T>,
    buffer_length: i64,
    buffer_length_unsigned: usize,
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer_length = nexus.buffer.len() as i64;
        let buffer_length_unsigned = nexus.buffer.len();
        let claimed = nexus.get_claimed();
        let tail = nexus.get_tail();
        let buffer_raw = nexus.buffer_raw;
        Self {
            nexus,
            claimed,
            tail,
            buffer_raw,
            buffer_length,
            buffer_length_unsigned,
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
    pub fn try_send(&mut self, value: T) -> Result<(), TrySendError<T>> {
        unsafe { self.try_unsafe_send(value) }
    }
    unsafe fn unsafe_send(&mut self, value: T) {
        let claimed = (*self.claimed).fetch_add(1, Ordering::Relaxed);
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.buffer_length_unsigned);

        let cell = self.buffer_raw.add(index);

        self.wait_for_write(claimed, cell);

        (*cell).write_and_publish(value, claimed);
    }

    unsafe fn wait_for_write(&mut self, claimed: i64, cell: *mut Cell<T>) {
        while (*self.tail).load(Ordering::Acquire) != claimed - 1 {
            core::hint::spin_loop();
        }

        (*cell).wait_for_readers();

        (*self.tail).store(claimed, Ordering::Release);
    }

    unsafe fn try_unsafe_send(&mut self, value: T) -> Result<(), TrySendError<T>> {
        let claimed = (*self.claimed).fetch_add(1, Ordering::Relaxed);
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.buffer_length_unsigned);

        let cell = self.buffer_raw.add(index);

        if (*self.tail).load(Ordering::Acquire) != claimed - 1 || !(*cell).safe_to_write() {
            return Err(TrySendError::Full(value));
        }

        (*self.tail).store(claimed, Ordering::Release);

        (*cell).write_and_publish(value, claimed);
        Ok(())
    }
}
