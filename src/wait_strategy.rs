use std::sync::atomic::{AtomicI64, Ordering};

pub trait WaitStrategy {
    fn wait_for_at_least<V: Waitable>(&self, variable: &V, min_value: V::BaseType) -> V::BaseType;
    fn notify(&self) {}
}

pub trait Waitable {
    type BaseType: Copy;
    fn at_least(&self, expected: Self::BaseType) -> Option<Self::BaseType>;
}

impl Waitable for AtomicI64 {
    type BaseType = i64;

    fn at_least(&self, expected: Self::BaseType) -> Option<Self::BaseType> {
        let current_value = self.load(Ordering::SeqCst);
        if current_value >= expected {
            Some(current_value)
        } else {
            None
        }
    }
}

#[derive(Debug, Default)]
pub struct HybridWaitStrategy {
    num_spin: u64,
    num_yield: u64,
    event: event_listener::Event,
}

impl WaitStrategy for HybridWaitStrategy {
    fn wait_for_at_least<V: Waitable>(&self, variable: &V, min_value: V::BaseType) -> V::BaseType {
        loop {
            if let Some(v) = variable.at_least(min_value) {
                return v;
            }
            core::hint::spin_loop()
        }
        // for _ in 0..self.num_spin {
        //
        // }
        // for _ in 0..self.num_yield {
        //     if let Some(v) = check(variable, expected) {
        //         return v;
        //     }
        //     std::thread::yield_now();
        // }
        // loop {
        //     if let Some(v) = check(variable, expected) {
        //         return v;
        //     }
        //     let listener = self.event.listen();
        //     if let Some(v) = check(variable, expected) {
        //         return v;
        //     }
        //     listener.wait();
        // }
    }
    fn notify(&self) {
        self.event.notify(usize::MAX);
    }
}
