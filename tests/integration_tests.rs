mod test_shared;

use nexusq2::make_channel;
use std::time::{Duration, Instant};
use test_shared::test;

mod standard_stress_tests {
    use super::*;
    #[cfg(feature = "backoff")]
    use crate::test_shared::advanced_test;
    #[test]
    fn one_sender_one_receiver() {
        test(1, 1, 100, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn one_sender_one_receiver_long() {
        test(1, 1, 500_000, 5);
    }
    #[test]
    fn one_sender_two_receiver() {
        test(1, 2, 100, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn one_sender_two_receiver_long() {
        test(1, 2, 500_000, 5);
    }

    #[test]
    fn two_sender_one_receiver() {
        test(2, 1, 100, 5);
    }

    #[test]
    fn two_sender_two_receiver() {
        test(2, 2, 100, 5);
    }

    #[test]
    #[cfg(feature = "backoff")]
    fn two_sender_two_receiver_backoff() {
        advanced_test(
            2,
            2,
            100,
            5,
            Duration::default(),
            Duration::default(),
            0.0,
            nexusq2::wait_strategy::backoff::BackoffWait::default(),
            nexusq2::wait_strategy::backoff::BackoffWait::default,
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    #[cfg(feature = "backoff")]
    fn two_sender_two_receiver_backoff_long() {
        advanced_test(
            2,
            2,
            1000,
            5,
            Duration::default(),
            Duration::default(),
            0.0,
            nexusq2::wait_strategy::backoff::BackoffWait::default(),
            nexusq2::wait_strategy::backoff::BackoffWait::default,
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn two_sender_two_receiver_long() {
        test(2, 2, 1000, 5);
    }

    #[test]
    #[ignore]
    fn two_sender_two_receiver_stress() {
        for _ in 0..1000 {
            test(2, 2, 1000, 5);
        }
    }
}

mod async_stress_tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    #[cfg_attr(miri, ignore)]
    async fn one_sender_one_receiver_async() {
        test_shared::shared_async::test(1, 1, 100, 5).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    #[cfg_attr(miri, ignore)]
    async fn one_sender_one_receiver_long_async() {
        test_shared::shared_async::test(1, 1, 500_000, 5).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    #[cfg_attr(miri, ignore)]
    async fn one_sender_two_receiver_async() {
        test_shared::shared_async::test(1, 2, 100, 5).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    #[cfg_attr(miri, ignore)]
    async fn one_sender_two_receiver_long_async() {
        test_shared::shared_async::test(1, 2, 500_000, 5).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    #[cfg_attr(miri, ignore)]
    async fn two_sender_one_receiver_async() {
        test_shared::shared_async::test(2, 1, 100, 5).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    #[cfg_attr(miri, ignore)]
    async fn two_sender_two_receiver_async() {
        test_shared::shared_async::test(2, 2, 100, 5).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    #[cfg_attr(miri, ignore)]
    async fn two_sender_two_receiver_long_async() {
        test_shared::shared_async::test(2, 2, 1000, 5).await;
    }
}

mod latency_tests {
    use super::*;

    #[test]
    fn latency() {
        let mut total_duration = Duration::from_nanos(0);
        let (sender, mut receiver) = make_channel(128).expect("couldn't construct channel");
        let num_sent = 500;
        for _ in 0..num_sent {
            sender.send(Instant::now()).expect("couldn't send");
            total_duration += receiver.recv().elapsed();
        }
        println!("that took, {}", total_duration.as_nanos() / num_sent);
    }
}
