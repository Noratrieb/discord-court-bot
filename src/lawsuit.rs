use std::str::FromStr;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use serenity::{
    http::Http,
    model::{
        channel::PermissionOverwriteType,
        id::{ChannelId, UserId},
        prelude::{GuildId, PermissionOverwrite},
        Permissions,
    },
};
use tracing::info;

use crate::{handler::Response, model::CourtRoom, Mongo, WrapErr};

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
    pub async fn initialize(
        &mut self,
        http: &Http,
        guild_id: GuildId,
        mongo_client: &Mongo,
    ) -> Result<Response> {
        let state = mongo_client
            .find_or_insert_state(&guild_id.to_string())
            .await?;

        let free_room = state.court_rooms.iter().find(|r| !r.ongoing_lawsuit);

        match (free_room, &state.court_category) {
            (Some(_room), _) => Ok(Response::Simple("a free room? rip".to_owned())),
            (None, Some(category)) => {
                // create room

                create_room(
                    http,
                    guild_id,
                    state.court_rooms.len(),
                    ChannelId::from_str(category).wrap_err("invalid channel_id stored")?,
                    mongo_client,
                )
                .await
                .wrap_err("create new room")?;

                Ok(Response::Simple("no free room? rip".to_owned()))
            }
            (None, None) => Ok(Response::Simple(
                "Zuerst eine Kategorie für die Gerichtsräume festlegen mit `/lawsuit set_category`"
                    .to_owned(),
            )),
        }
    }
}

async fn create_room(
    http: &Http,
    guild_id: GuildId,
    room_len: usize,
    category_id: ChannelId,
    mongo_client: &Mongo,
) -> Result<()> {
    let room_number = room_len + 1;
    let room_name = format!("gerichtsraum-{room_number}");
    let role_name = format!("Gerichtsprozess {room_number}");

    let guild = guild_id
        .to_partial_guild(http)
        .await
        .wrap_err("fetch partial guild")?;

    let court_role = guild
        .create_role(http, |role| {
            role.name(role_name).permissions(Permissions::empty())
        })
        .await
        .wrap_err("create role")?;

    let channel = guild
        .create_channel(http, |channel| {
            channel
                .name(room_name)
                .category(category_id)
                .permissions(vec![PermissionOverwrite {
                    allow: Permissions::SEND_MESSAGES,
                    deny: Permissions::empty(),
                    kind: PermissionOverwriteType::Role(court_role.id),
                }])
        })
        .await
        .wrap_err("create channel")?;

    let room = CourtRoom {
        channel_id: channel.id.to_string(),
        ongoing_lawsuit: false,
        role_id: court_role.id.to_string(),
    };

    mongo_client
        .add_court_room(&guild_id.to_string(), room)
        .await
        .wrap_err("add court room to database")?;

    info!(guild_id = %guild_id, channel_id = %channel.id, "Created new court room");

    Ok(())
}
