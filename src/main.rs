mod handler;

use std::env;

use color_eyre::{eyre::WrapErr, Result};
use serenity::{model::prelude::*, prelude::*};

use crate::handler::Handler;

#[tokio::main]
fn main() -> Result<()> {
    color_eyre::install()?;

    let _ = dotenv::dotenv();

    let token = env::var("DISCORD_TOKEN").wrap_err("DISCORD_TOKEN not found in environment")?;
    let guild_id = if let Ok(_) = env::var("DEV") {
           SOme( GuildId(
                env::var("GUILD_ID")
                    .wrap_err("GUILD_ID not found in environment, must be set when DEV is set")?
                    .parse()
                    .wrap_err("GUILD_ID must be an integer")?,
            ))
        })
} else {None};

    let mut client = Client::builder(token, GatewayIntents::empty())
        .event_handler(Handler { dev_guild_id })
        .await
        .wrap_err("failed to create discord client")?;

    client.start().await.wrap_err("running client")
}
