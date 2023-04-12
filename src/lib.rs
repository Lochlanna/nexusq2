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
extern crate core;

mod cell;
mod receiver;
mod sender;
pub mod wait_strategy;

use alloc::sync::Arc;
use alloc::vec::Vec;
use portable_atomic::{AtomicPtr, AtomicUsize};
use thiserror::Error as ThisError;

pub use receiver::{Receiver, RecvError};
pub use sender::{SendError, Sender};
use wait_strategy::{HybridWait, Take, Wait};

pub(crate) trait FastMod {
    #[must_use]
    fn fast_mod(&self, denominator: Self) -> Self;
    #[must_use]
    fn maybe_next_power_of_two(&self) -> Self;
}

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

#[derive(Debug, ThisError, Eq, PartialEq, Copy, Clone)]
pub enum BufferError {
    #[error("nexusq channel buffers must be at least 2 elements")]
    BufferTooSmall,
    #[error("nexusq channel buffers cannot be larger than isize::MAX")]
    BufferTooLarge,
}

#[derive(Debug)]
struct NexusDetails<T> {
    claimed: *const AtomicUsize,
    tail: *const AtomicPtr<cell::Cell<T>>,
    tail_wait_strategy: *const dyn Take<AtomicPtr<cell::Cell<T>>>,
    buffer_raw: *mut cell::Cell<T>,
    buffer_length: usize,
    num_receivers: *const AtomicUsize,
}

impl<T> Copy for NexusDetails<T> {}

impl<T> Clone for NexusDetails<T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Debug)]
struct NexusQ<T> {
    buffer: Vec<cell::Cell<T>>,
    buffer_raw: *mut cell::Cell<T>,
    claimed: AtomicUsize,
    tail: AtomicPtr<cell::Cell<T>>,
    tail_wait_strategy: Box<dyn Take<AtomicPtr<cell::Cell<T>>>>,
    num_receivers: AtomicUsize,
}

impl<T> NexusQ<T> {
    fn new(size: usize) -> Result<Self, BufferError> {
        Self::with_strategies(size, HybridWait::default(), HybridWait::default)
    }

    fn with_strategies<W, R>(
        size: usize,
        writer_ws: W,
        reader_ws: impl Fn() -> R,
    ) -> Result<Self, BufferError>
    where
        W: Take<AtomicPtr<cell::Cell<T>>> + 'static,
        R: Wait<AtomicUsize> + 'static + Clone,
    {
        if size < 2 {
            return Err(BufferError::BufferTooSmall);
        }
        if size > isize::MAX as usize {
            // max size for vector!
            return Err(BufferError::BufferTooLarge);
        }

        let size = size.maybe_next_power_of_two();
        let mut buffer = Vec::with_capacity(size);
        buffer.resize_with(size, || cell::Cell::new(reader_ws()));

        let buffer_raw = buffer.as_mut_ptr();

        unsafe {
            Ok(Self {
                buffer,
                buffer_raw,
                claimed: AtomicUsize::new(1),
                tail: AtomicPtr::new(buffer_raw.add(1)),
                tail_wait_strategy: Box::new(writer_ws),
                num_receivers: AtomicUsize::new(0),
            })
        }
    }

    pub(crate) fn get_details(&self) -> NexusDetails<T> {
        NexusDetails {
            claimed: &self.claimed,
            tail: &self.tail,
            tail_wait_strategy: Box::as_ref(&self.tail_wait_strategy),
            buffer_raw: self.buffer_raw,
            buffer_length: self.buffer.len(),
            num_receivers: &self.num_receivers,
        }
    }
}

/// Create a new nexusq channel with a buffer of the given size.
/// This function will initialise the channel using the default [`HybridWait`] wait strategies
/// for both the sender and receiver.
///
/// # Arguments
///
/// * `size`: The size of the channel buffer. This must be at least 2, and no larger than [`isize::MAX`]
///
/// # Errors
/// - [`BufferError::BufferTooSmall`] if the buffer size is less than 2
/// - [`BufferError::BufferTooLarge`] if the buffer size is larger than [`isize::MAX`]
///
/// # Examples
///
/// ```
/// let (sender, mut receiver) = nexusq2::make_channel(4).expect("couldn't construct channel");
/// sender.send(42).expect("couldn't send");
/// sender.send(2).expect("couldn't send");
/// assert_eq!(receiver.recv(), 42);
/// assert_eq!(receiver.recv(), 2);
/// ```
pub fn make_channel<T>(size: usize) -> Result<(Sender<T>, Receiver<T>), BufferError> {
    make_channel_with(size, HybridWait::default(), HybridWait::default)
}

