use serde::{Deserialize, Serialize};

use super::net;
use crate::Result;

#[derive(Serialize, Deserialize)]
pub enum PeerRequest {}

async fn test(req: Vec<u8>) -> Result<Vec<u8>> {
    todo!()
}

pub struct RaftNetwork {
    peers: net::PeersHandle<PeerRequest>,
}

impl RaftNetwork {
    async fn connect(config: net::SocketKind) -> Result<Self> {
        let mut nodes = net::Node::create_node(config).await?;
        let handle = nodes.handle();

        Ok(Self { peers: handle })
    }
}
