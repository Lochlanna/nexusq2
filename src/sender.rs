use crate::producer_tracker::ProducerTracker;
use crate::reader_tracker::ReaderTracker;
use crate::{FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    producer_tracker: *const ProducerTracker,
    reader_tracker: *const ReaderTracker,
    buffer_raw: *mut T,
    buffer_length: i64,
    buffer_length_unsigned: usize,
    tail_cache: i64,
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer_length = nexus.buffer.len() as i64;
        let buffer_length_unsigned = nexus.buffer.len();
        let producer_tracker = std::ptr::addr_of!(nexus.producer_tracker);
        let reader_tracker = std::ptr::addr_of!(nexus.reader_tracker);
        let buffer_raw = nexus.buffer_raw;
        Self {
            nexus,
            producer_tracker,
            reader_tracker,
            buffer_raw,
            buffer_length,
            buffer_length_unsigned,
            tail_cache: 0,
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

        if claimed >= self.buffer_length {
            let expected_tail = claimed - self.buffer_length + 1;
            if self.tail_cache < expected_tail {
                self.tail_cache = (*self.reader_tracker).wait_for_tail(expected_tail);
            }
        }

        let index = (claimed as usize).fast_mod(self.buffer_length_unsigned);

        let mut old_value: Option<T> = None;
        let cell = self.buffer_raw.add(index);
        if claimed < self.buffer_length {
            cell.write(value);
        } else {
            old_value = Some(cell.replace(value));
        }

        // Notify other threads that a value has been written
        (*self.producer_tracker).publish(claimed);

        // This will ensure that the compiler doesn't do this earlier for some reason (it probably wouldn't anyway)
        drop(old_value);
    }
}
