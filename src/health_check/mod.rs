use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use once_cell::sync::Lazy;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

const HEALTHY: u8 = 0;
const UNHEALTHY: u8 = 1;

static LAST_DB_WRITE: Lazy<parking_lot::Mutex<SystemTime>> = Lazy::new(|| parking_lot::Mutex::from(UNIX_EPOCH));

#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
pub use windows::*;

pub fn update() {
    let mut guard = LAST_DB_WRITE.lock();
    *guard = SystemTime::now();
}
