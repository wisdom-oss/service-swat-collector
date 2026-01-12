use crate::locations::{Location, RequestLocationError};
use crate::webhook::Webhook;
use chrono::NaiveDateTime;
use clap::Parser;
use futures::stream;
use influxdb2::api::buckets::ListBucketsRequest;
use influxdb2::api::organization::ListOrganizationRequest;
use influxdb2::api::write::TimestampPrecision;
use influxdb2::models::data_point::DataPointError;
use influxdb2::models::{DataPoint, PostBucketRequest};
use std::collections::BTreeMap;
use std::process::ExitCode;
use std::str::FromStr;
use std::{env, iter};
use thiserror::Error;
use twilight_model::id::Id;

#[cfg(feature = "health-check")]
mod health_check;
mod locations;
mod webhook;

const BUCKET_NAME: &str = "swat";

macro_rules! env {
    ($env:literal) => {
        match env::var($env) {
            Ok(var) => var,
            Err(err) => panic!("expected {:?} to be available, {err}", $env),
        }
    };
}

#[derive(Debug, Parser)]
#[command(version)]
pub struct Args {
    /// Runs a health check when used, primarily for Docker to verify the application's status.
    #[cfg(feature = "health-check")]
    #[arg(long = "health-check")]
    pub health_check: bool,

    #[arg(long = "unchecked-tls")]
    pub unchecked_tls: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    #[cfg_attr(not(feature = "health-check"), allow(unused_variables))]
    let args = Args::parse();

    #[cfg(feature = "health-check")]
    {
        if args.health_check {
            return health_check::check().await;
        }

        tokio::spawn(async {
            if let Err(e) = health_check::listen().await {
                eprintln!("{e}");
            }
        });
    }

    let influxdb_url = env!("INFLUXDB_URL");
    let influxdb_org = env!("INFLUXDB_ORG");
    let influxdb_token = env!("INFLUXDB_TOKEN");
    let webhook_token = env!("DISCORD_WEBHOOK_TOKEN");
    let webhook_id = env!("DISCORD_WEBHOOK_ID");
    let webhook_id = Id::from_str(&webhook_id).unwrap();

    let webhook = Webhook::new(webhook_id, webhook_token);
    let reqwest_client = reqwest::Client::new();
    let influxdb_client =
        influxdb2::Client::new(influxdb_url, influxdb_org.clone(), influxdb_token);

    init_bucket(&influxdb_client, influxdb_org).await;

    let mut errors_reported = false;
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(120));
    loop {
        interval.tick().await;
        let locations = &locations::LOCATIONS.locations;

        let mut errors = Vec::with_capacity(locations.len());
        for location in locations.iter() {
            if let Err(err) = handle_location(location, &reqwest_client, &influxdb_client).await {
                handle_location_error(location, err, &mut errors);
            }
        }

        #[cfg(feature = "health-check")]
        health_check::update();

        handle_location_errors(errors.as_slice(), &mut errors_reported, &webhook).await;
    }
}

async fn init_bucket(client: &influxdb2::Client, org: String) {
    let swat_buckets = client
        .list_buckets(Some(ListBucketsRequest {
            name: BUCKET_NAME.to_string().into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    if swat_buckets.buckets.is_empty() {
        let org_id = client
            .list_organizations(ListOrganizationRequest {
                org: org.into(),
                ..Default::default()
            })
            .await
            .unwrap()
            .orgs
            .first()
            .unwrap()
            .id
            .clone()
            .unwrap();

        client
            .create_bucket(Some(PostBucketRequest::new(org_id, BUCKET_NAME.to_owned())))
            .await
            .unwrap();
    }

    let datetime = chrono::Utc::now().format("%Y-%m-%d %H:%M");
    eprintln!("INFO  [{datetime}]: initialized bucket {BUCKET_NAME:?}, swat-collector running");
}

#[derive(Debug, Error)]
enum HandleLocationError {
    #[error("forecast request failed, {0}")]
    RequestForecast(#[from] RequestLocationError),

    #[error("parsing `from` timestamp failed, {0}")]
    ParseFromTimestamp(#[from] chrono::format::ParseError),

    #[error("could not serialize data for query, {0}")]
    SerializeData(#[from] serde_json::Error),

    #[error("error while building data point, {0}")]
    DataPoint(#[from] DataPointError),

    #[error("writing influxdb query failed, {0}")]
    WritePoints(#[from] influxdb2::RequestError),
}

async fn handle_location(
    location: &Location,
    reqwest_client: &reqwest::Client,
    influxdb_client: &influxdb2::Client,
) -> Result<(), HandleLocationError> {
    let forecast = location.request_forecast(reqwest_client).await?;

    let timestamp = NaiveDateTime::parse_from_str(&forecast.from, "%Y-%m-%d %H:%M")?;
    let timestamp = timestamp.and_utc().timestamp();
    let precision = TimestampPrecision::Seconds;

    let current_json = serde_json::to_string(&BTreeMap::from([forecast.current]))?;
    let forecasts_json = serde_json::to_string(&forecast.forecasts)?;
    let data_point = DataPoint::builder("forecast")
        .timestamp(timestamp)
        .field("current", current_json)
        .field("forecasts", forecasts_json)
        .tag("id", location.id.to_string())
        .tag("name", location.name)
        .tag("lat", location.lat.to_string())
        .tag("lon", location.lon.to_string())
        .build()?;

    influxdb_client
        .write_with_precision(BUCKET_NAME, stream::iter(iter::once(data_point)), precision)
        .await?;

    let datetime = chrono::Utc::now().format("%Y-%m-%d %H:%M");
    eprintln!(
        "INFO  [{datetime}]: inserted location {:?} into db for {}",
        location.name, forecast.from
    );

    Ok(())
}

fn handle_location_error<'l>(
    location: &'l Location,
    error: HandleLocationError,
    errors: &mut Vec<(&'l Location, HandleLocationError)>,
) {
    let datetime = chrono::Utc::now().format("%Y-%m-%d %H:%M");
    type HLE = HandleLocationError;
    type RLE = RequestLocationError;
    match &error {
        HLE::RequestForecast(RLE::Parse { error, from }) => {
            println!("ERROR [{datetime}]: {error}, original text:\n{from}");
        }
        error => eprintln!("ERROR [{datetime}]: {error}"),
    }

    errors.push((location, error));
}

async fn handle_location_errors(
    errors: &[(&Location, HandleLocationError)],
    errors_reported: &mut bool,
    webhook: &Webhook,
) {
    match (errors.is_empty(), *errors_reported) {
        (false, false) => {
            if webhook.alert(errors).await.is_ok() {
                *errors_reported = true;
            }
        }
        (true, true) => {
            if webhook.resolved().await.is_ok() {
                *errors_reported = false;
            }
        }
        _ => (),
    }
}
