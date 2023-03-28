use nexusq2::make_channel;
use nexusq2::Receiver;
use nexusq2::Sender;
use pretty_assertions_sorted::assert_eq_sorted;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn test(num_senders: usize, num_receivers: usize, num: usize, buffer_size: usize) {
    advanced_test(
        num_senders,
        num_receivers,
        num,
        buffer_size,
        Duration::default(),
        Duration::default(),
        0.0,
    );
}

pub fn advanced_test(
    num_senders: usize,
    num_receivers: usize,
    num: usize,
    buffer_size: usize,
    receiver_lag: Duration,
    sender_lag: Duration,
    average_jitter: f64,
) {
    let (sender, receiver) = make_channel(buffer_size);

    let mut receivers: Vec<_> = (0..(num_receivers - 1)).map(|_| receiver.clone()).collect();
    receivers.push(receiver);

    let mut senders: Vec<_> = (0..(num_senders - 1)).map(|_| sender.clone()).collect();
    senders.push(sender);

    let receivers: Vec<_> = receivers
        .into_iter()
        .map(|receiver| {
            thread::spawn(move || {
                receive_thread(num_senders, num, receiver_lag, average_jitter, receiver)
            })
        })
        .collect();
    thread::sleep(Duration::from_secs_f64(0.01));

    let sender_barrier = Arc::new(std::sync::Barrier::new(senders.len() + 1));

    for sender in senders {
        let sb_clone = Arc::clone(&sender_barrier);
        thread::spawn(move || {
            let sender = sender_thread(num, sender_lag, average_jitter, sender);
            sb_clone.wait();
            drop(sender);
        });
    }

    let mut expected_map = HashMap::with_capacity(num);
    expected_map.extend((0..num).map(|i| (i, num_senders)));

    let results: Vec<_> = receivers
        .into_iter()
        .map(|jh| jh.join().expect("couldn't join read thread"))
        .collect();

    sender_barrier.wait();

    for result in results {
        let mut count_map: HashMap<usize, usize> = HashMap::with_capacity(num);
        for v in result {
            let e = count_map.entry(v).or_default();
            *e += 1;
        }
        assert_eq_sorted!(expected_map, count_map);
    }
}

fn sender_thread(
    num: usize,
    sender_lag: Duration,
    average_jitter: f64,
    sender: Sender<usize>,
) -> Sender<usize> {
    for i in 0..num {
        sender.send(i);
        apply_lag(sender_lag, average_jitter);
    }
    sender
}

fn apply_lag(lag_time: Duration, average_jitter: f64) {
    if !lag_time.is_zero() {
        let jitter = (rand::random::<f64>() + 0.5) * (average_jitter + 1.0);
        let sleep_time = lag_time.mul_f64(jitter);
        thread::sleep(sleep_time);
    }
}

fn receive_thread(
    num_senders: usize,
    num: usize,
    receiver_lag: Duration,
    average_jitter: f64,
    mut receiver: Receiver<usize>,
) -> Vec<usize> {
    let mut values = Vec::with_capacity(num);
    for _ in 0..(num * num_senders) {
        let v = receiver.recv();
        apply_lag(receiver_lag, average_jitter);
        values.push(v);
    }
    values
}
