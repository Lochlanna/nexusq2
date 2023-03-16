#![allow(dead_code)]

extern crate alloc;

mod producer_tracker;
mod reader_tracker;
mod receiver;
mod sender;
mod wait_strategy;

#[cfg(test)]
mod bench_test;

#[cfg(test)]
mod test_shared;

use alloc::sync::Arc;
use producer_tracker::ProducerTracker;
use reader_tracker::ReaderTracker;

pub use receiver::Receiver;
pub use sender::Sender;

pub trait FastMod {
    fn fast_mod(&self, denominator: Self) -> Self;
    fn maybe_next_power_of_two(&self) -> Self;
}

impl FastMod for usize {
    #[cfg(feature = "fast-mod")]
    fn fast_mod(&self, denominator: Self) -> Self {
        debug_assert!(denominator > 0);
        debug_assert!(denominator.is_power_of_two());
        *self & (denominator - 1)
    }

    #[cfg(not(feature = "fast-mod"))]
    fn fast_mod(&self, denominator: Self) -> Self {
        *self % denominator
    }

    #[cfg(feature = "fast-mod")]
    fn maybe_next_power_of_two(&self) -> Self {
        self.next_power_of_two()
    }

    #[cfg(not(feature = "fast-mod"))]
    fn maybe_next_power_of_two(&self) -> Self {
        *self
    }
}

#[derive(Debug)]
struct NexusQ<T> {
    length: usize,
    buffer: *mut Vec<T>,
    producer_tracker: ProducerTracker,
    reader_tracker: ReaderTracker,
}

unsafe impl<T> Send for NexusQ<T> {}
unsafe impl<T> Sync for NexusQ<T> {}

impl<T> Drop for NexusQ<T> {
    fn drop(&mut self) {
        let current_length = (self.producer_tracker.current_published() + 1) as usize;
        unsafe {
            // This ensures that drop is run correctly for all valid items in the buffer and also not run on uninitialised memory!
            if current_length < self.length {
                (*self.buffer).set_len(current_length);
            }
            drop(Box::from_raw(self.buffer));
        }
    }
}

impl<T> NexusQ<T> {
    fn new(size: usize) -> Self {
        let size = size.maybe_next_power_of_two();
        let mut buffer = Box::new(Vec::with_capacity(size));
        unsafe {
            buffer.set_len(size);
        }

        let buffer = Box::into_raw(buffer);

        Self {
            length: size,
            buffer,
            producer_tracker: Default::default(),
            reader_tracker: ReaderTracker::new(size),
        }
    }
}

pub fn make_channel<T>(size: usize) -> (Sender<T>, Receiver<T>) {
    let nexus = Arc::new(NexusQ::new(size));
    let receiver = Receiver::new(Arc::clone(&nexus));
    let sender = Sender::new(nexus);
    (sender, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_shared::*;

    #[test]
    fn basic_tracker_test() {
        setup_logging();
        let nexus = NexusQ::<usize>::new(10);
        assert_eq!(nexus.producer_tracker.current_published(), -1);
        assert_eq!(nexus.producer_tracker.claim(), 0);
        nexus.producer_tracker.publish(0);
        assert_eq!(nexus.producer_tracker.current_published(), 0);
        nexus.producer_tracker.publish(1);
        assert_eq!(nexus.producer_tracker.current_published(), 1);

        assert_eq!(nexus.reader_tracker.current_tail_position(), 0);
        nexus.reader_tracker.register();
        nexus.reader_tracker.update_position(0, 1);
        assert_eq!(nexus.reader_tracker.current_tail_position(), 1);
        nexus.reader_tracker.update_position(1, 2);
        assert_eq!(nexus.reader_tracker.current_tail_position(), 2);
    }

    #[test]
    fn basic_channel_test() {
        setup_logging();
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
    fn one_sender_one_receiver() {
        setup_logging();
        test(1, 1, 100, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn one_sender_one_receiver_long() {
        setup_logging();
        test(1, 1, 500000, 5);
    }

    #[test]
    fn one_sender_two_receiver() {
        setup_logging();
        test(1, 2, 100, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn one_sender_two_receiver_long() {
        setup_logging();
        test(1, 2, 500000, 5);
    }

    #[test]
    fn two_sender_one_receiver() {
        setup_logging();
        test(2, 1, 100, 5);
    }

    #[test]
    fn two_sender_two_receiver() {
        setup_logging();
        test(2, 2, 100, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn two_sender_two_receiver_long() {
        setup_logging();
        test(2, 2, 10000, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn two_sender_two_receiver_stress() {
        setup_logging();
        for i in 0..1000 {
            println!("run {}", i);
            test(2, 2, 1000, 5);
        }
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;
    use crate::test_shared::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Debug, Clone)]
    struct CustomDropper {
        value: i64,
        counter: Arc<AtomicU64>,
    }
    impl CustomDropper {
        fn new(counter: &Arc<AtomicU64>) -> Self {
            Self {
                value: 42424242,
                counter: Arc::clone(counter),
            }
        }
    }
    impl Drop for CustomDropper {
        fn drop(&mut self) {
            assert_eq!(self.value, 42424242);
            self.counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn valid_drop_full_buffer() {
        setup_logging();
        let counter = Default::default();
        let (mut sender, _) = make_channel(10);
        for _ in 0..10 {
            sender.send(CustomDropper::new(&counter));
        }
        drop(sender);
        assert_eq!(counter.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn valid_drop_partial_buffer() {
        setup_logging();
        let counter = Default::default();
        let (mut sender, _) = make_channel(10);
        for _ in 0..3 {
            sender.send(CustomDropper::new(&counter));
        }
        drop(sender);
        assert_eq!(counter.load(Ordering::Acquire), 3);
    }

    #[test]
    fn valid_drop_empty_buffer() {
        setup_logging();
        let (sender, _) = make_channel::<CustomDropper>(10);
        drop(sender);
    }

    #[test]
    fn valid_drop_overwrite() {
        setup_logging();
        let counter = Default::default();
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
