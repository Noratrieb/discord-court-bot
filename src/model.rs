use color_eyre::Result;
use mongodb::{options::ClientOptions, Client, Database};
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};

use crate::{lawsuit::Lawsuit, WrapErr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub guild_id: GuildId,
    pub lawsuits: Vec<Lawsuit>,
    pub justice_category: ChannelId,
    pub court_rooms: Vec<CourtRoom>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourtRoom {
    pub channel_id: ChannelId,
    pub ongoing_lawsuit: bool,
}

pub struct Mongo {
    client: Client,
    db: Database,
}

impl Mongo {
    pub async fn connect(uri: &str, db_name: &str) -> Result<Self> {
        let mut client_options = ClientOptions::parse(uri)
            .await
            .wrap_err("failed to create client options")?;

        client_options.app_name = Some("Discord Court Bot".to_owned());

        let client = Client::with_options(client_options).wrap_err("failed to create client")?;

        let db = client.database(db_name);

        Ok(Self { client, db })
    }

    pub fn insert_lawsuit(lawsuit: &Lawsuit) {}
}
