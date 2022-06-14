use color_eyre::Result;
use mongodb::{
    bson::doc,
    options::{ClientOptions, Credential},
    Client, Database,
};
use serde::{Deserialize, Serialize};

use crate::{lawsuit::Lawsuit, WrapErr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub guild_id: String,
    pub lawsuits: Vec<Lawsuit>,
    pub justice_category: Option<String>,
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

    pub async fn find_state(&self, guild_id: &str) -> Result<Option<State>> {
        let collection = self.db.collection("state");
        let state = collection
            .find_one(doc! {"guild_id": guild_id  }, None)
            .await
            .wrap_err("find state")?;

        Ok(state)
    }

    pub async fn new_state(&self, guild_id: String) -> Result<State> {
        let state = State {
            guild_id,
            lawsuits: vec![],
            justice_category: None,
            court_rooms: vec![],
        };

        let collection = self.db.collection::<State>("state");
        collection
            .insert_one(&state, None)
            .await
            .wrap_err("insert state")?;
        Ok(state)
    }
}
