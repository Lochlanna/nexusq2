use crate::NexusQ;
use std::sync::Arc;

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        Self { nexus }
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
    pub fn send(&self, value: T) {
        let claimed = self.nexus.producer_tracker.claim();
        debug_assert!(claimed >= 0);

        if claimed >= (self.nexus.capacity as i64) {
            let tail = claimed - (self.nexus.capacity as i64);
            self.nexus.reader_tracker.wait_for_tail(tail + 1);
        }

        let index = (claimed as usize) % self.nexus.capacity;

        let mut old_value: Option<T> = None;
        unsafe {
            if claimed < (self.nexus.capacity as i64) {
                core::ptr::copy_nonoverlapping(
                    &value,
                    (*self.nexus.buffer).get_unchecked_mut(index),
                    1,
                );
                std::mem::forget(value);
            } else {
                old_value = Some(core::mem::replace(
                    (*self.nexus.buffer).get_unchecked_mut(index),
                    value,
                ));
            }
        }

        // Notify other threads that a value has been written
        self.nexus.producer_tracker.publish(claimed);

        // This will ensure that the compiler doesn't do this earlier for some reason (it probably wouldn't anyway)
        drop(old_value);
    }
}
