pub mod client;
pub mod common;
pub mod server;
pub mod udp;

use std::sync::Once;

static INIT: Once = Once::new();

pub fn initialize() {
    INIT.call_once(|| {
        env_logger::init();
    });
}
