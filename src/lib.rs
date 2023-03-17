#![cfg_attr(not(feature = "std"), no_std)]
#![allow(dead_code)]
#![warn(future_incompatible)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod producer_tracker;
mod reader_tracker;
mod receiver;
mod sender;
mod wait_strategy;

use alloc::sync::Arc;
use alloc::vec::Vec;
use producer_tracker::ProducerTracker;
use std::sync::atomic::{AtomicI64, Ordering};

pub use receiver::Receiver;
pub use sender::Sender;

pub trait FastMod {
    #[must_use]
    fn fast_mod(&self, denominator: Self) -> Self;
    #[must_use]
    fn maybe_next_power_of_two(&self) -> Self;
}

#[cfg(feature = "fast-mod")]
impl FastMod for usize {
    fn fast_mod(&self, denominator: Self) -> Self {
        debug_assert!(denominator > 0);
        debug_assert!(denominator.is_power_of_two());
        *self & (denominator - 1)
    }

    fn maybe_next_power_of_two(&self) -> Self {
        self.next_power_of_two()
    }
}

#[cfg(not(feature = "fast-mod"))]
impl FastMod for usize {
    fn fast_mod(&self, denominator: Self) -> Self {
        *self % denominator
    }
    fn maybe_next_power_of_two(&self) -> Self {
        *self
    }
}

#[derive(Debug)]
struct Cell<T> {
    value: T,
    counter: AtomicI64,
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        unsafe {
            Self {
                value: core::mem::MaybeUninit::zeroed().assume_init(),
                counter: AtomicI64::new(-1),
            }
        }
    }
}

