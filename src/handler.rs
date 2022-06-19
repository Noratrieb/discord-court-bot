use color_eyre::eyre::{eyre, ContextCompat};
use mongodb::bson::Uuid;
use serenity::{
    async_trait,
    builder::CreateApplicationCommands,
    model::{
        interactions::application_command::ApplicationCommandOptionType,
        prelude::{application_command::*, *},
    },
    prelude::*,
};
use tracing::{debug, error, info};

use crate::{
    lawsuit::{Lawsuit, LawsuitCtx},
    model::SnowflakeId,
    Mongo, WrapErr,
};

fn slash_commands(commands: &mut CreateApplicationCommands) -> &mut CreateApplicationCommands {
    commands
        .create_application_command(|command| {
            command
                .name("lawsuit")
                .description("Einen Gerichtsprozess starten")
                .create_option(|option| {
                    option
                        .name("create")
                        .description("Einen neuen Gerichtsprozess anfangen")
                        .kind(ApplicationCommandOptionType::SubCommand)
                        .create_sub_option(|option| {
                            option
                                .name("plaintiff")
                                .description("Der Kläger")
                                .kind(ApplicationCommandOptionType::User)
                                .required(true)
                        })
                        .create_sub_option(|option| {
                            option
                                .name("accused")
                                .description("Der Angeklagte")
                                .kind(ApplicationCommandOptionType::User)
                                .required(true)
                        })
                        .create_sub_option(|option| {
                            option
                                .name("judge")
                                .description("Der Richter")
                                .kind(ApplicationCommandOptionType::User)
                                .required(true)
                        })
                        .create_sub_option(|option| {
                            option
                                .name("reason")
                                .description("Der Grund für die Klage")
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                        .create_sub_option(|option| {
                            option
                                .name("plaintiff_lawyer")
                                .description("Der Anwalt des Klägers")
                                .kind(ApplicationCommandOptionType::User)
                                .required(false)
                        })
                        .create_sub_option(|option| {
                            option
                                .name("accused_lawyer")
                                .description("Der Anwalt des Angeklagten")
                                .kind(ApplicationCommandOptionType::User)
                                .required(false)
                        })
                })
                .create_option(|option| {
                    option
                        .name("set_category")
                        .description("Die Gerichtskategorie setzen")
                        .kind(ApplicationCommandOptionType::SubCommand)
                        .create_sub_option(|option| {
                            option
                                .name("category")
                                .description("Die Kategorie")
                                .kind(ApplicationCommandOptionType::Channel)
                                .required(true)
                        })
                })
                .create_option(|option| {
                    option
                        .name("close")
                        .description("Den Prozess abschliessen")
                        .kind(ApplicationCommandOptionType::SubCommand)
                        .create_sub_option(|option| {
                            option
                                .name("verdict")
                                .description("Das Urteil")
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                })
                .create_option(|option| {
                    option
                        .name("clear")
                        .description("Alle Rechtsprozessdaten löschen")
                        .kind(ApplicationCommandOptionType::SubCommand)
                })
        })
        .create_application_command(|command| {
            command
                .name("prison")
                .description("Leute im Gefängnis einsperren")
                .create_option(|option| {
                    option
                        .name("arrest")
                        .description("Jemanden einsperren")
                        .kind(ApplicationCommandOptionType::SubCommand)
                        .create_sub_option(|option| {
                            option
                                .name("user")
                                .description("Die Person zum einsperren")
                                .kind(ApplicationCommandOptionType::User)
                                .required(true)
                        })
                })
                .create_option(|option| {
                    option
                        .name("release")
                        .description("Jemanden freilassen")
                        .kind(ApplicationCommandOptionType::SubCommand)
                        .create_sub_option(|option| {
                            option
                                .name("user")
                                .description("Die Person zum freilassen")
                                .kind(ApplicationCommandOptionType::User)
                                .required(true)
                        })
                })
                .create_option(|option| {
                    option
                        .name("set_role")
                        .description("Die Rolle für Gefangene setzen")
                        .kind(ApplicationCommandOptionType::SubCommand)
                        .create_sub_option(|option| {
                            option
                                .name("role")
                                .description("Die Rolle")
                                .kind(ApplicationCommandOptionType::Role)
                                .required(true)
                        })
                })
        })
}

pub struct Handler {
    pub dev_guild_id: Option<GuildId>,
    pub set_global_commands: bool,
    pub mongo: Mongo,
}

pub enum Response {
    EphemeralStr(&'static str),
    Ephemeral(String),
    NoPermissions,
}

#[async_trait]
impl EventHandler for Handler {
    async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
        if let Err(err) = self.handle_guild_member_join(ctx, new_member).await {
            error!(?err, "An error occurred in guild_member_addition handler");
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(name = %ready.user.name, "Bot is connected!");

        if let Some(guild_id) = self.dev_guild_id {
            let guild_commands =
                GuildId::set_application_commands(&guild_id, &ctx.http, slash_commands).await;

            match guild_commands {
                Ok(_) => info!("Installed guild slash commands"),
                Err(error) => error!(?error, "Failed to create global commands"),
            }
        }

        if self.set_global_commands {
            let guild_commands =
                ApplicationCommand::set_global_application_commands(&ctx.http, slash_commands)
                    .await;
            match guild_commands {
                Ok(commands) => info!(?commands, "Created global commands"),
                Err(error) => error!(?error, "Failed to create global commands"),
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            if let Err(err) = self.handle_interaction(ctx, command).await {
                error!(?err, "An error occurred in interaction_create handler");
            }
        }
    }
}
impl Handler {
    async fn handle_interaction(
        &self,
        ctx: Context,
        command: ApplicationCommandInteraction,
    ) -> color_eyre::Result<()> {
        debug!(name = %command.data.name, "Received command interaction");

        let response = match command.data.name.as_str() {
            "lawsuit" => lawsuit_command_handler(&command, &ctx, &self.mongo).await,
            "prison" => prison_command_handler(&command, &ctx, &self.mongo).await,
            _ => Ok(Response::EphemeralStr("not implemented :(")),
        };

        match response {
            Ok(response) => self.send_response(ctx, command, response).await,
            Err(err) => {
                error!(?err, "Error during command execution");
                self.send_response(
                    ctx,
                    command,
                    Response::EphemeralStr("An internal error occurred"),
                )
                .await
            }
        }
    }

    async fn handle_guild_member_join(
        &self,
        ctx: Context,
        mut member: Member,
    ) -> color_eyre::Result<()> {
        let guild_id = member.guild_id;
        let user_id = member.user.id;
        let state = self.mongo.find_or_insert_state(guild_id.into()).await?;

        debug!(member = ?member.user.id, "New member joined");

        if let Some(role_id) = state.prison_role {
            if self
                .mongo
                .find_prison_entry(guild_id.into(), user_id.into())
                .await?
                .is_some()
            {
                info!("New member was in prison, giving them the prison role");

                member
                    .add_role(&ctx.http, role_id)
                    .await
                    .wrap_err("add role to member in prison")?;
            }
        }

        Ok(())
    }

    async fn send_response(
        &self,
        ctx: Context,
        command: ApplicationCommandInteraction,
        response: Response,
    ) -> color_eyre::Result<()> {
        command
            .create_interaction_response(&ctx.http, |res| match response {
                Response::EphemeralStr(content) => res
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content(content)
                            .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                    }),
                Response::Ephemeral(content) => res
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content(content)
                            .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                    }),
                Response::NoPermissions => res
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content("du häsch kei recht für da!")
                            .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                    }),
            })
            .await
            .wrap_err("sending response")?;
        Ok(())
    }
}

