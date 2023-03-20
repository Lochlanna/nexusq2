use crate::{cell::Cell, FastMod, NexusQ};
use alloc::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug)]
pub struct Sender<T> {
    nexus: Arc<NexusQ<T>>,
    claimed: *const AtomicI64,
    tail: *const AtomicI64,
    published: *const AtomicI64,
    buffer_raw: *mut Cell<T>,
    buffer_length: i64,
    buffer_length_unsigned: usize,
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub(crate) fn new(nexus: Arc<NexusQ<T>>) -> Self {
        let buffer_length = nexus.buffer.len() as i64;
        let buffer_length_unsigned = nexus.buffer.len();
        let claimed = nexus.get_claimed();
        let tail = nexus.get_tail();
        let published = nexus.get_published();
        let buffer_raw = nexus.buffer_raw;
        Self {
            nexus,
            claimed,
            tail,
            published,
            buffer_raw,
            buffer_length,
            buffer_length_unsigned,
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
        let claimed = (*self.claimed).fetch_add(1, Ordering::Relaxed);
        debug_assert!(claimed >= 0);

        let index = (claimed as usize).fast_mod(self.buffer_length_unsigned);

        let cell = self.buffer_raw.add(index);

        while (*self.tail).load(Ordering::Acquire) < claimed - 1 {
            core::hint::spin_loop();
        }

        (*cell).wait_for_write();

        (*self.tail).store(claimed, Ordering::Release);

        (*cell).write(value);

        if claimed == 0 {
            (*cell).finish_write();
        }

        while (*self.published)
            .compare_exchange_weak(claimed - 1, claimed, Ordering::Release, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }
}
