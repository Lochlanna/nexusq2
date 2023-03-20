use crate::{cell::Cell, FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Receiver<T> {
    nexus: Arc<NexusQ<T>>,
    buffer_raw: *mut Cell<T>,
    published_cache: i64,
    buffer_length: usize,
    cursor: i64,
}

unsafe impl<T> Send for Receiver<T> {}

impl<T> Receiver<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer_length = nexus.buffer.len();
        let buffer_raw = nexus.buffer_raw;
        Self::register(buffer_raw);
        Self {
            nexus,
            buffer_raw,
            published_cache: -1,
            buffer_length,
            cursor: 0,
        }
    }
    fn register(buffer: *mut Cell<T>) {
        unsafe {
            (*buffer).initial_queue_for_read();
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        if self.cursor == 0 {
            unsafe {
                let cell = self.buffer_raw.add(self.cursor as usize);
                (*cell).initial_queue_for_read();
            }
        } else {
            unsafe {
                let cell = self.buffer_raw.add(self.cursor as usize - 1);
                (*cell).claim_for_read();
            }
        }
        Self {
            nexus: Arc::clone(&self.nexus),
            buffer_raw: self.buffer_raw,
            published_cache: self.published_cache,
            buffer_length: self.buffer_length,
            cursor: self.cursor,
        }
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
        let previous_index = ((self.cursor - 1) as usize).fast_mod(self.buffer_length);
        let current_index = (self.cursor as usize).fast_mod(self.buffer_length);
        let current_cell = self.buffer_raw.add(current_index);

        (*current_cell).wait_for_published(self.cursor);

        if self.cursor > 0 {
            (*current_cell).claim_for_read();
            let previous_cell = self.buffer_raw.add(previous_index);
            (*previous_cell).finish_read();
        }
        self.cursor += 1;

        (*current_cell).read()
    }
}