async fn lawsuit_command_handler(
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    mongo_client: &Mongo,
) -> color_eyre::Result<Response> {
    let options = &command.data.options;
    let subcommand = options.get(0).wrap_err("needs subcommand")?;

    let options = &subcommand.options;
    let guild_id = command.guild_id.wrap_err("guild_id not found")?;

    let member = command
        .member
        .as_ref()
        .wrap_err("command must be used my member")?;
    let permissions = member.permissions.wrap_err("must be in interaction")?;

    match subcommand.name.as_str() {
        "create" => {
            if !permissions.contains(Permissions::MANAGE_GUILD) {
                return Ok(Response::NoPermissions);
            }

            let plaintiff = UserOption::get(options.get(0)).wrap_err("plaintiff")?;
            let accused = UserOption::get(options.get(1)).wrap_err("accused")?;
            let judge = UserOption::get(options.get(2)).wrap_err("judge")?;
            let reason = StringOption::get(options.get(3)).wrap_err("reason")?;
            let plaintiff_layer =
                UserOption::get_optional(options.get(4)).wrap_err("plaintiff_layer")?;
            let accused_layer =
                UserOption::get_optional(options.get(5)).wrap_err("accused_layer")?;

            let lawsuit = Lawsuit {
                id: Uuid::new(),
                plaintiff: plaintiff.0.id.into(),
                accused: accused.0.id.into(),
                judge: judge.0.id.into(),
                plaintiff_lawyer: plaintiff_layer.map(|user| user.0.id.into()),
                accused_lawyer: accused_layer.map(|user| user.0.id.into()),
                reason: reason.to_owned(),
                verdict: None,
                court_room: SnowflakeId(0),
            };

            let lawsuit_ctx = LawsuitCtx {
                lawsuit,
                mongo_client: mongo_client.clone(),
                http: ctx.http.clone(),
                guild_id,
            };

            let response = lawsuit_ctx
                .initialize()
                .await
                .wrap_err("initialize lawsuit")?;

            Ok(response)
        }
        "set_category" => {
            if !permissions.contains(Permissions::MANAGE_GUILD) {
                return Ok(Response::NoPermissions);
            }

            let channel = ChannelOption::get(options.get(0))?;

            let channel = channel
                .id
                .to_channel(&ctx.http)
                .await
                .wrap_err("fetch category for set_category")?;
            match channel.category() {
                Some(category) => {
                    let id = category.id;
                    mongo_client
                        .set_court_category(guild_id.into(), id.into())
                        .await?;
                }
                None => return Ok(Response::EphemeralStr("Das ist keine Kategorie!")),
            }

            Ok(Response::EphemeralStr("isch gsetzt"))
        }
        "close" => {
            let permission_override = permissions.contains(Permissions::MANAGE_GUILD);

            let verdict = StringOption::get(options.get(0))?;

            let room_id = command.channel_id;

            let state = mongo_client
                .find_or_insert_state(guild_id.into())
                .await
                .wrap_err("find guild for verdict")?;

            let lawsuit = state
                .lawsuits
                .iter()
                .find(|l| l.court_room == room_id.into() && l.verdict.is_none());

            let lawsuit = match lawsuit {
                Some(lawsuit) => lawsuit.clone(),
                None => {
                    return Ok(Response::EphemeralStr(
                        "i dem channel lauft kein aktive prozess!",
                    ))
                }
            };

            let room = state
                .court_rooms
                .iter()
                .find(|r| r.channel_id == room_id.into());
            let room = match room {
                Some(room) => room.clone(),
                None => {
                    return Ok(Response::EphemeralStr(
                        "i dem channel lauft kein aktive prozess!",
                    ))
                }
            };

            let mut lawsuit_ctx = LawsuitCtx {
                lawsuit,
                mongo_client: mongo_client.clone(),
                http: ctx.http.clone(),
                guild_id,
            };

            let response = lawsuit_ctx
                .rule_verdict(
                    permission_override,
                    member.user.id,
                    verdict.to_string(),
                    room,
                )
                .await?;

            if let Err(response) = response {
                return Ok(response);
            }

            Ok(Response::EphemeralStr("ich han en dir abschlosse"))
        }
        "clear" => {
            if !permissions.contains(Permissions::MANAGE_GUILD) {
                return Ok(Response::NoPermissions);
            }

            mongo_client.delete_guild(guild_id.into()).await?;
            Ok(Response::EphemeralStr("alles weg"))
        }
        _ => Err(eyre!("Unknown subcommand")),
    }
}

