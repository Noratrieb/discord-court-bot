use std::{
    fmt::{Display, Formatter},
    num::ParseIntError,
    str::FromStr,
};

use color_eyre::Result;
use mongodb::{
    bson,
    bson::{doc, Bson, Uuid},
    options::{ClientOptions, Credential, IndexOptions, UpdateOptions},
    Client, Collection, Database, IndexModel,
};
use poise::serenity::model::id::{ChannelId, GuildId, RoleId, UserId};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{lawsuit::Lawsuit, WrapErr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SnowflakeId(#[serde(with = "serde_string")] pub u64);

impl FromStr for SnowflakeId {
    type Err = ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl Display for SnowflakeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

mod serde_string {
    use std::{fmt::Display, str::FromStr};

    use serde::{de, Deserialize, Deserializer, Serializer};

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Display,
        S: Serializer,
    {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: FromStr,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

impl From<SnowflakeId> for Bson {
    fn from(id: SnowflakeId) -> Self {
        Bson::String(id.to_string())
    }
}

macro_rules! from_snowflake {
    ($($ty:ty),*) => {
        $(
            impl From<SnowflakeId> for $ty {
                fn from(id: SnowflakeId) -> Self {
                    Self(id.0)
                }
            }

            impl From<$ty> for SnowflakeId {
                fn from(id: $ty) -> Self {
                    Self(id.0)
                }
            }
        )*
    };
}

from_snowflake!(GuildId, RoleId, ChannelId, UserId);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub guild_id: SnowflakeId,
    pub lawsuits: Vec<Lawsuit>,
    pub court_category: Option<SnowflakeId>,
    pub court_rooms: Vec<CourtRoom>,
    pub prison_role: Option<SnowflakeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourtRoom {
    pub channel_id: SnowflakeId,
    pub ongoing_lawsuit: bool,
    pub role_id: SnowflakeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrisonEntry {
    pub guild_id: SnowflakeId,
    pub user_id: SnowflakeId,
}

#[derive(Clone)]
pub struct Mongo {
    db: Database,
}

impl Mongo {
    #[tracing::instrument(skip(password))]
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
        let mongo = Self { db };

        info!("Creating indexes");

        mongo
            .state_coll()
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "guild_id": 1 })
                    .options(IndexOptions::builder().name("state.guild_id".to_string()).build())
                    .build(),
                None,
            )
            .await
            .wrap_err("create state index")?;

        mongo
            .prison_coll()
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "guild_id": 1, "user_id": 1 })
                    .options(IndexOptions::builder().name("prison.guild_id_user_id".to_string()).build())
                    .build(),
                None,
            )
            .await
            .wrap_err("create state index")?;

        Ok(mongo)
    }

    #[tracing::instrument(skip(self))]
    pub async fn find_or_insert_state(&self, guild_id: SnowflakeId) -> Result<State> {
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

    #[tracing::instrument(skip(self))]
    pub async fn new_state(&self, guild_id: SnowflakeId) -> Result<State> {
        let state = State {
            guild_id,
            lawsuits: vec![],
            court_category: None,
            court_rooms: vec![],
            prison_role: None,
        };

        let coll = self.db.collection::<State>("state");
        coll.insert_one(&state, None)
            .await
            .wrap_err("insert state")?;
        Ok(state)
    }

    #[tracing::instrument(skip(self))]
    pub async fn set_court_category(
        &self,
        guild_id: SnowflakeId,
        category: SnowflakeId,
    ) -> Result<()> {
        let _ = self.find_or_insert_state(guild_id).await?;
        let coll = self.state_coll();
        coll.update_one(
            doc! { "guild_id": &guild_id  },
            doc! { "$set": { "court_category": category } },
            None,
        )
        .await
        .wrap_err("update court category")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn set_prison_role(
        &self,
        guild_id: SnowflakeId,
        prison_role: SnowflakeId,
    ) -> Result<()> {
        let _ = self.find_or_insert_state(guild_id).await?;
        let coll = self.state_coll();
        coll.update_one(
            doc! { "guild_id": &guild_id  },
            doc! { "$set": { "prison_role": prison_role } },
            None,
        )
        .await
        .wrap_err("update prison role")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn add_court_room(&self, guild_id: SnowflakeId, room: &CourtRoom) -> Result<()> {
        let _ = self.find_or_insert_state(guild_id).await?;
        let coll = self.state_coll();
        coll.update_one(
            doc! { "guild_id": &guild_id  },
            doc! { "$push": { "court_rooms": bson::to_bson(room).wrap_err("invalid bson for room")? }},
            None,
        )
        .await
        .wrap_err("push court room")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn add_lawsuit(&self, guild_id: SnowflakeId, lawsuit: &Lawsuit) -> Result<()> {
        let _ = self.find_or_insert_state(guild_id).await?;
        let coll = self.state_coll();

        coll.update_one(
            doc! { "guild_id": &guild_id  },
            doc! { "$push": { "lawsuits": bson::to_bson(lawsuit).wrap_err("invalid bson for lawsuit")? } },
            None,
        )
        .await
        .wrap_err("push lawsuit")?;

        Ok(())
    }

    #[tracing::instrument(skip(self, value))]
    pub async fn set_court_room(
        &self,
        guild_id: SnowflakeId,
        channel_id: SnowflakeId,
        value: impl Into<Bson>,
    ) -> Result<()> {
        let _ = self.find_or_insert_state(guild_id).await?;
        let coll = self.state_coll();

        coll.update_one(
            doc! { "guild_id": &guild_id, "court_rooms.channel_id": channel_id  },
            doc! { "$set": value.into() },
            None,
        )
        .await
        .wrap_err("set courtroom")?;
        Ok(())
    }

    #[tracing::instrument(skip(self, value))]
    pub async fn set_lawsuit(
        &self,
        guild_id: SnowflakeId,
        lawsuit_id: Uuid,
        value: impl Into<Bson>,
    ) -> Result<()> {
        let _ = self.find_or_insert_state(guild_id).await?;
        let coll = self.state_coll();

        coll.update_one(
            doc! { "guild_id": &guild_id, "lawsuit.id": lawsuit_id  },
            doc! { "$set": value.into() },
            None,
        )
        .await
        .wrap_err("set courtroom")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn delete_guild(&self, guild_id: SnowflakeId) -> Result<()> {
        let coll = self.state_coll();

        coll.delete_one(doc! { "guild_id": &guild_id }, None)
            .await
            .wrap_err("delete guild")?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn add_to_prison(&self, guild_id: SnowflakeId, user_id: SnowflakeId) -> Result<()> {
        let coll = self.prison_coll();

        coll.update_one(
            doc! { "guild_id": guild_id, "user_id": user_id },
            doc! {
                "$setOnInsert": {
                    "guild_id": guild_id, "user_id": user_id,
                }
            },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await
        .wrap_err("add to prison collection")?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove_from_prison(
        &self,
        guild_id: SnowflakeId,
        user_id: SnowflakeId,
    ) -> Result<()> {
        let coll = self.prison_coll();

        coll.delete_one(doc! { "guild_id": guild_id, "user_id": user_id }, None)
            .await
            .wrap_err("remove from prison")?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn find_prison_entry(
        &self,
        guild_id: SnowflakeId,
        user_id: SnowflakeId,
    ) -> Result<Option<PrisonEntry>> {
        let coll = self.prison_coll();

        coll.find_one(doc! { "guild_id": guild_id, "user_id": user_id }, None)
            .await
            .wrap_err("remove from prison")
    }

    fn state_coll(&self) -> Collection<State> {
        self.db.collection("state")
    }

    fn prison_coll(&self) -> Collection<PrisonEntry> {
        self.db.collection("prison")
    }
}
