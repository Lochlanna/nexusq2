//! A wait strategy that uses an event listener to wait for a condition to be met.

use crate::wait_strategy::{
    AsyncEventGuard, Notifiable, Take, Takeable, Wait, WaitError, Waitable,
};
use std::task::{Context, Poll};
use std::time::Instant;
use wake_me::Event;

/// A wait strategy that uses an event listener to wait for a condition to be met.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Default)]
pub struct BlockStrategy {
    event: Event,
}

impl BlockStrategy {
    /// Creates a new block strategy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            event: Event::default(),
        }
    }
}

impl Clone for BlockStrategy {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Notifiable for BlockStrategy {
    fn notify_all(&self) {
        self.event.notify_all();
    }

    fn notify_one(&self) {
        self.event.notify_one();
    }
}

impl<W> Wait<W> for BlockStrategy
where
    W: Waitable,
{
    fn wait_for(&self, waitable: &W, expected_value: &W::Inner) {
        loop {
            let listen_guard = self.event.listen();
            if waitable.check(expected_value) {
                return;
            }
            listen_guard.wait();
            if waitable.check(expected_value) {
                return;
            }
        }
    }

    fn wait_until(
        &self,
        waitable: &W,
        expected_value: &W::Inner,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        loop {
            let listen_guard = self.event.listen();
            if waitable.check(expected_value) {
                return Ok(());
            }
            if listen_guard.wait_deadline(deadline).is_err() {
                return Err(WaitError::Timeout);
            }
            if waitable.check(expected_value) {
                return Ok(());
            }
        }
    }

    fn poll(
        &self,
        cx: &mut Context<'_>,
        waitable: &W,
        expected_value: &W::Inner,
        event_listener: &mut Option<Box<dyn AsyncEventGuard>>,
    ) -> Poll<()> {
        if waitable.check(expected_value) {
            *event_listener = None;
            return Poll::Ready(());
        }
        #[allow(clippy::option_if_let_else)]
        let mut listen_guard = match event_listener {
            None => event_listener.insert(Box::new(self.event.listen())),
            Some(lg) => lg,
        };
        loop {
            if waitable.check(expected_value) {
                *event_listener = None;
                return Poll::Ready(());
            }
            let poll = listen_guard.poll(cx);
            match poll {
                Poll::Ready(_) => {
                    if waitable.check(expected_value) {
                        *event_listener = None;
                        return Poll::Ready(());
                    }
                    listen_guard = event_listener.insert(Box::new(self.event.listen()));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

impl<T> Take<T> for BlockStrategy
where
    T: Takeable,
{
    fn take(&self, takeable: &T) -> T::Inner {
        loop {
            let listen_guard = self.event.listen();
            if let Some(value) = takeable.try_take() {
                return value;
            }
            listen_guard.wait();
            if let Some(value) = takeable.try_take() {
                return value;
            }
        }
    }

    fn try_take(&self, takeable: &T) -> Option<T::Inner> {
        takeable.try_take()
    }

    fn take_before(&self, takeable: &T, deadline: Instant) -> Result<T::Inner, WaitError> {
        loop {
            let listen_guard = self.event.listen();
            if let Some(v) = takeable.try_take() {
                return Ok(v);
            }
            if listen_guard.wait_deadline(deadline).is_err() {
                return Err(WaitError::Timeout);
            }
            if let Some(v) = takeable.try_take() {
                return Ok(v);
            }
        }
    }

    fn poll(
        &self,
        cx: &mut Context<'_>,
        takeable: &T,
        event_listener: &mut Option<Box<dyn AsyncEventGuard>>,
    ) -> Poll<T::Inner> {
        if let Some(ptr) = takeable.try_take() {
            *event_listener = None;
            return Poll::Ready(ptr);
        }
        #[allow(clippy::option_if_let_else)]
        let mut listen_guard = match event_listener {
            None => event_listener.insert(Box::new(self.event.listen())),
            Some(lg) => lg,
        };

        loop {
            if let Some(ptr) = takeable.try_take() {
                *event_listener = None;
                return Poll::Ready(ptr);
            }
            match listen_guard.poll(cx) {
                Poll::Ready(_) => {
                    if let Some(ptr) = takeable.try_take() {
                        *event_listener = None;
                        return Poll::Ready(ptr);
                    }
                    listen_guard = event_listener.insert(Box::new(self.event.listen()));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}
