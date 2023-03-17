mod test_shared;
use test_shared::{setup_tests, test};

#[test]
fn one_sender_one_receiver() {
    setup_tests();
    test(1, 1, 100, 5);
}

#[test]
#[cfg_attr(miri, ignore)]
fn one_sender_one_receiver_long() {
    setup_tests();
    test(1, 1, 500_000, 5);
}

#[test]
fn one_sender_two_receiver() {
    setup_tests();
    test(1, 2, 100, 5);
}

#[test]
#[cfg_attr(miri, ignore)]
fn one_sender_two_receiver_long() {
    setup_tests();
    test(1, 2, 500_000, 5);
}

#[test]
fn two_sender_one_receiver() {
    setup_tests();
    test(2, 1, 100, 5);
}

#[test]
fn two_sender_two_receiver() {
    setup_tests();
    test(2, 2, 100, 5);
}

#[test]
#[cfg_attr(miri, ignore)]
fn two_sender_two_receiver_long() {
    setup_tests();
    test(2, 2, 10_000, 5);
}

#[test]
#[cfg_attr(miri, ignore)]
fn two_sender_two_receiver_stress() {
    setup_tests();
    for _ in 0..1000 {
        test(2, 2, 1000, 5);
    }
}
