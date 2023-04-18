mod shared;
use shared::*;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use nexusq2::make_channel_with;
use nexusq2::wait_strategy::hybrid::HybridWait;
use workerpool::thunk::{Thunk, ThunkWorker};
use workerpool::Pool;

#[inline(always)]
fn read_n(mut receiver: impl TestReceiver<Instant> + 'static, num_to_read: usize) -> Duration {
    let mut total_latency = Duration::default();
    for _ in 0..num_to_read {
        let latency = receiver.test_recv().elapsed();
        total_latency += latency;
    }
    total_latency
}

#[inline(always)]
fn write_n<S: TestSender<Instant> + 'static>(mut sender: S, num_to_write: usize) -> (Duration, S) {
    for _ in 0..num_to_write {
        sender.test_send(Instant::now());
    }
    (Default::default(), sender)
}

fn nexus(
    iterations: u64,
    writers: usize,
    readers: usize,
    pool: &Pool<ThunkWorker<Duration>>,
    tx: &std::sync::mpsc::Sender<Duration>,
    rx: &mut std::sync::mpsc::Receiver<Duration>,
) -> Duration {
    let size = 100_u64.next_power_of_two();
    let (sender, receiver) =
        make_channel_with(size.try_into().unwrap(), HybridWait::new(1000, 0), || {
            HybridWait::new(1000, 0)
        })
        .expect("couldn't construct channel");

    run_test(iterations, writers, readers, pool, tx, rx, sender, receiver)
}

#[allow(clippy::too_many_arguments)]
fn run_test(
    iterations: u64,
    writers: usize,
    readers: usize,
    pool: &Pool<ThunkWorker<Duration>>,
    tx: &std::sync::mpsc::Sender<Duration>,
    rx: &mut std::sync::mpsc::Receiver<Duration>,
    sender: impl TestSender<Instant> + 'static,
    receiver: impl TestReceiver<Instant> + 'static,
) -> Duration {
    let mut receivers: Vec<_> = (0..readers - 1).map(|_| receiver.another()).collect();
    let mut senders: Vec<_> = (0..writers - 1).map(|_| sender.another()).collect();
    receivers.push(receiver);
    senders.push(sender);

    let total_num_messages = iterations * (writers as u64);

    for r in receivers {
        pool.execute_to(
            tx.clone(),
            Thunk::of(move || read_n(r, total_num_messages as usize)),
        )
    }

    let sender_barrier = Arc::new(Barrier::new(writers + 1));
    let sender_thunks = senders.into_iter().map(|s| {
        let barrier = sender_barrier.clone();
        Thunk::of(move || {
            let (res, s) = write_n(s, iterations as usize);
            barrier.wait();
            drop(s);
            res
        })
    });
    sender_thunks.for_each(|thunk| pool.execute(thunk));

    let total_latency: Duration = rx.iter().take(readers).sum();
    sender_barrier.wait();

    total_latency.div_f64((total_num_messages * (readers as u64)) as f64)
}

struct RunParam((usize, usize));
impl Display for RunParam {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{},{}", self.0 .0, self.0 .1).as_str())
    }
}

fn throughput(c: &mut Criterion) {
    let max_writers = 3;
    let max_readers = 3;

    let pool = Pool::<ThunkWorker<Duration>>::new(max_writers + max_readers);
    let (tx, mut rx) = std::sync::mpsc::channel();

    for num_writers in 1..=max_writers {
        for num_readers in 1..=max_readers {
            let mut group = c.benchmark_group("latency");
            let input = (num_writers, num_readers);
            group.bench_with_input(
                BenchmarkId::new("nexus", RunParam(input)),
                &input,
                |b, &input| {
                    b.iter_custom(|iters| {
                        black_box(nexus(iters, input.0, input.1, &pool, &tx, &mut rx))
                    });
                },
            );
            group.finish();
        }
    }
}
criterion_group!(benches, throughput);
criterion_main!(benches);
