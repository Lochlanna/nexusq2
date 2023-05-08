use crate::wait_strategy::{hybrid::HybridWait, AsyncEventGuard, Wait, WaitError};
use core::fmt::{Debug, Formatter};
use portable_atomic::{AtomicUsize, Ordering};
use std::cell::UnsafeCell;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

pub struct Cell<T> {
    value: UnsafeCell<Option<T>>,
    read_counter: AtomicUsize,
    current_id: AtomicUsize,
    wait_strategy: Box<dyn Wait<AtomicUsize>>,
}

impl<T> Debug for Cell<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        // write all members of cell excluding wait_strategy
        f.debug_struct("Cell")
            .field("value", &self.value)
            .field("read_counter", &self.read_counter)
            .field("current_id", &self.current_id)
            .finish()
    }
}

impl<T> Default for Cell<T> {
    #[allow(clippy::uninit_assumed_init)]
    fn default() -> Self {
        Self::new(HybridWait::default())
    }
}

//wait functions
impl<T> Cell<T> {
    pub fn new(ws: impl Wait<AtomicUsize> + 'static) -> Self {
        Self {
            value: UnsafeCell::new(None),
            read_counter: AtomicUsize::new(0),
            current_id: AtomicUsize::new(usize::MAX),
            wait_strategy: Box::new(ws),
        }
    }

    pub fn wait_for_write_safe(&self) -> bool {
        if self.read_counter.load(Ordering::Acquire) == 0 {
            return true;
        }
        self.wait_strategy.wait_for(&self.read_counter, &0);
        false
    }

    pub fn wait_for_write_safe_before(&self, deadline: Instant) -> Result<bool, WaitError> {
        if self.read_counter.load(Ordering::Acquire) == 0 {
            return Ok(true);
        }
        self.wait_strategy
            .wait_until(&self.read_counter, &0, deadline)?;
        Ok(false)
    }

    pub fn poll_write_safe(
        &self,
        cx: &mut Context<'_>,
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
    ) -> Poll<()> {
        self.wait_strategy
            .poll(cx, &self.read_counter, &0, event_listener)
    }

    pub fn wait_for_published(&self, expected_published_id: usize) {
        self.wait_strategy
            .wait_for(&self.current_id, &expected_published_id);
    }

    pub fn poll_published(
        &self,
        cx: &mut Context<'_>,
        expected_published_id: usize,
        event_listener: &mut Option<Pin<Box<dyn AsyncEventGuard>>>,
    ) -> Poll<()> {
        self.wait_strategy
            .poll(cx, &self.current_id, &expected_published_id, event_listener)
    }
    pub fn wait_for_published_until(
        &self,
        expected_published_id: usize,
        deadline: Instant,
    ) -> Result<(), WaitError> {
        self.wait_strategy
            .wait_until(&self.current_id, &expected_published_id, deadline)
    }
    pub fn wait_for_published_with_timeout(
        &self,
        expected_published_id: usize,
        timeout: Duration,
    ) -> Result<(), WaitError> {
        self.wait_strategy.wait_until(
            &self.current_id,
            &expected_published_id,
            Instant::now() + timeout,
        )
    }

    pub fn get_published(&self) -> usize {
        self.current_id.load(Ordering::Acquire)
    }
}

//write side functions
impl<T> Cell<T> {
    pub fn safe_to_write(&self) -> bool {
        self.read_counter.load(Ordering::Acquire) == 0
    }

    pub fn write_and_publish(&self, value: T, id: usize) {
        let dst = UnsafeCell::raw_get(&self.value);
        let old_value = unsafe { (*dst).replace(value) };
        self.current_id.store(id, Ordering::Release);
        self.wait_strategy.notify_all();
        drop(old_value);
    }
}

//read side functions
impl<T> Cell<T> {
    pub fn move_from(&self) {
        let old = self.read_counter.fetch_sub(1, Ordering::Release);
        debug_assert!(old >= 1);
        if old == 1 {
            // only wake if there are no more readers on the cell
            self.wait_strategy.notify_all();
        }
    }

    pub fn move_to(&self) {
        self.read_counter.fetch_add(1, Ordering::Relaxed);
    }
}
impl<T> Cell<T>
where
    T: Clone,
{
    pub unsafe fn read(&self) -> T {
        (*UnsafeCell::raw_get(&self.value))
            .as_ref()
            .unwrap_unchecked()
            .clone()
    }

    pub fn read_opt(&self) -> Option<T> {
        unsafe { (*UnsafeCell::raw_get(&self.value)).clone() }
    }
}
