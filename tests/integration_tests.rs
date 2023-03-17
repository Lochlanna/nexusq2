mod test_shared;
use test_shared::test;

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
// #[cfg_attr(miri, ignore)]
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
