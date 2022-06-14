use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, UserId};

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
    pub court_room: ChannelId,
}
