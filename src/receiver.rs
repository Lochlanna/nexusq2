use crate::{cell::Cell, FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Receiver<T> {
    nexus: Arc<NexusQ<T>>,
    buffer_raw: *mut Cell<T>,
    buffer_length: usize,
    cursor: i64,
}

unsafe impl<T> Send for Receiver<T> {}

impl<T> Receiver<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer_length = nexus.buffer.len();
        let buffer_raw = nexus.buffer_raw;
        // Self::register(buffer_raw);
        Self {
            nexus,
            buffer_raw,
            buffer_length,
            cursor: -1,
        }
    }
    fn register(buffer: *mut Cell<T>) {
        unsafe {
            (*buffer).move_to();
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
        let old_index = (self.cursor as usize).fast_mod(self.buffer_length);

        self.cursor += 1;
        let new_index = (self.cursor as usize).fast_mod(self.buffer_length);
        let new_cell = self.buffer_raw.add(new_index);

        (*new_cell).move_to();

        if self.cursor > 0 {
            let old_cell = self.buffer_raw.add(old_index);
            (*old_cell).move_from();
        }

        let value;
        unsafe {
            value = (*new_cell).clone_value();
        }

        value
    }
}
