extern crate core;

mod handler;
mod lawsuit;
mod model;

use std::env;

use color_eyre::{eyre::WrapErr, Report, Result};
use poise::{
    serenity_prelude as serenity,
    serenity_prelude::{Activity, GatewayIntents, GuildId},
};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

use crate::{handler::Handler, model::Mongo};

type Context<'a> = poise::Context<'a, Handler, Report>;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let _ = dotenv::dotenv();

    let pretty = env::var("PRETTY").is_ok();

    setup_tracing(pretty);

    info!("Starting up...");

    let mongo_uri = env::var("MONGO_URI").wrap_err("MONGO_URI not found in the environment")?;
    let db_name = env::var("DB_NAME").unwrap_or_else(|_| "court-bot".to_string());

    let username = env::var("MONGO_INITDB_ROOT_USERNAME")
        .wrap_err("MONGO_INITDB_ROOT_USERNAME not found in the environment")?;
    let password = env::var("MONGO_INITDB_ROOT_PASSWORD")
        .wrap_err("MONGO_INITDB_ROOT_PASSWORD not found in the environment")?;

    let mongo = Mongo::connect(&mongo_uri, &db_name, username, password).await?;

    info!("Connected to mongodb");

    let token = env::var("DISCORD_TOKEN").wrap_err("DISCORD_TOKEN not found in environment")?;
    let dev_guild_id = if env::var("DEV").is_ok() {
        Some(GuildId(
            env::var("GUILD_ID")
                .wrap_err("GUILD_ID not found in environment, must be set when DEV is set")?
                .parse()
                .wrap_err("GUILD_ID must be an integer")?,
        ))
    } else {
        None
    };

    let set_global_commands = env::var("SET_GLOBAL").is_ok();

    poise::Framework::build()
        .token(token)
        .user_data_setup(move |ctx, ready, framework| {
            Box::pin(async move {
                let data = Handler {
                    dev_guild_id,
                    set_global_commands,
                    mongo,
                };

                let commands = &framework.options().commands;
                let create_commands = poise::builtins::create_application_commands(commands);

                if data.set_global_commands {
                    info!("Installing global slash commands...");
                    let guild_commands =
                        serenity::ApplicationCommand::set_global_application_commands(ctx, |b| {
                            *b = create_commands.clone();
                            b
                        })
                        .await;
                    match guild_commands {
                        Ok(commands) => info!(?commands, "Created global commands"),
                        Err(error) => error!(?error, "Failed to create global commands"),
                    }
                }

                if let Some(guild_id) = data.dev_guild_id {
                    info!("Installing guild commands...");
                    let guild_commands = GuildId::set_application_commands(&guild_id, ctx, |b| {
                        *b = create_commands;
                        b
                    })
                    .await;

                    match guild_commands {
                        Ok(_) => info!("Installed guild slash commands"),
                        Err(error) => error!(?error, "Failed to create global commands"),
                    }
                }

                ctx.set_activity(Activity::playing("für Recht und Ordnung sorgen"))
                    .await;

                info!(name = %ready.user.name, "Bot is connected!");

                Ok(data)
            })
        })
        .options(poise::FrameworkOptions {
            commands: vec![
                handler::lawsuit::lawsuit(),
                handler::prison::prison(),
                hello(),
            ],
            on_error: |err| Box::pin(async { handler::error_handler(err).await }),
            listener: |ctx, event, ctx2, data| {
                Box::pin(async move { handler::listener(ctx, event, ctx2, data).await })
            },
            pre_command: |ctx| {
                Box::pin(async move {
                    let channel_name = ctx
                        .channel_id()
                        .name(&ctx.discord())
                        .await
                        .unwrap_or_else(|| "<unknown>".to_owned());
                    let author = ctx.author().tag();

                    match ctx {
                        Context::Application(ctx) => {
                            let command_name = &ctx.interaction.data().name;

                            info!(?author, ?channel_name, ?command_name, "Command called");
                        }
                        Context::Prefix(_) => {
                            tracing::warn!("Prefix command called!");
                            // we don't use prefix commands
                        }
                    }
                })
            },
            ..Default::default()
        })
        .intents(GatewayIntents::non_privileged() | GatewayIntents::GUILD_MEMBERS)
        .run()
        .await
        .wrap_err("failed to create discord client")?;
    Ok(())
}

/// Sag Karin hallo.
#[poise::command(slash_command)]
async fn hello(ctx: Context<'_>) -> Result<()> {
    ctx.say("hoi!").await?;
    Ok(())
}

fn setup_tracing(pretty: bool) {
    let registry = Registry::default().with(EnvFilter::from_default_env());

    if pretty {
        let tree_layer = tracing_tree::HierarchicalLayer::new(2)
            .with_targets(true)
            .with_bracketed_fields(true);

        registry.with(tree_layer).init();
    } else {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_level(true)
            .with_timer(tracing_subscriber::fmt::time::time())
            .with_ansi(true)
            .with_thread_names(true);

        registry.with(fmt_layer).init();
    };
}
