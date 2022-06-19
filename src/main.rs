extern crate core;

mod handler;
mod lawsuit;
mod model;

use std::env;

use color_eyre::{eyre::WrapErr, Report, Result};
use poise::serenity_prelude as serenity;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{handler::Handler, model::Mongo};

type Context<'a> = poise::Context<'a, Handler, Report>;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let _ = dotenv::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

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
        Some(serenity::GuildId(
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
        .user_data_setup(move |_, _, _| {
            Box::pin(async move {
                Ok(Handler {
                    dev_guild_id,
                    set_global_commands,
                    mongo,
                })
            })
        })
        .options(poise::FrameworkOptions {
            commands: vec![handler::lawsuit(), handler::prison()],
            on_error: |err| Box::pin(async { handler::error_handler(err).await }),
            listener: |ctx, event, ctx2, data| {
                Box::pin(async move { handler::listener(ctx, event, ctx2, data).await })
            },
            ..Default::default()
        })
        .intents(serenity::GatewayIntents::GUILD_MEMBERS)
        .run()
        .await
        .wrap_err("failed to create discord client")?;
    Ok(())
}
