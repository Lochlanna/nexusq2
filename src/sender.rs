use crate::producer_tracker::ProducerTracker;
use crate::{Cell, FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    producer_tracker: *const ProducerTracker,
    buffer_raw: *mut Cell<T>,
    buffer_length: i64,
    buffer_length_unsigned: usize,
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer_length = nexus.buffer.len() as i64;
        let buffer_length_unsigned = nexus.buffer.len();
        let producer_tracker = std::ptr::addr_of!(nexus.producer_tracker);
        let buffer_raw = nexus.buffer_raw;
        Self {
            nexus,
            producer_tracker,
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
        let claimed = (*self.producer_tracker).claim();
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.buffer_length_unsigned);

        let cell = self.buffer_raw.add(index);

        if claimed >= self.buffer_length {
            // this cell has been written to before so we will need to handle dropping it
            (*cell).replace(value);
        } else {
            // this is uninit memory so we are safe to just overwrite it!
            (*cell).write(value);
        }

        (*self.producer_tracker).publish(claimed);
    }

    pub fn try_send(&mut self, value: T) -> bool {
        unsafe { self.try_unsafe_send(value) }
    }
    unsafe fn try_unsafe_send(&mut self, value: T) -> bool {
        let claimed = (*self.producer_tracker).claim();
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.buffer_length_unsigned);

        let cell = self.buffer_raw.add(index);

        if claimed >= self.buffer_length {
            // this cell has been written to before so we will need to handle dropping it
            if !(*cell).try_replace(value) {
                return false;
            }
        } else {
            // this is uninit memory so we are safe to just overwrite it!
            (*cell).write(value);
        }

        (*self.producer_tracker).publish(claimed);
        true
    }
}
