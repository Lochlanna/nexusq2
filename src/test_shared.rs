use super::*;
use simple_logger::SimpleLogger;
use std::collections::HashMap;
use std::sync::Once;
use std::thread;
use std::time::Duration;

static START: Once = Once::new();

pub fn setup_logging() {
    START.call_once(|| {
        SimpleLogger::new().init().unwrap();
    });
}

pub(crate) fn test(num_senders: usize, num_receivers: usize, num: usize, buffer_size: usize) {
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

pub(crate) fn advanced_test(
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

    senders.into_iter().for_each(|sender| {
        thread::spawn(move || {
            sender_thread(num, sender_lag, average_jitter, sender);
        });
    });

    let mut expected_map = HashMap::with_capacity(num);
    for i in 0..num {
        expected_map.insert(i, num_senders);
    }

    let results: Vec<_> = receivers
        .into_iter()
        .map(|jh| jh.join().expect("couldn't join read thread"))
        .map(|result| {
            let mut count_map: HashMap<usize, usize> = HashMap::with_capacity(num);
            for v in result {
                let e = count_map.entry(v).or_default();
                *e += 1;
            }
            let mut diff_map = HashMap::new();
            for (k, v) in count_map {
                let expected_count = expected_map
                    .get(&k)
                    .expect("key didn't exist in expected map");
                if *expected_count != v {
                    diff_map.insert(k, v);
                }
            }
            diff_map
        })
        .collect();

    results.iter().for_each(|result| {
        if !result.is_empty() {
            println!("diff: {:?}", result);
        }
        assert!(result.is_empty())
    });
}

fn sender_thread(num: usize, sender_lag: Duration, average_jitter: f64, mut sender: Sender<usize>) {
    for i in 0..num {
        sender.send(i);
        apply_lag(sender_lag, average_jitter);
    }
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