async fn prison_command_handler(
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    mongo_client: &Mongo,
) -> color_eyre::Result<Response> {
    let options = &command.data.options;
    let subcommand = options.get(0).wrap_err("needs subcommand")?;

    let options = &subcommand.options;
    let guild_id = command.guild_id.wrap_err("guild_id not found")?;

    let member = command
        .member
        .as_ref()
        .wrap_err("command must be used my member")?;
    let permissions = member.permissions.wrap_err("must be in interaction")?;

    match subcommand.name.as_str() {
        "set_role" => {
            if !permissions.contains(Permissions::MANAGE_GUILD) {
                return Ok(Response::NoPermissions);
            }

            let role = RoleOption::get(options.get(0))?;

            mongo_client
                .set_prison_role(guild_id.into(), role.id.into())
                .await?;

            Ok(Response::EphemeralStr("isch gsetzt"))
        }
        "arrest" => {
            if !permissions.contains(Permissions::MANAGE_GUILD) {
                return Ok(Response::NoPermissions);
            }

            let (user, _) = UserOption::get(options.get(0))?;

            let state = mongo_client.find_or_insert_state(guild_id.into()).await?;
            let role = state.prison_role;

            let role = match role {
                Some(role) => role,
                None => {
                    return Ok(Response::EphemeralStr(
                        "du mosch zerst e rolle setze mit /prison set_role",
                    ))
                }
            };

            mongo_client
                .add_to_prison(guild_id.into(), user.id.into())
                .await?;

            guild_id
                .member(&ctx.http, user.id)
                .await
                .wrap_err("fetching guild member")?
                .add_role(&ctx.http, role)
                .await
                .wrap_err("add guild member role")?;

            Ok(Response::EphemeralStr("hani igsperrt"))
        }
        "release" => {
            if !permissions.contains(Permissions::MANAGE_GUILD) {
                return Ok(Response::NoPermissions);
            }

            let (user, _) = UserOption::get(options.get(0))?;

            let state = mongo_client.find_or_insert_state(guild_id.into()).await?;
            let role = state.prison_role;

            let role = match role {
                Some(role) => role,
                None => {
                    return Ok(Response::EphemeralStr(
                        "du mosch zerst e rolle setze mit /prison set_role",
                    ))
                }
            };

            mongo_client
                .remove_from_prison(guild_id.into(), user.id.into())
                .await?;

            guild_id
                .member(&ctx.http, user.id)
                .await
                .wrap_err("fetching guild member")?
                .remove_role(&ctx.http, role)
                .await
                .wrap_err("remove guild member role")?;

            Ok(Response::EphemeralStr("d'freiheit wartet"))
        }
        _ => Err(eyre!("Unknown subcommand")),
    }
}

