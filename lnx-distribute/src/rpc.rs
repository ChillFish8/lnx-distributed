use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use crate::net::PeersHandle;

use super::net;
use crate::Result;

pub const HEARTBEAT_INTERVAL: u64 = 30;

#[derive(Debug, Serialize, Deserialize)]
pub enum PeerRequest {
    HeartBeat,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeartBeatResponse {

}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {

}


async fn handle_event(
    _peers: PeersHandle<PeerRequest>,
    request: PeerRequest,
) -> anyhow::Result<Vec<u8>> {
    let data = match request {
        PeerRequest::HeartBeat => {
            info!("Beat!");
            bincode::serialize(&HeartBeatResponse{})?
        }
    };

    Ok(data)
}



pub struct RaftNetwork {
    peers: net::PeersHandle<PeerRequest>,
}

impl RaftNetwork {
    pub async fn connect(
        config: net::SocketKind,
    ) -> Result<Self> {
        let nodes = net::Node::create_node(config).await?;
        let handle = nodes.handle();
        let handle_waker = nodes.handle();
        let waker = nodes.handle();

        tokio::spawn(async move {
            let heartbeat_interval = Duration::from_secs(HEARTBEAT_INTERVAL);
            let mut interval = tokio::time::interval(heartbeat_interval);

            loop {
                interval.tick().await;

                let res = waker.send_to_all_connected_peers::<HeartBeatResponse>(&PeerRequest::HeartBeat)
                    .await;

                if let Err(e) = res {
                    warn!("failed to emit heartbeats due to error {}", e.to_string());
                }
            }
        });

        tokio::spawn(async move {
            let handle = handle_waker.clone();
            let res = nodes.serve(
                move |r| {
                    trace!("Handling request {:?}", &r);
                    let new_handle = handle.clone();
                    handle_event(new_handle, r)
                },
            ).await;

            if let Err(e) = res {
                error!("node server shutdown to error {}", e.to_string());
            }
        }).await;

        Ok(Self { peers: handle })
    }
}
