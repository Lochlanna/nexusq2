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

impl<T> TestReceiver<T> for multiqueue2::BroadcastReceiver<T>
where
    T: 'static + Clone + Send + Sync,
{
    #[inline(always)]
    fn test_recv(&mut self) -> T {
        loop {
            let res = self.recv();
            match res {
                Ok(v) => return v,
                Err(_) => panic!("receiver panic multiqueue2"),
            }
        }
    }

    fn another(&self) -> Self {
        self.add_stream()
    }
}

impl<T> TestSender<T> for nexusq2::Sender<T>
where
    T: Send,
{
    fn test_send(&mut self, value: T) {
        self.send(value);
    }

    fn another(&self) -> Self {
        self.clone()
    }
}

impl<T> TestSender<T> for multiqueue2::BroadcastSender<T>
where
    T: 'static + Clone + Send + Sync,
{
    #[inline(always)]
    fn test_send(&mut self, mut value: T) {
        while let Err(err) = self.try_send(value) {
            match err {
                std::sync::mpsc::TrySendError::Full(v) => value = v,
                std::sync::mpsc::TrySendError::Disconnected(_) => panic!("multiq disconnected"),
            }
        }
    }

    fn another(&self) -> Self {
        self.clone()
    }
}