#[nougat::gat]
trait GetOption {
    type Get<'a>;

    fn extract(
        command: &ApplicationCommandInteractionDataOptionValue,
    ) -> color_eyre::Result<Self::Get<'_>>;

    fn get(
        option: Option<&ApplicationCommandInteractionDataOption>,
    ) -> color_eyre::Result<Self::Get<'_>> {
        let option = Self::get_optional(option);
        match option {
            Ok(Some(get)) => Ok(get),
            Ok(None) => Err(eyre!("Expected value!")),
            Err(err) => Err(err),
        }
    }
    fn get_optional(
        option: Option<&ApplicationCommandInteractionDataOption>,
    ) -> color_eyre::Result<Option<Self::Get<'_>>> {
        if let Some(option) = option {
            if let Some(command) = option.resolved.as_ref() {
                Self::extract(command).map(Some)
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

struct UserOption;

#[nougat::gat]
impl GetOption for UserOption {
    type Get<'a> = (&'a User, &'a Option<PartialMember>);

    fn extract(
        command: &ApplicationCommandInteractionDataOptionValue,
    ) -> crate::Result<Self::Get<'_>> {
        if let ApplicationCommandInteractionDataOptionValue::User(user, member) = command {
            Ok((user, member))
        } else {
            Err(eyre!("Expected user!"))
        }
    }
}

struct StringOption;

#[nougat::gat]
impl GetOption for StringOption {
    type Get<'a> = &'a str;

    fn extract(
        command: &ApplicationCommandInteractionDataOptionValue,
    ) -> crate::Result<Self::Get<'_>> {
        if let ApplicationCommandInteractionDataOptionValue::String(str) = command {
            Ok(str)
        } else {
            Err(eyre!("Expected string!"))
        }
    }
}

struct ChannelOption;

#[nougat::gat]
impl GetOption for ChannelOption {
    type Get<'a> = &'a PartialChannel;

    fn extract(
        command: &ApplicationCommandInteractionDataOptionValue,
    ) -> crate::Result<Self::Get<'_>> {
        if let ApplicationCommandInteractionDataOptionValue::Channel(channel) = command {
            Ok(channel)
        } else {
            Err(eyre!("Expected string!"))
        }
    }
}

struct RoleOption;

#[nougat::gat]
impl GetOption for RoleOption {
    type Get<'a> = &'a Role;

    fn extract(
        command: &ApplicationCommandInteractionDataOptionValue,
    ) -> crate::Result<Self::Get<'_>> {
        if let ApplicationCommandInteractionDataOptionValue::Role(role) = command {
            Ok(role)
        } else {
            Err(eyre!("Expected string!"))
        }
    }
}
