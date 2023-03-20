use crate::{cell::Cell, FastMod, NexusQ};
use alloc::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    claimed: *const AtomicI64,
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
        let buffer_raw = nexus.buffer_raw;
        Self {
            nexus,
            claimed,
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
    unsafe fn unsafe_send(&mut self, value: T) {
        let claimed = (*self.claimed).fetch_add(1, Ordering::Relaxed);
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.buffer_length_unsigned);

        let cell = self.buffer_raw.add(index);

        (*cell).claim_for_write();
        (*cell).write(value);
        (*cell).finish_write();
    }
}
