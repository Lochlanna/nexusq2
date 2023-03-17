use simple_logger::SimpleLogger;
use std::sync::Once;

static START: Once = Once::new();

pub fn setup_tests() {
    START.call_once(|| {
        SimpleLogger::new().init().unwrap();
    });
}
