
use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use tokio::task::JoinHandle;

use crate::net::PeersHandle;
use crate::{net, NodeId};
use crate::Result;
use super::request::PeerRequest;

pub const HEARTBEAT_INTERVAL: u64 = 30;


#[derive(Debug, Serialize, Deserialize)]
pub struct EmptyResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {

}

async fn handle_event(
    peers: PeersHandle<PeerRequest>,
    request: PeerRequest,
) -> anyhow::Result<Vec<u8>> {
    let data = match request {
        PeerRequest::HelloWorld => {
            peers.retry_all_peers().await?;
            bincode::serialize(&EmptyResponse)?
        },
        PeerRequest::HeartBeat => bincode::serialize(&EmptyResponse)?,
    };

    Ok(data)
}


pub struct ServerHandle(JoinHandle<()>, JoinHandle<()>);
impl ServerHandle {
    pub fn abort(self) {
        self.0.abort();
        self.1.abort();
    }

    pub async fn wait(self) -> anyhow::Result<()> {
        self.1.await?;
        self.0.abort();

        Ok(())
    }
}


#[derive(Clone)]
pub struct PeerNetwork {
    peers: net::PeersHandle<PeerRequest>,
}

impl PeerNetwork {
    /// Starts the node server and attempts to connect to all RPC peers.
    ///
    /// Note this this will accept some nodes being unavailable and will attempt a
    /// reconnect when a peer connects to the node.
    pub async fn connect(
        config: net::SocketKind,
    ) -> Result<(Self, ServerHandle)> {
        let nodes = net::Node::create_node(config).await?;
        let handle = nodes.handle();
        let handle_waker = nodes.handle();
        let waker = nodes.handle();

        let heartbeat_handle = tokio::spawn(async move {
            let heartbeat_interval = Duration::from_secs(HEARTBEAT_INTERVAL);
            let mut interval = tokio::time::interval(heartbeat_interval);

            loop {
                interval.tick().await;

                let res = waker
                    .send_to_all_connected_peers::<EmptyResponse>(&PeerRequest::HeartBeat)
                    .await;

                if let Err(e) = res {
                    warn!("failed to emit heartbeats due to error {}", e.to_string());
                }
            }
        });

        let server_handle = tokio::spawn(async move {
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
        });

        let server = ServerHandle(heartbeat_handle, server_handle);

        info!("sending!");
        handle.send_to_all_peers::<EmptyResponse>(&PeerRequest::HelloWorld).await?;

        Ok((Self { peers: handle }, server))
    }

    /// Send a request to a peer.
    pub async fn send_request<Resp>(&self, node_id: NodeId, req: &PeerRequest) -> Result<()>
    where
        Resp: DeserializeOwned + Serialize +Sized + Send + Sync + 'static
    {
        self.peers.send_to_peer(node_id, req).await
    }
}