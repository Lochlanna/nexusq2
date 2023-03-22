use std::hint::black_box;
use std::time::{Duration, Instant};

use std::io::Write;

use nexusq2::make_channel;
use workerpool::thunk::{Thunk, ThunkWorker};
use workerpool::Pool;

trait TestReceiver<T>: Send {
    fn test_recv(&mut self) -> T;
    fn another(&self) -> Self;
}

impl<T> TestReceiver<T> for nexusq2::Receiver<T>
where
    T: Clone,
{
    fn test_recv(&mut self) -> T {
        self.recv()
    }

    fn another(&self) -> Self {
        self.clone()
    }
}

trait TestSender<T>: Send {
    fn test_send(&mut self, value: T);
    fn another(&self) -> Self;
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

fn read_n(mut receiver: impl TestReceiver<usize> + 'static, num_to_read: usize) {
    for _ in 0..num_to_read {
        black_box(receiver.test_recv());
    }
}

fn write_n(mut sender: impl TestSender<usize> + 'static, num_to_write: usize) {
    for i in 0..num_to_write {
        sender.test_send(i);
    }
}

fn nexus(
    num: usize,
    writers: usize,
    readers: usize,
    pool: &Pool<ThunkWorker<()>>,
    tx: &std::sync::mpsc::Sender<()>,
    rx: &mut std::sync::mpsc::Receiver<()>,
    iters: u64,
) -> Duration {
    let mut total_duration = Duration::new(0, 0);
    for _ in 0..iters {
        let (sender, receiver) = make_channel(100);
        let mut receivers: Vec<_> = (0..readers - 1).map(|_| receiver.another()).collect();
        let mut senders: Vec<_> = (0..writers - 1).map(|_| sender.another()).collect();
        receivers.push(receiver);
        senders.push(sender);

        for r in receivers {
            pool.execute_to(tx.clone(), Thunk::of(move || read_n(r, num * writers)));
        }
        let senders: Vec<_> = senders
            .into_iter()
            .map(|s| Thunk::of(move || write_n(s, num)))
            .collect();
        let start = Instant::now();
        for s in senders {
            pool.execute(s);
        }
        let results = rx.iter().take(readers).count();
        total_duration += start.elapsed();
        assert_eq!(results, readers);
    }

    total_duration
}

/// Used for debugging and profiling. Based on the benchmark code
#[test]
#[ignore]
fn profile() {
    let num = 100000;
    // let num = 1000;
    let writers = 1;
    let readers = 2;
    let iterations = 500;

    let pool = Pool::<ThunkWorker<()>>::new(writers + readers);
    let (tx, mut rx) = std::sync::mpsc::channel();
    for _ in 1..=1 {
        let duration = nexus(num, writers, readers, &pool, &tx, &mut rx, iterations);
        let throughput =
            (num * writers * iterations as usize) as f64 / duration.as_secs_f64() / 1_000_000_f64;
        println!("{readers} throughput is {throughput} million/second");
        let _ = std::io::stdout().flush();
    }
}
