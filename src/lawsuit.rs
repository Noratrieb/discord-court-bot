use color_eyre::Result;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, UserId};
use tracing::info;

use crate::Mongo;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LawsuitState {
    Initial,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lawsuit {
    pub plaintiff: UserId,
    pub accused: UserId,
    pub plaintiff_layer: Option<UserId>,
    pub accused_layer: Option<UserId>,
    pub reason: String,
    pub state: LawsuitState,
    pub court_room: Option<ChannelId>,
}

impl Lawsuit {
    pub async fn initialize(&mut self, guild_id: String, mongo_client: &Mongo) -> Result<()> {
        let state = mongo_client.find_state(&guild_id).await?;

        let state = match state {
            Some(state) => state,
            None => {
                info!(%guild_id, "No state found for guild, creating new state");
                mongo_client.new_state(guild_id).await?
            }
        };

        Ok(())
    }
}