impl<T> Cell<T> {
    fn take_cell_for_write(&self) {
        debug_assert!(self.counter.load(Ordering::Acquire) >= 0);
        while self
            .counter
            .compare_exchange_weak(0, -1, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn try_take_cell_for_write(&self) -> bool {
        debug_assert!(self.counter.load(Ordering::Acquire) >= 0);
        self.counter
            .compare_exchange(0, -1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    pub fn publish_cell(&self) {
        debug_assert_eq!(self.counter.load(Ordering::Acquire), -1);
        self.counter.store(0, Ordering::Release);
    }

    pub fn move_to(&self) {
        while self.counter.load(Ordering::Acquire) < 0 {
            core::hint::spin_loop();
        }
        debug_assert!(self.counter.load(Ordering::Acquire) >= 0);
        self.counter.fetch_add(1, Ordering::Release);
    }

    pub fn move_from(&self) {
        debug_assert!(self.counter.load(Ordering::Acquire) > 0);
        self.counter.fetch_sub(1, Ordering::Release);
    }

    pub unsafe fn replace(&mut self, value: T) {
        self.take_cell_for_write();
        let old = core::ptr::replace(&mut self.value, value);
        self.publish_cell();
        drop(old);
    }

    pub unsafe fn try_replace(&mut self, value: T) -> bool {
        if !self.try_take_cell_for_write() {
            return false;
        }
        let old = core::ptr::replace(&mut self.value, value);
        self.publish_cell();
        drop(old);
        true
    }

    pub unsafe fn write(&mut self, value: T) {
        core::ptr::write(&mut self.value, value);
        self.publish_cell();
    }
}

#[derive(Debug)]
struct NexusQ<T> {
    buffer: Vec<Cell<T>>,
    buffer_raw: *mut Cell<T>,
    producer_tracker: ProducerTracker,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for NexusQ<T> {}
unsafe impl<T> Sync for NexusQ<T> {}

impl<T> Drop for NexusQ<T> {
    fn drop(&mut self) {
        let current_length = self.producer_tracker.current_published() + 1;
        let current_length = current_length.min(self.buffer.len() as i64) as usize;
        unsafe {
            // This ensures that drop is run correctly for all valid items in the buffer and also not run on uninitialised memory!
            self.buffer.set_len(current_length);
        }
    }
}

impl<T> NexusQ<T> {
    #[allow(clippy::uninit_vec)]
    fn new(size: usize) -> Self {
        let size = size.maybe_next_power_of_two();
        let mut buffer = Vec::with_capacity(size);
        buffer.resize_with(size, Cell::default);

        let buffer_raw = buffer.as_mut_ptr();

        Self {
            buffer,
            buffer_raw,
            producer_tracker: ProducerTracker::default(),
        }
    }
}

#[must_use]
pub fn make_channel<T>(size: usize) -> (Sender<T>, Receiver<T>) {
    let nexus = Arc::new(NexusQ::new(size));
    let receiver = Receiver::new(Arc::clone(&nexus));
    let sender = Sender::new(nexus);
    (sender, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions_sorted::assert_eq;

    #[test]
    fn basic_channel_test() {
        let (mut sender, mut receiver) = make_channel(5);
        sender.send(1);
        sender.send(2);
        sender.send(3);
        sender.send(4);
        sender.send(5);
        assert_eq!(receiver.recv(), 1);
        assert_eq!(receiver.recv(), 2);
        assert_eq!(receiver.recv(), 3);
        assert_eq!(receiver.recv(), 4);
        assert_eq!(receiver.recv(), 5);
    }

    #[test]
    fn cant_overwrite() {
        let (mut sender, mut receiver) = make_channel(5);
        sender.send(1);
        sender.send(2);
        sender.send(3);
        sender.send(4);
        sender.send(5);
        assert_eq!(receiver.recv(), 1);
        sender.send(6);
        assert!(!sender.try_send(7));
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;
    use core::sync::atomic::{AtomicU64, Ordering};
    use pretty_assertions_sorted::assert_eq;

    #[derive(Debug, Clone)]
    struct CustomDropper {
        value: i64,
        counter: Arc<AtomicU64>,
    }
    impl CustomDropper {
        fn new(counter: &Arc<AtomicU64>) -> Self {
            Self {
                value: 42_424_242,
                counter: Arc::clone(counter),
            }
        }
    }
    impl Drop for CustomDropper {
        fn drop(&mut self) {
            assert_eq!(self.value, 42_424_242);
            self.counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn valid_drop_full_buffer() {
        let counter = Arc::default();
        let (mut sender, _) = make_channel(10);
        for _ in 0..10 {
            sender.send(CustomDropper::new(&counter));
        }
        drop(sender);
        assert_eq!(counter.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn valid_drop_partial_buffer() {
        let counter = Arc::default();
        let (mut sender, _) = make_channel(10);
        for _ in 0..3 {
            sender.send(CustomDropper::new(&counter));
        }
        drop(sender);
        assert_eq!(counter.load(Ordering::Acquire), 3);
    }

    #[test]
    fn valid_drop_empty_buffer() {
        let (sender, _) = make_channel::<CustomDropper>(10);
        drop(sender);
    }

    #[test]
    fn valid_drop_overwrite() {
        let counter = Arc::default();
        let (mut sender, mut receiver) = make_channel::<CustomDropper>(4);
        sender.send(CustomDropper::new(&counter));
        sender.send(CustomDropper::new(&counter));
        sender.send(CustomDropper::new(&counter));
        sender.send(CustomDropper::new(&counter));
        receiver.recv();
        receiver.recv();
        receiver.recv();
        receiver.recv();
        assert_eq!(counter.load(Ordering::Acquire), 4);
        sender.send(CustomDropper::new(&counter));
        assert_eq!(counter.load(Ordering::Acquire), 5);
        sender.send(CustomDropper::new(&counter));
        assert_eq!(counter.load(Ordering::Acquire), 6);
        sender.send(CustomDropper::new(&counter));
        assert_eq!(counter.load(Ordering::Acquire), 7);
        //TODO fix this so that it can be filled!
        // sender.send(CustomDropper::new(&counter));
        // assert_eq!(counter.load(Ordering::Relaxed), 8);
        drop(sender);
        drop(receiver);
        assert_eq!(counter.load(Ordering::Acquire), 11);
    }
}

#[cfg(test)]
mod fast_mod_tests {
    use super::*;
    use pretty_assertions_sorted::assert_eq;

    #[test]
    fn test_fast_mod() {
        for n in 0..1024 {
            let mut d = 1_usize;
            while d <= 1024_usize.next_power_of_two() {
                d = d.next_power_of_two();
                let expected = n % d;
                let fast_mod = n.fast_mod(d);
                assert_eq!(expected, fast_mod);
                d += 1;
            }
        }
    }
}
