#![allow(dead_code)]

mod hint;
mod producer_tracker;
mod reader_tracker;
mod receiver;
mod sender;
mod sync;
mod wait_strategy;

#[cfg(test)]
mod thread;

#[cfg(all(test, loom))]
mod loom_tests;

#[cfg(test)]
mod test_shared;

use crate::receiver::Receiver;
use crate::sender::Sender;
use crate::sync::Arc;
use producer_tracker::ProducerTracker;
use reader_tracker::ReaderTracker;

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
        let current = self.producer_tracker.current_published();
        unsafe {
            if current < 0 {
                (*self.buffer).set_len(0);
            } else if (current as usize) < self.length {
                (*self.buffer).set_len(current as usize);
            }
            drop(Box::from_raw(self.buffer));
        }
    }
}

impl<T> NexusQ<T> {
    fn new(size: usize) -> Self {
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
        let (sender, mut receiver) = make_channel(5);

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
        test(1, 1, 100, 5);
    }

    #[test]
    #[ignore]
    fn one_sender_one_receiver_long() {
        test(1, 1, 50000000, 5);
    }

    #[test]
    fn one_sender_two_receiver() {
        test(1, 2, 100, 5);
    }

    #[test]
    #[ignore]
    fn one_sender_two_receiver_long() {
        test(1, 2, 5000000, 5);
    }

    #[test]
    fn two_sender_one_receiver() {
        test(2, 1, 100, 5);
    }

    #[test]
    fn two_sender_two_receiver() {
        test(2, 2, 100, 5);
    }

    #[test]
    fn two_sender_two_receiver_long() {
        test(2, 2, 500000, 5);
    }

    #[test]
    #[ignore]
    #[cfg_attr(miri, ignore)]
    fn two_sender_two_receiver_stress() {
        for i in 0..1000 {
            println!("run {}", i);
            test(2, 2, 1000, 5);
        }
    }
}
