#![allow(dead_code)]

mod producer_tracker;
mod reader_tracker;
mod receiver;
mod sender;
mod wait_strategy;

use crate::receiver::Receiver;
use crate::sender::Sender;
use producer_tracker::ProducerTracker;
use reader_tracker::ReaderTracker;
use std::sync::Arc;

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
mod tracker_tests {
    use super::*;
    use std::collections::HashMap;
    use std::thread;
    use std::time::Duration;

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

    fn test(num_senders: usize, num_receivers: usize, num: usize, buffer_size: usize) {
        let (sender, receiver) = make_channel(buffer_size);

        let mut receivers: Vec<_> = (0..(num_receivers - 1)).map(|_| receiver.clone()).collect();
        receivers.push(receiver);

        let mut senders: Vec<_> = (0..(num_senders - 1)).map(|_| sender.clone()).collect();
        senders.push(sender);

        let receivers: Vec<_> = receivers
            .into_iter()
            .map(|mut receiver| {
                thread::spawn(move || {
                    let mut values = Vec::with_capacity(num);
                    for _ in 0..(num * num_senders) {
                        let v = receiver.recv();
                        values.push(v);
                    }
                    values
                })
            })
            .collect();
        thread::sleep(Duration::from_secs_f64(0.01));

        senders.into_iter().for_each(|sender| {
            thread::spawn(move || {
                for i in 0..num {
                    sender.send(i);
                }
            });
        });

        let mut expected_map = HashMap::with_capacity(num);
        for i in 0..num {
            expected_map.insert(i, num_senders);
        }

        let results: Vec<_> = receivers
            .into_iter()
            .map(|jh| jh.join().expect("couldn't join read thread"))
            .map(|result| {
                let mut count_map: HashMap<usize, usize> = HashMap::with_capacity(num);
                for v in result {
                    let e = count_map.entry(v).or_default();
                    *e += 1;
                }
                let mut diff_map = HashMap::new();
                for (k, v) in count_map {
                    let expected_count = expected_map
                        .get(&k)
                        .expect("key didn't exist in expected map");
                    if *expected_count != v {
                        diff_map.insert(k, v);
                    }
                }
                diff_map
            })
            .collect();

        results.iter().for_each(|result| {
            println!("diff: {:?}", result);
            assert!(result.is_empty())
        });
    }

    #[test]
    fn one_sender_one_receiver() {
        test(1, 1, 100, 5);
    }

    #[test]
    fn one_sender_two_receiver() {
        test(1, 2, 100, 5);
    }

    #[test]
    fn two_sender_one_receiver() {
        test(2, 1, 100, 5);
    }

    #[test]
    fn two_sender_two_receiver() {
        test(2, 2, 1000, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn two_sender_two_receiver_stress() {
        for i in 0..1000 {
            println!("run {}", i);
            test(2, 2, 1000, 5);
        }
    }
}
