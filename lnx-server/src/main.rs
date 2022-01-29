#[macro_use]
extern crate tracing;

use std::net::SocketAddr;
use std::collections::HashMap;

use lnx_distribute::{NodeId, raft_network};
use lnx_distribute::SocketKind;


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let node_id = std::env::var("NODE_ID")?.parse::<NodeId>()?;
    let bind_addr = std::env::var("BIND_ADDR")?.parse::<SocketAddr>()?;

    let remote_node_id = std::env::var("REMOTE_ID")?.parse::<NodeId>()?;
    let remote = std::env::var("REMOTE_ADDR")?.parse::<SocketAddr>()?;

    let nodes = vec![
        (remote_node_id, remote)
    ];

    let map: HashMap<NodeId, SocketAddr> = HashMap::from_iter(nodes.into_iter());

    let config = SocketKind::Insecure {
        node_id: node_id,
        bind_address: bind_addr,
        peers: map,
    };
    let (_node, handle) = raft_network::PeerNetwork::connect(config).await?;
    handle.wait().await?;
    Ok(())
}
