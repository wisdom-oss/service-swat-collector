use crate::locations::Location;
use std::fmt::Display;
use thiserror::Error;
use twilight_http::client::Client as DiscordClient;
use twilight_http::error::Error as HttpError;
use twilight_model::id::marker::WebhookMarker;
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedAuthorBuilder, EmbedBuilder};
use twilight_validate::message::MessageValidationError;

pub struct Webhook {
    discord_client: DiscordClient,
    id: Id<WebhookMarker>,
    token: String,
}

#[derive(Debug, Error)]
pub enum WebhookExecuteError {
    #[error("{0}")]
    MessageValidation(#[from] MessageValidationError),

    #[error("{0}")]
    Http(#[from] HttpError),
}

impl Webhook {
    pub fn new(id: Id<WebhookMarker>, token: String) -> Webhook {
        Self {
            discord_client: DiscordClient::new(String::new()),
            id,
            token,
        }
    }

    pub async fn execute(
        &self,
        location: &Location,
        content: impl Display,
    ) -> Result<(), WebhookExecuteError> {
        let embed = EmbedBuilder::new()
            .color(0x9e2c2c)
            .author(EmbedAuthorBuilder::new(location.name).build())
            .description(content.to_string())
            .build();

        self.discord_client
            .execute_webhook(self.id, &self.token)
            .embeds(&[embed])?
            .await
            .map(|_| ())
            .map_err(|err| err.into())
    }
}