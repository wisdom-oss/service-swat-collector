use crate::locations::{Location, RequestLocationError};
use crate::webhook::Webhook;
use chrono::NaiveDateTime;
use futures::stream;
use influxdb2::api::buckets::ListBucketsRequest;
use influxdb2::api::organization::ListOrganizationRequest;
use influxdb2::api::write::TimestampPrecision;
use influxdb2::models::data_point::DataPointError;
use influxdb2::models::{DataPoint, PostBucketRequest};
use std::collections::BTreeMap;
use std::str::FromStr;
use std::{env, iter};
use thiserror::Error;
use twilight_model::id::Id;

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

#[tokio::main]
async fn main() {
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

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(120));
    loop {
        interval.tick().await;

        for location in locations::LOCATIONS.locations.iter() {
            if let Err(err) = handle_location(location, &reqwest_client, &influxdb_client).await {
                let datetime = chrono::Utc::now().format("%Y-%m-%d %H:%M");
                type HLE = HandleLocationError;
                type RLE = RequestLocationError;
                match &err {
                    HLE::RequestForecast(RLE::Parse { error, from }) => {
                        println!("ERROR [{datetime}]: {error}, original text:\n{from}");
                    }
                    _ => eprintln!("ERROR [{datetime}]: {err}"),
                }
                let _ = webhook.execute(location, err).await;
            }
        }
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
    let timestamp = timestamp.timestamp();
    let precision = TimestampPrecision::Seconds;

    let current_json = serde_json::to_string(&BTreeMap::from([forecast.current]))?;
    let forecasts_json = serde_json::to_string(&forecast.forecasts)?;
    let data_point = DataPoint::builder("forecast")
        .timestamp(timestamp)
        .field("current", current_json)
        .field("forecasts", forecasts_json)
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
