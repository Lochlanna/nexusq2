mod test_shared;

use nexusq2::make_channel;
use std::time::{Duration, Instant};
use test_shared::test;

#[test]
fn one_sender_one_receiver() {
    test(1, 1, 100, 5);
}

#[tokio::test]
async fn one_sender_one_receiver_async() {
    test_shared::shared_async::test(1, 1, 100, 5).await;
}

#[test]
#[cfg_attr(miri, ignore)]
fn one_sender_one_receiver_long() {
    test(1, 1, 500_000, 5);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn one_sender_one_receiver_long_async() {
    test_shared::shared_async::test(1, 1, 500_000, 5).await;
}

#[test]
fn one_sender_two_receiver() {
    test(1, 2, 100, 5);
}

#[tokio::test]
async fn one_sender_two_receiver_async() {
    test_shared::shared_async::test(1, 2, 100, 5).await;
}

#[test]
#[cfg_attr(miri, ignore)]
fn one_sender_two_receiver_long() {
    test(1, 2, 500_000, 5);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn one_sender_two_receiver_long_async() {
    test_shared::shared_async::test(1, 2, 500_000, 5).await;
}

#[test]
fn two_sender_one_receiver() {
    test(2, 1, 100, 5);
}

#[tokio::test]
async fn two_sender_one_receiver_async() {
    test_shared::shared_async::test(2, 1, 100, 5).await;
}

#[test]
fn two_sender_two_receiver() {
    test(2, 2, 100, 5);
}

#[tokio::test]
async fn two_sender_two_receiver_async() {
    test_shared::shared_async::test(2, 2, 100, 5).await;
}

#[test]
// #[cfg_attr(miri, ignore)]
fn two_sender_two_receiver_long() {
    test(2, 2, 1000, 5);
}

#[tokio::test]
// #[cfg_attr(miri, ignore)]
async fn two_sender_two_receiver_long_async() {
    test_shared::shared_async::test(2, 2, 1000, 5).await;
}

#[test]
#[ignore]
fn latency() {
    let mut total_duration = Duration::from_nanos(0);
    let (sender, mut receiver) = make_channel(128);
    let num_sent = 500;
    for _ in 0..num_sent {
        sender.send(Instant::now());
        total_duration += receiver.recv().elapsed();
    }
    println!("that took, {}", total_duration.as_nanos() / num_sent);
}

#[test]
#[ignore]
#[cfg(not(miri))]
fn mq2_latency() {
    let mut total_duration = Duration::from_nanos(0);
    let (sender, receiver) = multiqueue2::broadcast_queue(128);
    let num_sent = 500;
    for _ in 0..num_sent {
        sender.try_send(Instant::now()).unwrap();
        total_duration += receiver.recv().unwrap().elapsed();
    }
    println!("that took, {}", total_duration.as_nanos() / num_sent);
}

#[test]
#[ignore]
fn two_sender_two_receiver_stress() {
    for _ in 0..1000 {
        test(2, 2, 1000, 5);
    }
}
