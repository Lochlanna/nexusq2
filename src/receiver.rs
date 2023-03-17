use crate::{FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Receiver<T> {
    cursor: i64,
    nexus: Arc<NexusQ<T>>,
    published_cache: i64,
    buffer_length: usize,
}

impl<T> Receiver<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        nexus.reader_tracker.register();
        let buffer_length = nexus.buffer.len();
        Self {
            cursor: -1,
            nexus,
            published_cache: -1,
            buffer_length,
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
        self.cursor += 1;
        debug_assert!(self.cursor >= 0);

        if self.cursor > self.published_cache {
            self.published_cache = self.nexus.producer_tracker.wait_for_publish(self.cursor);
        }

        self.nexus
            .reader_tracker
            .update_position(self.cursor - 1, self.cursor);

        let index = (self.cursor as usize).fast_mod(self.buffer_length);

        unsafe {
            let cell = self.nexus.buffer_raw.add(index);
            (*cell).clone()
        }
    }
}
