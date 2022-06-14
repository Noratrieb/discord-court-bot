use color_eyre::eyre::{eyre, ContextCompat};
use serenity::{
    async_trait,
    builder::CreateApplicationCommands,
    http::Http,
    model::{
        interactions::application_command::ApplicationCommandOptionType,
        prelude::{application_command::*, *},
    },
    prelude::*,
};
use tracing::{debug, error, info};

use crate::{
    lawsuit::{Lawsuit, LawsuitState},
    Mongo, WrapErr,
};

fn slash_commands(commands: &mut CreateApplicationCommands) -> &mut CreateApplicationCommands {
    commands.create_application_command(|command| {
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
    })
}

pub struct Handler {
    pub dev_guild_id: Option<GuildId>,
    pub set_global_commands: bool,
    pub mongo: Mongo,
}

#[async_trait]
impl EventHandler for Handler {
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
            todo!()
            // let guild_commands =
            //     ApplicationCommand::create_global_application_command(&ctx.http, slash_commands)
            //         .await;
            // match guild_commands {
            //     Ok(commands) => info!(?commands, "Created global commands"),
            //     Err(error) => error!(?error, "Failed to create global commands"),
            // }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            debug!(name = %command.data.name, "Received command interaction");

            let result = match command.data.name.as_str() {
                "lawsuit" => {
                    let result = lawsuit_command_handler(&command, &ctx.http, &self.mongo).await;
                    if let Err(err) = result {
                        error!(?err, "Error processing response");
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message.content("An error occurred")
                                    })
                            })
                            .await
                            .wrap_err("error response")
                    } else {
                        Ok(())
                    }
                }
                _ => command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("not implemented :(")
                            })
                    })
                    .await
                    .wrap_err("not implemented response"),
            };
            if let Err(err) = result {
                error!(?err, "Error sending response");
            }
        }
    }
}

async fn lawsuit_command_handler(
    command: &ApplicationCommandInteraction,
    http: impl AsRef<Http>,
    mongo_client: &Mongo,
) -> color_eyre::Result<()> {
    let options = &command.data.options;
    let subcomamnd = options.get(0).wrap_err("needs subcommand")?;

    match subcomamnd.name.as_str() {
        "create" => {
            let options = &subcomamnd.options;
            let plaintiff = UserOption::get(options.get(0)).wrap_err("plaintiff")?;
            let accused = UserOption::get(options.get(1)).wrap_err("accused")?;
            let reason = StringOption::get(options.get(2)).wrap_err("reason")?;
            let plaintiff_layer =
                UserOption::get_optional(options.get(3)).wrap_err("plaintiff_layer")?;
            let accused_layer =
                UserOption::get_optional(options.get(4)).wrap_err("accused_layer")?;

            let mut lawsuit = Lawsuit {
                plaintiff: plaintiff.0.id,
                accused: accused.0.id,
                plaintiff_layer: plaintiff_layer.map(|l| l.0.id),
                accused_layer: accused_layer.map(|l| l.0.id),
                reason: reason.to_owned(),
                state: LawsuitState::Initial,
                court_room: None,
            };

            lawsuit
                .initialize(
                    command.guild_id.wrap_err("guild_id not found")?.to_string(),
                    mongo_client,
                )
                .await
                .wrap_err("initialize lawsuit")?;

            info!(?lawsuit, "Created lawsuit");

            command
                .create_interaction_response(http, |res| {
                    res.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("hani erstellt, keis problem")
                        })
                })
                .await
                .wrap_err("success reponse")?;
            Ok(())
        }
        "set_category" => Ok(()),
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