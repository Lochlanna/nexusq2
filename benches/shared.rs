pub trait TestReceiver<T>: Send {
    fn test_recv(&mut self) -> T;
    fn another(&self) -> Self;
}

pub trait TestSender<T>: Send {
    fn test_send(&mut self, value: T);
    fn another(&self) -> Self;
}

impl<T> TestReceiver<T> for nexusq2::Receiver<T>
where
    T: Clone,
{
    #[inline(always)]
    fn test_recv(&mut self) -> T {
        self.recv()
    }

    fn another(&self) -> Self {
        self.clone()
    }
}

impl<T> TestSender<T> for nexusq2::Sender<T>
where
    T: Send,
{
    fn test_send(&mut self, value: T) {
        if self.send(value).is_err() {
            panic!("couldn't send");
        }
    }

    fn another(&self) -> Self {
        self.clone()
    }
}
