use once_cell::sync::Lazy;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
mod platform_impl;

const HEALTHY: u8 = 0;
const UNHEALTHY: u8 = 1;

#[cfg(not(test))]
const HEALTHY_UPDATE_TIME: Duration = Duration::from_secs(3 * 60);
#[cfg(test)]
const HEALTHY_UPDATE_TIME: Duration = Duration::from_secs(3);


static LAST_DB_WRITE: Lazy<parking_lot::Mutex<SystemTime>> =
    Lazy::new(|| parking_lot::Mutex::from(UNIX_EPOCH));

#[derive(Debug, Error)]
#[error(transparent)]
pub struct HealthError(#[from] platform_impl::HealthError);

pub async fn listen() -> Result<(), HealthError> {
    platform_impl::listen().await.map_err(HealthError)
}

pub fn update() {
    let mut guard = LAST_DB_WRITE.lock();
    *guard = SystemTime::now();
}

pub async fn check() -> ExitCode {
    platform_impl::check().await
}

#[cfg(test)]
mod tests {
    use crate::health_check;
    use super::*;

    trait TestExitCode {
        // Panics if assertion fails.
        fn assert(&self, code: u8, line: u32);
    }

    impl TestExitCode for ExitCode {
        fn assert(&self, code: u8, line: u32) {
            let self_dbg = format!("{:?}", self);
            let code_dbg = format!("{:?}", ExitCode::from(code));
            if self_dbg != code_dbg {
                panic!("ExitCode was not {code:?} at {}:{line:?}", file!());
            }
        }
    }

    #[tokio::test]
    async fn health_check() {
        // there is no server, so the service is unhealthy
        check().await.assert(UNHEALTHY, line!());
        
        tokio::spawn(async {
            if let Err(e) = health_check::listen().await {
                panic!("{e}");
            }
        });

        // unhealthy by default
        tokio::time::sleep(Duration::from_secs(1)).await;
        check().await.assert(UNHEALTHY, line!());

        // after an update the service is healthy
        update();
        check().await.assert(HEALTHY, line!());

        // not updating for half the update time should be fine
        tokio::time::sleep(HEALTHY_UPDATE_TIME / 2).await;
        check().await.assert(HEALTHY, line!());

        // not updating for the other half is too long and unhealthy
        tokio::time::sleep(HEALTHY_UPDATE_TIME / 2).await;
        check().await.assert(UNHEALTHY, line!());

        // updating again makes it healthy again
        update();
        check().await.assert(HEALTHY, line!());

        // still healthy after some time
        tokio::time::sleep(HEALTHY_UPDATE_TIME / 2).await;
        check().await.assert(HEALTHY, line!());

        // update again, we can wait a bit again next time
        update();
        check().await.assert(HEALTHY, line!());

        // since previously updated, this wait should work
        tokio::time::sleep(HEALTHY_UPDATE_TIME / 2).await;
        check().await.assert(HEALTHY, line!());
    }
}
