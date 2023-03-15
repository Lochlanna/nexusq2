use crate::sync::Arc;
use crate::NexusQ;

#[derive(Debug)]
pub struct Receiver<T> {
    cursor: i64,
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
        debug_assert!(self.cursor >= 0);

        self.nexus.producer_tracker.wait_for_publish(self.cursor);

        let index = (self.cursor as usize) % self.nexus.length;

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
            .update_position(self.cursor - 1, self.cursor);
        self.cursor += 1;

        value
    }
}