/// Create a new nexusq channel with a buffer of the given size and given wait strategies
///
/// # Arguments
///
/// * `size`: The size of the channel buffer. This must be at least 2, and no larger than [`isize::MAX`]
/// * `writer_ws`: An instance of a wait strategy for the writers to use to wait on each other
/// * `reader_ws`: A function that produces wait strategies which are used to wait on the readers
///
/// # Errors
/// - [`BufferError::BufferTooSmall`] if the buffer size is less than 2
/// - [`BufferError::BufferTooLarge`] if the buffer size is larger than [`isize::MAX`]
///
/// # Examples
///
/// ```
/// use nexusq2::wait_strategy::HybridWait;
/// let (sender, mut receiver) = nexusq2::make_channel_with(4, HybridWait::default(), HybridWait::default).expect("couldn't construct channel");
/// sender.send(42).expect("couldn't send");
/// assert_eq!(receiver.recv(), 42);
/// ```
pub fn make_channel_with<T, W, R>(
    size: usize,
    writer_ws: W,
    reader_ws: impl Fn() -> R,
) -> Result<(Sender<T>, Receiver<T>), BufferError>
where
    W: Take<AtomicPtr<cell::Cell<T>>> + 'static,
    R: Wait<AtomicUsize> + 'static + Clone,
{
    let nexus = Arc::new(NexusQ::with_strategies(size, writer_ws, reader_ws)?);
    let receiver = Receiver::new(Arc::clone(&nexus));
    let sender = Sender::new(nexus);
    Ok((sender, receiver))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use pretty_assertions_sorted::assert_eq;

    #[test]
    fn basic_channel() {
        let (sender, mut receiver) = make_channel(4).expect("couldn't construct channel");
        sender.send(1).expect("couldn't send");
        sender.send(2).expect("couldn't send");
        sender.send(3).expect("couldn't send");
        assert_eq!(receiver.recv(), 1);
        assert_eq!(receiver.recv(), 2);
        assert_eq!(receiver.recv(), 3);
        sender.send(4).expect("couldn't send");
        sender.send(5).expect("couldn't send");
        sender.send(6).expect("couldn't send");
        assert_eq!(receiver.recv(), 4);
        assert_eq!(receiver.recv(), 5);
        assert_eq!(receiver.recv(), 6);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn basic_channel_async() {
        let (mut sender, mut receiver) = make_channel(4).expect("couldn't construct channel");
        futures::sink::SinkExt::send(&mut sender, 1)
            .await
            .expect("couldn't send async");
        futures::sink::SinkExt::send(&mut sender, 2)
            .await
            .expect("couldn't send async");
        futures::sink::SinkExt::send(&mut sender, 3)
            .await
            .expect("couldn't send async");
        assert_eq!(receiver.next().await.expect("couldn't receive async"), 1);
        assert_eq!(receiver.next().await.expect("couldn't receive async"), 2);
        assert_eq!(receiver.next().await.expect("couldn't receive async"), 3);
        futures::sink::SinkExt::send(&mut sender, 4)
            .await
            .expect("couldn't send async");
        futures::sink::SinkExt::send(&mut sender, 5)
            .await
            .expect("couldn't send async");
        futures::sink::SinkExt::send(&mut sender, 6)
            .await
            .expect("couldn't send async");
        assert_eq!(receiver.next().await.expect("couldn't receive async"), 4);
        assert_eq!(receiver.next().await.expect("couldn't receive async"), 5);
        assert_eq!(receiver.next().await.expect("couldn't receive async"), 6);
    }

    #[test]
    fn basic_channel_try() {
        let (sender, mut receiver) = make_channel(4).expect("couldn't construct channel");
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
    fn send_without_receiver_fails() {
        let (sender, mut receiver) = make_channel(4).expect("couldn't construct channel");
        sender.send(1).expect("couldn't send");
        sender.send(2).expect("couldn't send");
        assert_eq!(receiver.recv(), 1);
        drop(receiver);
        assert_eq!(sender.send(1), Err(SendError::Disconnected(1)));
    }

    #[test]
    fn buffer_min_size() {
        assert_eq!(
            make_channel::<()>(0).unwrap_err(),
            BufferError::BufferTooSmall
        );
        assert_eq!(
            make_channel::<()>(1).unwrap_err(),
            BufferError::BufferTooSmall
        );
        assert!(make_channel::<()>(2).is_ok());
    }

    #[test]
    fn buffer_too_big() {
        assert_eq!(
            make_channel::<()>(isize::MAX as usize + 1).unwrap_err(),
            BufferError::BufferTooLarge
        );
        //Don't actually test a valid but extremely large buffer as that would kill ram for tests...
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
        let (sender, mut receiver) = make_channel(16).expect("couldn't construct channel");
        for _ in 0..15 {
            sender
                .send(CustomDropper::new(&counter))
                .expect("couldn't send");
        }
        receiver.recv();
        sender
            .send(CustomDropper::new(&counter))
            .expect("couldn't send");
        drop(receiver);
        drop(sender);
        assert_eq!(counter.load(Ordering::Relaxed), 17);
    }

    #[test]
    fn valid_drop_partial_buffer() {
        let counter = Arc::default();
        let (sender, receiver) = make_channel(16).expect("couldn't construct channel");
        for _ in 0..3 {
            sender
                .send(CustomDropper::new(&counter))
                .expect("couldn't send");
        }
        drop(sender);
        drop(receiver);
        assert_eq!(counter.load(Ordering::Acquire), 3);
    }

    #[test]
    fn valid_drop_empty_buffer() {
        let (sender, _) = make_channel::<CustomDropper>(10).expect("couldn't construct channel");
        drop(sender);
    }

    #[test]
    fn valid_drop_overwrite() {
        let counter = Arc::default();
        let (sender, mut receiver) =
            make_channel::<CustomDropper>(4).expect("couldn't construct channel");
        sender
            .send(CustomDropper::new(&counter))
            .expect("couldn't send");
        sender
            .send(CustomDropper::new(&counter))
            .expect("couldn't send");
        sender
            .send(CustomDropper::new(&counter))
            .expect("couldn't send");
        receiver.recv();
        receiver.recv();
        receiver.recv();
        assert_eq!(counter.load(Ordering::Acquire), 3);
        sender
            .send(CustomDropper::new(&counter))
            .expect("couldn't send");
        assert_eq!(counter.load(Ordering::Acquire), 3);
        sender
            .send(CustomDropper::new(&counter))
            .expect("couldn't send");
        assert_eq!(counter.load(Ordering::Acquire), 4);
        sender
            .send(CustomDropper::new(&counter))
            .expect("couldn't send");
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
