#[macro_use]
extern crate tracing;

use std::net::SocketAddr;
use std::collections::HashMap;

use lnx_distribute::{NodeId, rpc};
use lnx_distribute::SocketKind;


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let mut nodes = vec![];
    nodes.push((1u64, "127.0.0.1:8001".parse::<SocketAddr>()?));

    let map: HashMap<NodeId, SocketAddr> = HashMap::from_iter(nodes.into_iter());

    let config = SocketKind::Insecure {
        node_id: 0,
        bind_address: "127.0.0.1:8000".parse()?,
        peers: map,
    };
    let node = rpc::RaftNetwork::connect(todo!());

    Ok(())
}
