use crate::{FastMod, NexusQ};
use alloc::sync::Arc;

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    buffer_length: i64,
    tail_cache: i64,
}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer_length = nexus.buffer.len() as i64;
        Self {
            nexus,
            buffer_length,
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
        let producer_tracker = &self.nexus.producer_tracker;

        let claimed = producer_tracker.claim();
        debug_assert!(claimed >= 0);

        if claimed >= self.buffer_length {
            let expected_tail = claimed - self.buffer_length + 1;
            if self.tail_cache < expected_tail {
                self.tail_cache = self.nexus.reader_tracker.wait_for_tail(expected_tail);
            }
        }

        let index = (claimed as usize).fast_mod(self.buffer_length as usize);

        let mut old_value: Option<T> = None;
        unsafe {
            let cell = self.nexus.buffer_raw.add(index);
            if claimed < self.buffer_length {
                cell.write(value);
            } else {
                old_value = Some(cell.replace(value));
            }
        }

        // Notify other threads that a value has been written
        producer_tracker.publish(claimed);

        // This will ensure that the compiler doesn't do this earlier for some reason (it probably wouldn't anyway)
        drop(old_value);
    }
}
