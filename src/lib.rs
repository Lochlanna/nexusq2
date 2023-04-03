#![warn(future_incompatible)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(dead_code)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![deny(clippy::disallowed_types)]

extern crate alloc;

mod cell;
mod receiver;
mod sender;
mod wait_strategy;

use alloc::sync::Arc;
use alloc::vec::Vec;
use portable_atomic::{AtomicPtr, AtomicUsize};

use crate::wait_strategy::HybridWait;
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

impl FastMod for u64 {
    fn fast_mod(&self, denominator: Self) -> Self {
        debug_assert!(denominator > 0);
        debug_assert!(denominator.is_power_of_two());
        *self & (denominator - 1)
    }

    fn maybe_next_power_of_two(&self) -> Self {
        self.next_power_of_two()
    }
}

#[derive(Debug)]
struct NexusDetails<T> {
    claimed: *const AtomicUsize,
    tail: *const AtomicPtr<cell::Cell<T>>,
    tail_wait_strategy: *const HybridWait,
    buffer_raw: *mut cell::Cell<T>,
    buffer_length: usize,
}

// Why can't we derive this?
impl<T> Copy for NexusDetails<T> {}

// Why can't we derive this?
impl<T> Clone for NexusDetails<T> {
    fn clone(&self) -> Self {
        Self {
            claimed: self.claimed,
            tail: self.tail,
            tail_wait_strategy: self.tail_wait_strategy,
            buffer_raw: self.buffer_raw,
            buffer_length: self.buffer_length,
        }
    }
}

unsafe impl<T> Send for NexusDetails<T> {}

#[derive(Debug)]
struct NexusQ<T> {
    buffer: Vec<cell::Cell<T>>,
    buffer_raw: *mut cell::Cell<T>,
    claimed: AtomicUsize,
    tail: AtomicPtr<cell::Cell<T>>,
    tail_wait_strategy: HybridWait,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for NexusQ<T> {}
unsafe impl<T> Sync for NexusQ<T> {}

impl<T> NexusQ<T> {
    fn new(size: usize) -> Self {
        let size = size.maybe_next_power_of_two();
        let mut buffer = Vec::with_capacity(size);
        buffer.resize_with(size, cell::Cell::default);

        let buffer_raw = buffer.as_mut_ptr();

        unsafe {
            Self {
                buffer,
                buffer_raw,
                claimed: AtomicUsize::new(1),
                tail: AtomicPtr::new(buffer_raw.add(1)),
                tail_wait_strategy: HybridWait::default(),
            }
        }
    }

    pub(crate) fn get_details(&self) -> NexusDetails<T> {
        NexusDetails {
            claimed: &self.claimed,
            tail: &self.tail,
            tail_wait_strategy: &self.tail_wait_strategy,
            buffer_raw: self.buffer_raw,
            buffer_length: self.buffer.len(),
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
        let (sender, mut receiver) = make_channel(4);
        sender.send(1);
        sender.send(2);
        sender.send(3);
        assert_eq!(receiver.recv(), 1);
        assert_eq!(receiver.recv(), 2);
        assert_eq!(receiver.recv(), 3);
        sender.send(4);
        sender.send(5);
        sender.send(6);
        assert_eq!(receiver.recv(), 4);
        assert_eq!(receiver.recv(), 5);
        assert_eq!(receiver.recv(), 6);
    }

    #[test]
    fn basic_channel_test_try() {
        let (sender, mut receiver) = make_channel(4);
        sender.try_send(1).unwrap();
        sender.try_send(2).unwrap();
        sender.try_send(3).unwrap();
        assert_eq!(receiver.recv(), 1);
        assert_eq!(receiver.recv(), 2);
        assert_eq!(receiver.recv(), 3);
        sender.try_send(4).unwrap();
        sender.try_send(5).unwrap();
        sender.try_send(6).unwrap();
        assert_eq!(receiver.recv(), 4);
        assert_eq!(receiver.recv(), 5);
        assert_eq!(receiver.recv(), 6);
    }

    #[test]
    #[should_panic]
    fn try_send_to_full_fails() {
        let (sender, _) = make_channel(4);
        sender.try_send(1).unwrap();
        sender.try_send(2).unwrap();
        sender.try_send(3).unwrap();
        sender.try_send(4).unwrap();
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;
    use portable_atomic::{AtomicU64, Ordering};
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
        let (sender, mut receiver) = make_channel(16);
        for _ in 0..15 {
            sender.send(CustomDropper::new(&counter));
        }
        receiver.recv();
        sender.send(CustomDropper::new(&counter));
        drop(receiver);
        drop(sender);
        assert_eq!(counter.load(Ordering::Relaxed), 17);
    }

    #[test]
    fn valid_drop_partial_buffer() {
        let counter = Arc::default();
        let (sender, _) = make_channel(16);
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
        let (sender, mut receiver) = make_channel::<CustomDropper>(4);
        sender.send(CustomDropper::new(&counter));
        sender.send(CustomDropper::new(&counter));
        sender.send(CustomDropper::new(&counter));
        receiver.recv();
        receiver.recv();
        receiver.recv();
        assert_eq!(counter.load(Ordering::Acquire), 3);
        sender.send(CustomDropper::new(&counter));
        assert_eq!(counter.load(Ordering::Acquire), 3);
        sender.send(CustomDropper::new(&counter));
        assert_eq!(counter.load(Ordering::Acquire), 4);
        sender.send(CustomDropper::new(&counter));
        assert_eq!(counter.load(Ordering::Acquire), 5);
        //TODO fix this so that it can be filled!
        // sender.send(CustomDropper::new(&counter));
        // assert_eq!(counter.load(Ordering::Relaxed), 8);
        drop(sender);
        drop(receiver);
        assert_eq!(counter.load(Ordering::Acquire), 9);
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
