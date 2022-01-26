use serde::{Deserialize, Serialize};

use super::net;
use crate::Result;

#[derive(Serialize, Deserialize)]
pub enum PeerRequest {}

#[derive(Serialize, Deserialize)]
pub struct HandShakeResponse {

}

pub struct RaftNetwork {
    peers: net::PeersHandle<PeerRequest>,
}

impl RaftNetwork {
    pub async fn connect(config: net::SocketKind) -> Result<Self> {
        let mut nodes = net::Node::create_node(config).await?;
        let waker_handle = nodes.handle();
        let handle = nodes.handle();

        nodes
            .serve(
                move |_r| {
                    info!("got connection!");
                    let handle = waker_handle.clone();
                    async move {
                        handle.retry_all_peers().await?;

                        let data = bincode::serialize(&HandShakeResponse{})?;
                        Ok::<_, anyhow::Error>(data)
                    }
                },
                move |_v| async { Ok::<_, anyhow::Error>(Vec::new()) },
            )
            .await?;

        Ok(Self { peers: handle })
    }
}
