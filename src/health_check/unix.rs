use crate::health_check::{HEALTHY, HEALTHY_UPDATE_TIME, LAST_DB_WRITE, UNHEALTHY};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, io};
use thiserror::Error;
use tokio::net::{UnixListener, UnixStream};

const HEALTH_CHECK_PATH: &str = "/tmp/wisdom/swat-collector.health.sock";

#[derive(Debug, Error)]
pub enum HealthError {
    #[error("could not create health socket, {0}")]
    Create(#[source] io::Error),

    #[error("could not connect to socket, {0}")]
    ConnectSocket(#[source] io::Error),

    #[error("could not check if the socket is ready, {0}")]
    SocketReady(#[source] io::Error),

    #[error("an error occurred while reading from the socket, {0}")]
    ReadSocket(#[source] io::Error),

    #[error("an error occurred while writing to the socket, {0}")]
    WriteSocket(#[source] io::Error),
}

pub async fn listen() -> Result<(), HealthError> {
    let path = Path::new(HEALTH_CHECK_PATH);
    let dir = path.parent().expect("path has parent dir");
    fs::create_dir_all(dir).map_err(HealthError::Create)?;
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path).map_err(HealthError::Create)?;
    listen_loop(&listener).await?;
    unreachable!("listen never returns with Ok")
}

async fn listen_loop(listener: &UnixListener) -> Result<(), HealthError> {
    loop {
        let (stream, _) = listener.accept().await.map_err(HealthError::Create)?;
        stream.readable().await.map_err(HealthError::SocketReady)?;
        let mut buf = [0u8; 1];
        match stream.try_read(&mut buf) {
            // client has closed, wait for a new connection
            Ok(0) => continue,
            Ok(_) => {
                stream.writable().await.map_err(HealthError::SocketReady)?;
                respond(&stream).await?;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(HealthError::ReadSocket(e)),
        }
    }
}

async fn respond(stream: &UnixStream) -> Result<(), HealthError> {
    stream.writable().await.map_err(HealthError::SocketReady)?;
    let last_db_guard = LAST_DB_WRITE.lock();
    stream
        .try_write(
            &last_db_guard
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_ne_bytes(),
        )
        .map_err(HealthError::WriteSocket)?;
    Ok(())
}

pub async fn check() -> ExitCode {
    match check_impl().await {
        Ok(true) => HEALTHY,
        Ok(false) => UNHEALTHY,
        Err(e) => {
            eprintln!("{e}");
            UNHEALTHY
        }
    }
    .into()
}

async fn check_impl() -> Result<bool, HealthError> {
    let stream = UnixStream::connect(HEALTH_CHECK_PATH)
        .await
        .map_err(HealthError::ConnectSocket)?;
    stream.writable().await.map_err(HealthError::SocketReady)?;
    stream.try_write(&[1]).map_err(HealthError::WriteSocket)?;
    stream.readable().await.map_err(HealthError::SocketReady)?;
    let mut buf = [0; 8];
    stream.try_read(&mut buf).map_err(HealthError::ReadSocket)?;
    let secs = u64::from_ne_bytes(buf);
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    let Ok(diff) = time.elapsed() else {
        println!("last update is from the future, this is fine");
        return Ok(true);
    };
    println!("last update was {} seconds ago", diff.as_secs());
    Ok(diff < HEALTHY_UPDATE_TIME)
}
