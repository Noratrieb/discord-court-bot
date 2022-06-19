use std::fmt::{Debug, Display, Formatter};

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
    Mongo, Report, WrapErr,
};

fn slash_commands(commands: &mut CreateApplicationCommands) -> &mut CreateApplicationCommands {
    commands.create_application_command(|command| {
        command
            .name("lawsuit")
            .description("Einen Gerichtsprozess starten")
            .create_option(|option| {
                option
                    .name("clear")
                    .description("Alle Rechtsprozessdaten löschen")
                    .kind(ApplicationCommandOptionType::SubCommand)
            })
    })
}

pub struct Handler {
    pub dev_guild_id: Option<GuildId>,
    pub set_global_commands: bool,
    pub mongo: Mongo,
}

impl Debug for Handler {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("HandlerData")
    }
}

pub enum Response {
    EphemeralStr(&'static str),
    Ephemeral(String),
    NoPermissions,
}

impl Display for Response {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EphemeralStr(str) => f.write_str(str),
            Self::Ephemeral(str) => f.write_str(str),
            Self::NoPermissions => f.write_str("du häsch kei recht für da!"),
        }
    }
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
}

impl Handler {
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
}

#[poise::command(
    slash_command,
    subcommands("prison_set_role", "prison_arrest", "prison_release")
)]
async fn lawsuit(_: crate::Context<'_>) -> color_eyre::Result<()> {
    unreachable!()
}

/// Einen neuen Gerichtsprozess erstellen
#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
async fn lawsuit_create(
    ctx: crate::Context<'_>,
    #[description = "Der Kläger"] plaintiff: User,
    #[description = "Der Angeklagte"] accused: User,
    #[description = "Der Richter"] judge: User,
    #[description = "Der Grund für die Klage"] reason: String,
    #[description = "Der Anwalt des Klägers"] plaintiff_lawyer: Option<User>,
    #[description = "Der Anwalt des Angeklagten"] accused_lawyer: Option<User>,
) -> color_eyre::Result<()> {
    lawsuit_create_impl(
        ctx,
        plaintiff,
        accused,
        judge,
        reason,
        plaintiff_lawyer,
        accused_lawyer,
    )
    .await
    .wrap_err("lawsuit_create")
}

/// Die Rolle für Gefangene setzen
#[poise::command(
    slash_command,
    required_permissions = "MANAGE_GUILD",
    on_error = "error_handler"
)]
async fn lawsuit_set_category(
    ctx: crate::Context<'_>,
    #[description = "Die Kategorie"] category: Channel,
) -> color_eyre::Result<()> {
    lawsuit_set_category_impl(ctx, category)
        .await
        .wrap_err("lawsuit_set_category")
}

/// Den Gerichtsprozess abschliessen und ein Urteil fällen
#[poise::command(
    slash_command,
    required_permissions = "MANAGE_GUILD",
    on_error = "error_handler"
)]
async fn lawsuit_close(
    ctx: crate::Context<'_>,
    #[description = "Das Urteil"] verdict: String,
) -> color_eyre::Result<()> {
    lawsuit_close_impl(ctx, verdict)
        .await
        .wrap_err("lawsuit_close")
}

/// Alle Rechtsprozessdaten löschen
#[poise::command(
    slash_command,
    required_permissions = "MANAGE_GUILD",
    on_error = "error_handler"
)]
async fn lawsuit_clear(ctx: crate::Context<'_>) -> color_eyre::Result<()> {
    lawsuit_clear_impl(ctx).await.wrap_err("lawsuit_clear")
}

#[poise::command(
    slash_command,
    subcommands("prison_set_role", "prison_arrest", "prison_release")
)]
async fn prison(_: crate::Context<'_>) -> color_eyre::Result<()> {
    unreachable!()
}

/// Die Rolle für Gefangene setzen
#[poise::command(
    slash_command,
    required_permissions = "MANAGE_GUILD",
    on_error = "error_handler"
)]
async fn prison_set_role(
    ctx: crate::Context<'_>,
    #[description = "Die Rolle"] role: Role,
) -> color_eyre::Result<()> {
    prison_set_role_impl(ctx, role)
        .await
        .wrap_err("prison_set_role")
}

/// Jemanden einsperren
#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
async fn prison_arrest(
    ctx: crate::Context<'_>,
    #[description = "Die Person zum einsperren"] user: User,
) -> color_eyre::Result<()> {
    prison_arrest_impl(ctx, user)
        .await
        .wrap_err("prison_arrest")
}

/// Einen Gefangenen freilassen
#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
async fn prison_release(
    ctx: crate::Context<'_>,
    #[description = "Die Person zum freilassen"] user: User,
) -> color_eyre::Result<()> {
    prison_release_impl(ctx, user)
        .await
        .wrap_err("prison_release")
}

