use std::{
    fmt::{Display, Formatter},
    num::ParseIntError,
    str::FromStr,
};

use color_eyre::Result;
use mongodb::{
    bson,
    bson::{doc, Bson, Uuid},
    options::{ClientOptions, Credential},
    Client, Collection, Database,
};
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId, RoleId, UserId};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourtRoom {
    pub channel_id: SnowflakeId,
    pub ongoing_lawsuit: bool,
    pub role_id: SnowflakeId,
}

#[derive(Clone)]
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

    pub async fn new_state(&self, guild_id: SnowflakeId) -> Result<State> {
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

    pub async fn set_court_category(
        &self,
        guild_id: SnowflakeId,
        category: SnowflakeId,
    ) -> Result<()> {
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

    pub async fn delete_guild(
        &self,
        guild_id: SnowflakeId,
    ) -> Result<()> {
        let coll = self.state_coll();

        coll.delete_one(
            doc! { "guild_id": &guild_id },
            None,
        )
        .await
        .wrap_err("delete guild")?;
        Ok(())
    }

    fn state_coll(&self) -> Collection<State> {
        self.db.collection("state")
    }
}
