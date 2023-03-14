use crate::NexusQ;
use std::sync::Arc;

#[derive(Debug)]
pub struct Receiver<T> {
    cursor: usize,
    nexus: Arc<NexusQ<T>>,
}

impl<T> Receiver<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        nexus.reader_tracker.register();
        Self { cursor: 0, nexus }
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
        self.nexus
            .producer_tracker
            .wait_for_publish(self.cursor as i64);

        let index = self.cursor % self.nexus.capacity;

        let value;
        unsafe {
            if let Some(cell) = (*self.nexus.buffer).get(index) {
                value = cell.clone();
            } else {
                panic!("index out of bounds doing a read!")
            }
        }

        self.nexus
            .reader_tracker
            .update_position(self.cursor, self.cursor + 1);
        self.cursor += 1;

        value
    }
}
