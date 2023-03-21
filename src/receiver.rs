use crate::{cell::Cell, FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Receiver<T> {
    nexus: Arc<NexusQ<T>>,
    buffer_raw: *mut Cell<T>,
    buffer_length: usize,
    cursor: i64,
    previous_cell: *mut Cell<T>,
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
            buffer_length,
            cursor: 1,
            previous_cell: buffer_raw,
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
        unsafe {
            (*self.previous_cell).move_to();
        }
        Self {
            nexus: Arc::clone(&self.nexus),
            buffer_raw: self.buffer_raw,
            buffer_length: self.buffer_length,
            cursor: self.cursor,
            previous_cell: self.previous_cell,
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
        let current_index = (self.cursor as usize).fast_mod(self.buffer_length);
        let current_cell = self.buffer_raw.add(current_index);

        (*current_cell).wait_for_published(self.cursor);

        (*current_cell).move_to();
        (*self.previous_cell).move_from();

        self.previous_cell = current_cell;
        self.cursor += 1;

        (*current_cell).read()
    }
}
