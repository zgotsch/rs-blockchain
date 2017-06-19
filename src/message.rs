use std::net::SocketAddr;
use std::borrow::Cow;

use block::{Block};

type BlockBorrow<'a> = &'a Block;
type BlockChainBorrow<'a> = &'a Vec<Block>;

#[derive(Serialize, Deserialize)]
pub enum ClientMessage {
    NewBlock(Block),
    QueryChain,
    Chain(Vec<Block>),
}

pub enum NameServerMessage {
    Inform(SocketAddr),
    Query,
}

impl NameServerMessage {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            &NameServerMessage::Inform(addr) => format!("i {}", addr),
            &NameServerMessage::Query => "q".to_string(),
        }.as_bytes().to_vec()
    }
}
