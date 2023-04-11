use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use futures::{SinkExt, StreamExt};
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nexusq2::make_channel;

pub trait Another {
    fn another(&self) -> Self;
}

impl<T> Another for nexusq2::Sender<T> {
    fn another(&self) -> Self {
        self.clone()
    }
}

impl<T> Another for nexusq2::Receiver<T> {
    fn another(&self) -> Self {
        self.clone()
    }
}

async fn read_n(receiver: impl StreamExt + 'static, num_to_read: usize) {
    receiver.take(num_to_read).count().await;
}

async fn write_n<S: SinkExt<usize> + 'static + Unpin>(mut sender: S, num_to_write: usize) -> S {
    for i in 0..num_to_write {
        let _ = sender.send(i).await;
    }
    sender
}

async fn nexus(num: usize, writers: usize, readers: usize, iters: u64) -> Duration {
    let size = 100_usize.next_power_of_two();
    let mut total_duration = Duration::new(0, 0);
    for _ in 0..iters {
        let (sender, receiver) = make_channel(size);

        total_duration += run_test(num, writers, readers, sender, receiver).await;
    }

    total_duration
}

#[allow(clippy::too_many_arguments)]
async fn run_test(
    num: usize,
    writers: usize,
    readers: usize,
    sender: impl SinkExt<usize> + 'static + Another + Send + Unpin,
    receiver: impl StreamExt + 'static + Another + Send,
) -> Duration {
    let mut receivers: Vec<_> = (0..readers - 1).map(|_| receiver.another()).collect();
    let mut senders: Vec<_> = (0..writers - 1).map(|_| sender.another()).collect();
    receivers.push(receiver);
    senders.push(sender);

    let receiver_handles: Vec<_> = receivers
        .into_iter()
        .map(|r| {
            tokio::spawn(async move {
                read_n(r, num * writers).await;
            })
        })
        .collect();

    let start_barrier = Arc::new(tokio::sync::Barrier::new(writers + 1));
    let stop_barrier = Arc::new(tokio::sync::Barrier::new(writers + 1));

    senders.into_iter().for_each(|s| {
        let start = Arc::clone(&start_barrier);
        let stop = Arc::clone(&stop_barrier);
        tokio::spawn(async move {
            start.wait().await;
            let s = write_n(s, num).await;
            stop.wait().await;
            drop(s);
        });
    });
    let start = Instant::now();
    start_barrier.wait().await;

    futures::future::join_all(receiver_handles).await;
    let duration = start.elapsed();
    stop_barrier.wait().await;
    duration
}

struct RunParam((usize, usize));
impl Display for RunParam {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{},{}", self.0 .0, self.0 .1).as_str())
    }
}

fn throughput(c: &mut Criterion) {
    let num_elements = 5000;
    let max_writers = 2;
    let max_readers = 2;

    let tokio_runtime = tokio::runtime::Runtime::new().expect("couldn't spawn tokio runtime");

    for num_writers in 1..=max_writers {
        for num_readers in 1..=max_readers {
            let mut group = c.benchmark_group("throughput");
            let input = (num_writers, num_readers);
            group.throughput(Throughput::Elements(
                num_elements as u64 * num_writers as u64,
            ));
            group.bench_with_input(
                BenchmarkId::new("nexus", RunParam(input)),
                &input,
                |b, &input| {
                    b.to_async(&tokio_runtime).iter_custom(|iters| async move {
                        nexus(num_elements, input.0, input.1, iters).await
                    });
                },
            );
            group.finish();
        }
    }
}
criterion_group!(benches, throughput);
criterion_main!(benches);
