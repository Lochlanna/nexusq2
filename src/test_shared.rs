use super::*;
use std::collections::HashMap;
use std::time::Duration;

pub(crate) fn test(num_senders: usize, num_receivers: usize, num: usize, buffer_size: usize) {
    let (sender, receiver) = make_channel(buffer_size);

    let mut receivers: Vec<_> = (0..(num_receivers - 1)).map(|_| receiver.clone()).collect();
    receivers.push(receiver);

    let mut senders: Vec<_> = (0..(num_senders - 1)).map(|_| sender.clone()).collect();
    senders.push(sender);

    let receivers: Vec<_> = receivers
        .into_iter()
        .map(|mut receiver| {
            thread::spawn(move || {
                let mut values = Vec::with_capacity(num);
                for _ in 0..(num * num_senders) {
                    let v = receiver.recv();
                    values.push(v);
                }
                values
            })
        })
        .collect();
    thread::sleep(Duration::from_secs_f64(0.01));

    senders.into_iter().for_each(|sender| {
        thread::spawn(move || {
            for i in 0..num {
                sender.send(i);
            }
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
