use reqwest::Client as ReqwestClient;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use static_toml::static_toml;
use std::collections::BTreeMap;

static_toml! {
    #[static_toml(values_ident = Location)]
    #[derive(Debug)]
    pub static LOCATIONS = include_toml!("locations.toml");
}

pub use locations::locations::location::Location;

#[derive(Debug, serde::Deserialize)]
pub struct Forecast {
    #[serde(rename(deserialize = "vorhersageZeit"))]
    pub from: String,

    pub lat: f64,
    pub lon: f64,

    #[serde(
        rename(deserialize = "aktuell"),
        deserialize_with = "deserialize_current_forecast"
    )]
    pub current: (String, u32),

    #[serde(rename(deserialize = "vorhersage"))]
    pub forecasts: BTreeMap<String, u32>,
}

fn deserialize_current_forecast<'de, D>(deserializer: D) -> Result<(String, u32), D::Error>
where
    D: Deserializer<'de>,
{
    let map = BTreeMap::deserialize(deserializer)?;
    let entry = map
        .into_iter()
        .next()
        .ok_or(D::Error::custom("expected at least one element"))?;
    Ok(entry)
}

impl Location {
    pub async fn request_forecast(
        &self,
        client: &ReqwestClient,
    ) -> Result<Forecast, reqwest::Error> {
        let Location { lat, lon, .. } = self;

        let response = client
            .get(format!(
                "https://swat.itwh.de/Vorhersage?lat={lat}&lon={lon}"
            ))
            .send()
            .await?;

        let forecast: Forecast = response.json().await?;
        Ok(forecast)
    }
}
