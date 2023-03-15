use crate::test_shared::test;

#[test]
fn loom_1w_by_1r() {
    loom::model(|| {
        test(1, 1, 3, 5);
    });
}

#[test]
fn loom_1w_by_2r() {
    loom::model(|| {
        test(1, 2, 1, 5);
    });
}
