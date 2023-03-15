use crate::test_shared::test;

#[test]
fn loom_1w_by_2r() {
    loom::model(|| {
        test(1, 2, 15, 5);
    });
}
