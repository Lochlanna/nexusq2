use crate::producer_tracker::ProducerTracker;
use crate::reader_tracker::ReaderTracker;
use crate::{FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Receiver<T> {
    nexus: Arc<NexusQ<T>>,
    producer_tracker: *const ProducerTracker,
    reader_tracker: *const ReaderTracker,
    buffer_raw: *mut T,
    buffer_length: usize,
    cursor: i64,
    published_cache: i64,
}

unsafe impl<T> Send for Receiver<T> {}

impl<T> Receiver<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        nexus.reader_tracker.register();
        let buffer_length = nexus.buffer.len();
        let producer_tracker = std::ptr::addr_of!(nexus.producer_tracker);
        let reader_tracker = std::ptr::addr_of!(nexus.reader_tracker);
        let buffer_raw = nexus.buffer_raw;
        Self {
            nexus,
            producer_tracker,
            reader_tracker,
            buffer_raw,
            buffer_length,
            cursor: -1,
            published_cache: -1,
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self::new(Arc::clone(&self.nexus))
    }
}

impl<T> Receiver<T>
where
    T: Clone,
{
    pub fn recv(&mut self) -> T {
        unsafe { self.unsafe_recv() }
    }
    unsafe fn unsafe_recv(&mut self) -> T {
        self.cursor += 1;
        debug_assert!(self.cursor >= 0);

        if self.cursor > self.published_cache {
            self.published_cache = (*self.producer_tracker).wait_for_publish(self.cursor);
        }

        (*self.reader_tracker).update_position(self.cursor - 1, self.cursor);

        let index = (self.cursor as usize).fast_mod(self.buffer_length);

        unsafe {
            let cell = self.buffer_raw.add(index);
            (*cell).clone()
        }
    }
}
