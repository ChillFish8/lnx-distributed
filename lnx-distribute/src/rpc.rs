use serde::{Deserialize, Serialize};

use super::net;
use crate::Result;

#[derive(Serialize, Deserialize)]
pub enum PeerRequest {}

pub struct RaftNetwork {
    peers: net::PeersHandle<PeerRequest>,
}

impl RaftNetwork {
    async fn connect(config: net::SocketKind) -> Result<Self> {
        let mut nodes = net::Node::create_node(config).await?;
        let waker_handle = nodes.handle();
        let handle = nodes.handle();

        nodes
            .serve(
                move |_r| {
                    let handle = waker_handle.clone();
                    async move {
                        handle.retry_all_peers().await?;

                        Ok(())
                    }
                },
                move |_v| async {
                    Ok::<_, anyhow::Error>(Vec::new())
                })
            .await?;

        Ok(Self { peers: handle })
    }
}