async fn lawsuit_create_impl(
    ctx: crate::Context<'_>,
    plaintiff: User,
    accused: User,
    judge: User,
    reason: String,
    plaintiff_lawyer: Option<User>,
    accused_lawyer: Option<User>,
) -> color_eyre::Result<()> {
    let guild_id = ctx.guild_id().wrap_err("guild_id not found")?;

    let lawsuit = Lawsuit {
        id: Uuid::new(),
        plaintiff: plaintiff.id.into(),
        accused: accused.id.into(),
        judge: judge.id.into(),
        plaintiff_lawyer: plaintiff_lawyer.map(|user| user.id.into()),
        accused_lawyer: accused_lawyer.map(|user| user.id.into()),
        reason: reason.to_owned(),
        verdict: None,
        court_room: SnowflakeId(0),
    };

    let lawsuit_ctx = LawsuitCtx {
        lawsuit,
        mongo_client: ctx.data().mongo.clone(),
        http: ctx.discord().http.clone(),
        guild_id,
    };

    let response = lawsuit_ctx
        .initialize()
        .await
        .wrap_err("initialize lawsuit")?;

    ctx.say(response.to_string()).await?;

    Ok(())
}

async fn lawsuit_set_category_impl(
    ctx: crate::Context<'_>,
    category: Channel,
) -> color_eyre::Result<()> {
    let guild_id = ctx.guild_id().wrap_err("guild_id not found")?;

    //let channel = channel
    //    .id
    //    .to_channel(&ctx.http)
    //    .await
    //    .wrap_err("fetch category for set_category")?;
    match category.category() {
        Some(category) => {
            let id = category.id;
            ctx.data()
                .mongo
                .set_court_category(guild_id.into(), id.into())
                .await?;
            ctx.say("isch gsetzt").await?;
        }
        None => {
            ctx.say("Das ist keine Kategorie!").await?;
        }
    }

    Ok(())
}

async fn lawsuit_close_impl(ctx: crate::Context<'_>, verdict: String) -> color_eyre::Result<()> {
    let guild_id = ctx.guild_id().wrap_err("guild_id not found")?;

    let member = ctx.author_member().await.wrap_err("member not found")?;
    let permission_override = member
        .permissions
        .wrap_err("permissions not found")?
        .contains(Permissions::MANAGE_GUILD);

    let room_id = ctx.channel_id();
    let mongo_client = &ctx.data().mongo;

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
            ctx.say("i dem channel lauft kein aktive prozess!").await?;
            return Ok(());
        }
    };

    let room = state
        .court_rooms
        .iter()
        .find(|r| r.channel_id == room_id.into());
    let room = match room {
        Some(room) => room.clone(),
        None => {
            ctx.say("i dem channel lauft kein aktive prozess!").await?;
            return Ok(());
        }
    };

    let mut lawsuit_ctx = LawsuitCtx {
        lawsuit,
        mongo_client: mongo_client.clone(),
        http: ctx.discord().http.clone(),
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
        ctx.say(response.to_string()).await?;
        return Ok(());
    }

    ctx.say("ich han en dir abschlosse").await?;

    Ok(())
}

async fn lawsuit_clear_impl(ctx: crate::Context<'_>) -> color_eyre::Result<()> {
    let guild_id = ctx.guild_id().wrap_err("guild_id not found")?;

    ctx.data().mongo.delete_guild(guild_id.into()).await?;
    ctx.say("alles weg").await?;
    Ok(())
}

async fn prison_set_role_impl(ctx: crate::Context<'_>, role: Role) -> color_eyre::Result<()> {
    ctx.data()
        .mongo
        .set_prison_role(
            ctx.guild_id().wrap_err("guild_id not found")?.into(),
            role.id.into(),
        )
        .await?;

    ctx.say("isch gsetzt").await.wrap_err("reply")?;

    Ok(())
}

async fn prison_arrest_impl(ctx: crate::Context<'_>, user: User) -> color_eyre::Result<()> {
    let mongo_client = &ctx.data().mongo;
    let guild_id = ctx.guild_id().wrap_err("guild_id not found")?;
    let http = &ctx.discord().http;

    let state = mongo_client.find_or_insert_state(guild_id.into()).await?;
    let role = state.prison_role;

    let role = match role {
        Some(role) => role,
        None => {
            ctx.say("du mosch zerst e rolle setze mit /prison set_role")
                .await?;
            return Ok(());
        }
    };

    mongo_client
        .add_to_prison(guild_id.into(), user.id.into())
        .await?;

    guild_id
        .member(http, user.id)
        .await
        .wrap_err("fetching guild member")?
        .add_role(http, role)
        .await
        .wrap_err("add guild member role")?;
    Ok(())
}

async fn prison_release_impl(ctx: crate::Context<'_>, user: User) -> color_eyre::Result<()> {
    let mongo_client = &ctx.data().mongo;
    let guild_id = ctx.guild_id().wrap_err("guild_id not found")?;
    let http = &ctx.discord().http;

    let state = mongo_client.find_or_insert_state(guild_id.into()).await?;
    let role = state.prison_role;

    let role = match role {
        Some(role) => role,
        None => {
            ctx.say("du mosch zerst e rolle setze mit /prison set_role")
                .await?;
            return Ok(());
        }
    };

    mongo_client
        .remove_from_prison(guild_id.into(), user.id.into())
        .await?;

    guild_id
        .member(http, user.id)
        .await
        .wrap_err("fetching guild member")?
        .remove_role(http, role)
        .await
        .wrap_err("remove guild member role")?;

    ctx.say("d'freiheit wartet").await?;

    Ok(())
}

async fn error_handler(error: poise::FrameworkError<'_, Handler, Report>) {
    error!(?error, "Error during command execution");
}
