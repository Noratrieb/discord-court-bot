use std::sync::Arc;

use color_eyre::Result;
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use serenity::{
    http::Http,
    model::{channel::PermissionOverwriteType, prelude::*, Permissions},
};
use tracing::{error, info};

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
    pub court_room: SnowflakeId,
}

pub struct LawsuitCtx {
    pub lawsuit: Lawsuit,
    pub mongo_client: Mongo,
    pub http: Arc<Http>,
    pub guild_id: GuildId,
}

impl LawsuitCtx {
    pub async fn initialize(mut self) -> Result<Response> {
        let state = self
            .mongo_client
            .find_or_insert_state(self.guild_id.into())
            .await?;

        let free_room = state
            .court_rooms
            .iter()
            .find(|r| !r.ongoing_lawsuit)
            .cloned();

        let room = match (free_room, &state.court_category) {
            (Some(room), _) => room,
            (None, Some(category)) => {
                // create room

                let result = self
                    .create_room(state.court_rooms.len(), *category)
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

        let result = self
            .send_process_open_message(&self.http, self.guild_id, &room)
            .await
            .wrap_err("send process open message")?;

        if let Err(response) = result {
            return Ok(response);
        }

        let channel_id = room.channel_id;
        self.lawsuit.court_room = channel_id;

        tokio::spawn(async move {
            if let Err(err) = self.setup(room).await {
                error!(?err, "Error setting up lawsuit");
            }
        });

        Ok(Response::Simple(format!(
            "ha eine ufgmacht im channel <#{}>",
            channel_id
        )))
    }

    async fn setup(&self, room: CourtRoom) -> Result<()> {
        let Self {
            mongo_client,
            http,
            guild_id,
            lawsuit,
        } = self;
        let guild_id = *guild_id;

        mongo_client.add_lawsuit(guild_id.into(), lawsuit).await?;
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
        assign_role(lawsuit.accused, &http, guild_id, room.role_id).await?;
        if let Some(accused_lawyer) = lawsuit.accused_lawyer {
            assign_role(accused_lawyer, &http, guild_id, room.role_id).await?;
        }
        assign_role(lawsuit.plaintiff, &http, guild_id, room.role_id).await?;
        if let Some(plaintiff_lawyer) = lawsuit.plaintiff_lawyer {
            assign_role(plaintiff_lawyer, &http, guild_id, room.role_id).await?;
        }
        assign_role(lawsuit.judge, &http, guild_id, room.role_id).await?;

        info!(?lawsuit, "Created lawsuit");

        Ok(())
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
                            let lawsuit = &self.lawsuit;
                            embed
                                .title("Prozess")
                                .field("Grund", &lawsuit.reason, false)
                                .field("Kläger", format!("<@{}>", lawsuit.plaintiff), false)
                                .field(
                                    "Anwalt des Klägers",
                                    match &lawsuit.plaintiff_lawyer {
                                        Some(lawyer) => format!("<@{}>", lawyer),
                                        None => "TBD".to_string(),
                                    },
                                    false,
                                )
                                .field("Angeklagter", format!("<@{}>", lawsuit.accused), false)
                                .field(
                                    "Anwalt des Angeklagten",
                                    match &lawsuit.accused_lawyer {
                                        Some(lawyer) => format!("<@{}>", lawyer),
                                        None => "TBD".to_string(),
                                    },
                                    false,
                                )
                                .field("Richter", format!("<@{}>", lawsuit.judge), false)
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

    async fn create_room(
        &self,
        room_len: usize,
        category_id: SnowflakeId,
    ) -> Result<Result<CourtRoom, Response>> {
        let room_number = room_len + 1;
        let room_name = format!("gerichtsraum-{room_number}");
        let role_name = format!("Gerichtsprozess {room_number}");

        let guild = self
            .guild_id
            .to_partial_guild(&self.http)
            .await
            .wrap_err("fetch partial guild")?;

        let role_id = match guild.role_by_name(&role_name) {
            Some(role) => role.id,
            None => {
                guild
                    .create_role(&self.http, |role| {
                        role.name(role_name).permissions(Permissions::empty())
                    })
                    .await
                    .wrap_err("create role")?
                    .id
            }
        };

        let channels = guild
            .channels(&self.http)
            .await
            .wrap_err("fetching channels")?;

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
                    .create_channel(&self.http, |channel| {
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

        self.mongo_client
            .add_court_room(self.guild_id.into(), &room)
            .await
            .wrap_err("add court room to database")?;

        info!(guild_id = %self.guild_id, channel_id = %channel_id, "Created new court room");

        Ok(Ok(room))
    }
}
