use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::{env, fs, io};
use std::fs::File;
use std::time::{Duration, SystemTime};
use thiserror::Error;

const HEALTHY: u8 = 0;
const UNHEALTHY: u8 = 1;

static HEALTH_FILE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Error)]
pub enum HealthError {
    #[error("could not find a location for the health directory")]
    NoLocation,

    #[error("could not create directory for health file, {0}")]
    CreateDir(#[source] io::Error),

    #[error("could not create health file, {0}")]
    CreateFile(#[source] io::Error),

    #[error("health file was not initialized")]
    Unset,

    #[error("could not open health file, {0}")]
    Open(#[source] io::Error),

    #[error("could not set modified time for health file, {0}")]
    Touch(#[source] io::Error),

    #[error("could not query metadata for health file, {0}")]
    QueryMetadata(#[source] io::Error),

    #[error("could not read modified timestamp for health_file, {0}")]
    ModifiedNotAvailable(#[source] io::Error)
}

fn health_file_path() -> Result<PathBuf, HealthError> {
    let mut path = match (dirs::data_local_dir(), env::current_dir()) {
        (Some(dir), _) | (None, Ok(dir)) => dir,
        _ => return Err(HealthError::NoLocation),
    };
    path.push(env!("CARGO_PKG_NAME"));
    path.push(".health");
    Ok(path)
}

pub fn create() -> Result<(), HealthError> {
    let health_file_path = match HEALTH_FILE.get() {
        Some(path) => path,
        None => {
            let path = health_file_path()?;
            HEALTH_FILE.set(path).expect("`HEALTH_FILE` was `None`");
            HEALTH_FILE.get().expect("just set")
        }
    };
    let health_dir_path = health_file_path.parent().expect("created using parent dir");
    fs::create_dir_all(&health_dir_path).map_err(HealthError::CreateDir)?;
    fs::write(&health_file_path, []).map_err(HealthError::CreateFile)?;
    Ok(())
}

pub fn touch() -> Result<(), HealthError> {
    let health_file_path = HEALTH_FILE.get().ok_or(HealthError::Unset)?;
    let health_file = File::create(health_file_path).map_err(HealthError::Open)?;
    health_file.set_modified(SystemTime::now()).map_err(HealthError::Touch)?;
    Ok(())
}

pub fn check() -> ExitCode {
    let check_file: fn() -> Result<bool, HealthError> = || {
        let health_file_path = health_file_path()?;
        let health_file = File::open(health_file_path).map_err(HealthError::Open)?;
        let metadata = health_file.metadata().map_err(HealthError::QueryMetadata)?;
        let modified = metadata.modified().map_err(HealthError::ModifiedNotAvailable)?;
        let elapsed = match modified.elapsed() {
            // modified time is in the future, that's fine
            Err(_) => return Ok(true),
            Ok(duration) => duration
        };
        Ok(elapsed < Duration::from_secs(300))
    };

    match check_file() {
        Ok(true) => HEALTHY.into(),
        Ok(false) => {
            eprintln!("last health update was longer than 5 minutes ago");
            UNHEALTHY.into()
        }
        Err(e) => {
            eprintln!("{e}");
            UNHEALTHY.into()
        }
    }
}
