use color_eyre::Result;
use mongodb::{
    bson::doc,
    options::{ClientOptions, Credential},
    Client, Collection, Database,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{lawsuit::Lawsuit, WrapErr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub guild_id: String,
    pub lawsuits: Vec<Lawsuit>,
    pub court_category: Option<String>,
    pub court_rooms: Vec<CourtRoom>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourtRoom {
    pub channel_id: String,
    pub ongoing_lawsuit: bool,
}

pub struct Mongo {
    db: Database,
}

impl Mongo {
    pub async fn connect(
        uri: &str,
        db_name: &str,
        username: String,
        password: String,
    ) -> Result<Self> {
        let mut client_options = ClientOptions::parse(uri)
            .await
            .wrap_err("failed to create client options")?;

        client_options.app_name = Some("Discord Court Bot".to_owned());
        let mut credentials = Credential::default();
        credentials.username = Some(username);
        credentials.password = Some(password);
        client_options.credential = Some(credentials);

        let client = Client::with_options(client_options).wrap_err("failed to create client")?;

        let db = client.database(db_name);

        Ok(Self { db })
    }

    pub async fn find_or_insert_state(&self, guild_id: &str) -> Result<State> {
        let coll = self.state_coll();
        let state = coll
            .find_one(doc! {"guild_id": &guild_id  }, None)
            .await
            .wrap_err("find state")?;

        let state = match state {
            Some(state) => state,
            None => {
                info!(%guild_id, "No state found for guild, creating new state");
                self.new_state(guild_id.to_owned()).await?
            }
        };

        Ok(state)
    }

    pub async fn new_state(&self, guild_id: String) -> Result<State> {
        let state = State {
            guild_id,
            lawsuits: vec![],
            court_category: None,
            court_rooms: vec![],
        };

        let coll = self.db.collection::<State>("state");
        coll.insert_one(&state, None)
            .await
            .wrap_err("insert state")?;
        Ok(state)
    }

    pub async fn set_court_category(&self, guild_id: &str, category: &str) -> Result<()> {
        let _ = self.find_or_insert_state(guild_id).await?;
        let coll = self.state_coll();
        coll.update_one(
            doc! {"guild_id": &guild_id  },
            doc! {"$set": { "court_category": category }},
            None,
        )
        .await
        .wrap_err("update court category")?;
        Ok(())
    }

    fn state_coll(&self) -> Collection<State> {
        self.db.collection("state")
    }
}
