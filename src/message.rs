use std::net::SocketAddr;

use block::{Block};

#[derive(Serialize, Deserialize)]
pub enum ClientMessage {
    NewBlock(Block),
    QueryChain,
    Chain(Vec<Block>),
}

#[derive(Serialize, Deserialize)]
pub enum ClientToNameserverMessage {
    Inform(u16),
    Query,
    Pong,
}

#[derive(Serialize, Deserialize)]
pub enum NameserverToClientMessage {
    Peers(Vec<SocketAddr>),
    Ping,
}
