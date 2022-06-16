use color_eyre::Result;
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use serenity::{
    http::Http,
    model::{channel::PermissionOverwriteType, prelude::*, Permissions},
};
use tracing::info;

use crate::{
    handler::Response,
    model::{CourtRoom, SnowflakeId},
    Mongo, WrapErr,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LawsuitState {
    Initial,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lawsuit {
    pub plaintiff: SnowflakeId,
    pub accused: SnowflakeId,
    pub plaintiff_lawyer: Option<SnowflakeId>,
    pub accused_lawyer: Option<SnowflakeId>,
    pub judge: SnowflakeId,
    pub reason: String,
    pub state: LawsuitState,
    pub court_room: Option<SnowflakeId>,
}

impl Lawsuit {
    pub async fn initialize(
        &mut self,
        http: &Http,
        guild_id: GuildId,
        mongo_client: &Mongo,
    ) -> Result<Response> {
        let state = mongo_client.find_or_insert_state(guild_id.into()).await?;

        let free_room = state
            .court_rooms
            .iter()
            .find(|r| !r.ongoing_lawsuit)
            .cloned();

        let room = match (free_room, &state.court_category) {
            (Some(room), _) => room,
            (None, Some(category)) => {
                // create room

                let result = create_room(
                    http,
                    guild_id,
                    state.court_rooms.len(),
                    *category,
                    mongo_client,
                )
                .await
                .wrap_err("create new room")?;

                match result {
                    Err(res) => return Ok(res),
                    Ok(room) => room,
                }
            }
            (None, None) => return Ok(Response::Simple(
                "Zuerst eine Kategorie für die Gerichtsräume festlegen mit `/lawsuit set_category`"
                    .to_owned(),
            )),
        };

        self.court_room = Some(room.channel_id);

        let result = self
            .send_process_open_message(http, guild_id, &room)
            .await
            .wrap_err("send process open message")?;

        if let Err(response) = result {
            return Ok(response);
        }

        mongo_client.add_lawsuit(guild_id.into(), self).await?;
        mongo_client
            .set_court_room(
                guild_id.into(),
                room.channel_id,
                doc! { "court_rooms.$.ongoing_lawsuit": true },
            )
            .await?;

        async fn assign_role(
            user: SnowflakeId,
            http: &Http,
            guild_id: GuildId,
            role_id: SnowflakeId,
        ) -> Result<()> {
            let mut member = guild_id.member(http, user).await.wrap_err("fetch member")?;
            member
                .add_role(http, role_id)
                .await
                .wrap_err("add role to member")?;

            Ok(())
        }

        assign_role(self.accused, http, guild_id, room.role_id).await?;
        if let Some(accused_lawyer) = self.accused_lawyer {
            assign_role(accused_lawyer, http, guild_id, room.role_id).await?;
        }
        assign_role(self.plaintiff, http, guild_id, room.role_id).await?;
        if let Some(plaintiff_lawyer) = self.plaintiff_lawyer {
            assign_role(plaintiff_lawyer, http, guild_id, room.role_id).await?;
        }
        assign_role(self.judge, http, guild_id, room.role_id).await?;

        Ok(Response::Simple(format!(
            "ha eine ufgmacht im channel <#{}>",
            room.channel_id
        )))
    }

    async fn send_process_open_message(
        &self,
        http: &Http,
        guild_id: GuildId,
        room: &CourtRoom,
    ) -> Result<Result<(), Response>> {
        let channels = guild_id
            .to_partial_guild(http)
            .await
            .wrap_err("fetch partial guild")?
            .channels(http)
            .await
            .wrap_err("fetch channels")?;
        let channel = channels.get(&room.channel_id.into());

        match channel {
            Some(channel) => {
                channel
                    .id
                    .send_message(http, |msg| {
                        msg.embed(|embed| {
                            embed
                                .title("Prozess")
                                .field("Grund", &self.reason, false)
                                .field("Kläger", format!("<@{}>", self.plaintiff), false)
                                .field(
                                    "Anwalt des Klägers",
                                    match &self.plaintiff_lawyer {
                                        Some(lawyer) => format!("<@{}>", lawyer),
                                        None => "TBD".to_string(),
                                    },
                                    false,
                                )
                                .field("Angeklagter", format!("<@{}>", self.accused), false)
                                .field(
                                    "Anwalt des Angeklagten",
                                    match &self.accused_lawyer {
                                        Some(lawyer) => format!("<@{}>", lawyer),
                                        None => "TBD".to_string(),
                                    },
                                    false,
                                )
                                .field("Richter", format!("<@{}>", self.judge), false)
                        })
                    })
                    .await
                    .wrap_err("send message")?;
            }
            None => {
                // todo: remove the court room from the db
                return Ok(Err(Response::Simple(
                    "i ha de channel zum de prozess öffne nöd gfunde".to_string(),
                )));
            }
        }

        Ok(Ok(()))
    }
}

async fn create_room(
    http: &Http,
    guild_id: GuildId,
    room_len: usize,
    category_id: SnowflakeId,
    mongo_client: &Mongo,
) -> Result<Result<CourtRoom, Response>> {
    let room_number = room_len + 1;
    let room_name = format!("gerichtsraum-{room_number}");
    let role_name = format!("Gerichtsprozess {room_number}");

    let guild = guild_id
        .to_partial_guild(http)
        .await
        .wrap_err("fetch partial guild")?;

    let role_id = match guild.role_by_name(&role_name) {
        Some(role) => role.id,
        None => {
            guild
                .create_role(http, |role| {
                    role.name(role_name).permissions(Permissions::empty())
                })
                .await
                .wrap_err("create role")?
                .id
        }
    };

    let channels = guild.channels(http).await.wrap_err("fetching channels")?;

    let channel_id = match channels.values().find(|c| c.name() == room_name) {
        Some(channel) => {
            if channel.parent_id != Some(category_id.into()) {
                return Ok(Err(Response::Simple(format!(
                    "de channel {room_name} isch i de falsche kategorie, man eh"
                ))));
            }
            channel.id
        }
        None => {
            guild
                .create_channel(http, |channel| {
                    channel
                        .name(room_name)
                        .category(category_id)
                        .permissions(vec![PermissionOverwrite {
                            allow: Permissions::SEND_MESSAGES,
                            deny: Permissions::empty(),
                            kind: PermissionOverwriteType::Role(role_id),
                        }])
                })
                .await
                .wrap_err("create channel")?
                .id
        }
    };

    let room = CourtRoom {
        channel_id: channel_id.into(),
        ongoing_lawsuit: false,
        role_id: role_id.into(),
    };

    mongo_client
        .add_court_room(guild_id.into(), &room)
        .await
        .wrap_err("add court room to database")?;

    info!(guild_id = %guild_id, channel_id = %channel_id, "Created new court room");

    Ok(Ok(room))
}
