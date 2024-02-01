use crate::locations::Location;
use crate::HandleLocationError;

use thiserror::Error;
use twilight_http::client::Client as DiscordClient;
use twilight_http::error::Error as HttpError;
use twilight_model::channel::message::Embed;
use twilight_model::id::marker::WebhookMarker;
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};
use twilight_validate::embed::FIELD_COUNT;
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

    pub async fn alert(
        &self,
        errors: &[(&Location, HandleLocationError)],
    ) -> Result<(), WebhookExecuteError> {
        let mut embed = EmbedBuilder::new()
            .color(0x9E2C2C)
            .description("Some errors occurred.\nAs soon as all requests are successful again you will be notified.");

        for field in errors.iter().take(FIELD_COUNT).map(|(location, error)| {
            EmbedFieldBuilder::new(location.name, error.to_string()).build()
        }) {
            embed = embed.field(field);
        }

        self.execute_embed_webhook(embed.build()).await
    }

    pub async fn resolved(&self) -> Result<(), WebhookExecuteError> {
        let embed = EmbedBuilder::new()
            .color(0x57F287)
            .description("All requests have been successful. Collector working as expected again.");
        self.execute_embed_webhook(embed.build()).await
    }

    pub async fn execute_embed_webhook(&self, embed: Embed) -> Result<(), WebhookExecuteError> {
        self.discord_client
            .execute_webhook(self.id, &self.token)
            .embeds(&[embed])?
            .await
            .map(|_| ())
            .map_err(|err| err.into())
    }
}
