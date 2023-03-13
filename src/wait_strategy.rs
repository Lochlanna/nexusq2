pub trait WaitStrategy {
    fn wait_for<V: Waitable>(
        &self,
        variable: &V,
        expected: V::BaseType,
        check: fn(&V, V::BaseType) -> Option<V::BaseType>,
    ) -> V::BaseType;
    fn notify(&self) {}
}

pub trait Waitable {
    type BaseType: Copy;
}

#[derive(Debug, Default)]
struct HybridWaitStrategy {
    num_spin: u64,
    num_yield: u64,
    event: event_listener::Event,
}

impl WaitStrategy for HybridWaitStrategy {
    fn wait_for<V: Waitable>(
        &self,
        variable: &V,
        expected: V::BaseType,
        check: fn(&V, V::BaseType) -> Option<V::BaseType>,
    ) -> V::BaseType {
        for _ in 0..self.num_spin {
            if let Some(v) = check(variable, expected) {
                return v;
            }
            core::hint::spin_loop()
        }
        for _ in 0..self.num_yield {
            if let Some(v) = check(variable, expected) {
                return v;
            }
            std::thread::yield_now();
        }
        loop {
            if let Some(v) = check(variable, expected) {
                return v;
            }
            let listener = self.event.listen();
            if let Some(v) = check(variable, expected) {
                return v;
            }
            listener.wait();
        }
    }
    fn notify(&self) {
        self.event.notify(usize::MAX);
    }
}
