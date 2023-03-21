#![warn(future_incompatible)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(dead_code)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

extern crate alloc;

mod cell;
mod receiver;
mod sender;
mod wait_strategy;

use alloc::sync::Arc;
use alloc::vec::Vec;
use std::sync::atomic::{AtomicI64, Ordering};

pub use receiver::Receiver;
pub use sender::{Sender, TrySendError};

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
struct NexusQ<T> {
    buffer: Vec<cell::Cell<T>>,
    buffer_raw: *mut cell::Cell<T>,
    claimed: AtomicI64,
    tail: AtomicI64,
}

impl<T> Drop for NexusQ<T> {
    fn drop(&mut self) {
        if self.claimed.load(Ordering::SeqCst) >= self.buffer.len() as i64 {
            // just do the drop!
            return;
        }
        for cell in self.buffer.drain(..) {
            if cell.should_drop() {
                core::mem::drop(cell);
            } else {
                core::mem::forget(cell);
            }
        }
    }
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

        Self {
            buffer,
            buffer_raw,
            claimed: AtomicI64::default(),
            tail: AtomicI64::new(-1),
        }
    }

    pub(crate) const fn get_claimed(&self) -> *const AtomicI64 {
        &self.claimed
    }
    pub(crate) const fn get_tail(&self) -> *const AtomicI64 {
        &self.tail
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
        sender.send(6);
        assert_eq!(receiver.recv(), 6);
    }

    #[test]
    fn basic_channel_test_try() {
        let (mut sender, mut receiver) = make_channel(5);
        sender.try_send(1).unwrap();
        sender.try_send(2).unwrap();
        sender.try_send(3).unwrap();
        sender.try_send(4).unwrap();
        sender.try_send(5).unwrap();
        assert_eq!(receiver.recv(), 1);
        assert_eq!(receiver.recv(), 2);
        assert_eq!(receiver.recv(), 3);
        assert_eq!(receiver.recv(), 4);
        assert_eq!(receiver.recv(), 5);
        sender.try_send(6).unwrap();
        assert_eq!(receiver.recv(), 6);
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
        let (mut sender, _) = make_channel(16);
        for _ in 0..16 {
            sender.send(CustomDropper::new(&counter));
        }
        drop(sender);
        assert_eq!(counter.load(Ordering::Relaxed), 16);
    }

    #[test]
    fn valid_drop_partial_buffer() {
        let counter = Arc::default();
        let (mut sender, _) = make_channel(16);
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
