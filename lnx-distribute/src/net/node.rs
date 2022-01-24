use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;

use hashbrown::HashMap;
use quinn::Incoming;
use serde::{Deserialize, Serialize};

use super::connection::{Client, create_server};
use crate::{RaftError, Result};
use crate::net::connection::create_client;

pub type NodeId = u64;

#[derive(Debug, Deserialize)]
pub struct TlsAddress {
    /// The server socket address of the peer.
    address: SocketAddr,

    /// The DNS name attached to the certificate.
    ///
    /// This is a limitation of RusTLS right now so we're
    /// currently stuck with it as much as it's incredibly annoying.
    dns_name: String,

    /// The TLS certificate for the given peer.
    cert_file: PathBuf,
}


#[derive(Debug, Deserialize)]
#[serde(untagged)]
/// The node's address and peers socket configuration.
///
/// The system must be either entirely insecure or entirely
/// secure backed by TLS.
pub enum SocketKind {
    /// A insecure socket configuration and peers.
    Insecure {
        bind_address: SocketAddr,
        peers: HashMap<NodeId, SocketAddr>,
    },

    /// A secure socket configuration and peers.
    Secure {
        bind_address: SocketAddr,
        cert_file: PathBuf,
        key_file: PathBuf,
        peers: HashMap<NodeId, TlsAddress>
    }
}


/// A remote peer that the node can interact with.
pub struct Peer {
    /// The socket address of the peer.
    address: SocketAddr,

    /// The connection to the client.
    ///
    /// This is a write-only stream. A node is not supposed to wait
    /// or care about it's peers.
    client: Client,
}


pub struct NodeServer<R, F, E>
where
    R: Deserialize<'static> + Sized,
    E: std::error::Error + Send + Sync,
    F: Future<Output = core::result::Result<(), E>>
{
    /// The server endpoint connection
    incoming: Incoming,

    /// The callback
    callback: fn(R) -> F,
}


/// A RPC node.
///
/// A node maintains a write-only connection to it's peers
/// with it's peers intern forming a connection with the node's given server
/// address.
pub struct Node<R, F, E>
where
    R: Deserialize<'static> + Serialize + Sized,
    E: std::error::Error + Send + Sync,
    F: Future<Output = core::result::Result<(), E>> + Sync + Send + 'static
{
    /// A set of peer nodes to communicate with.
    connected_peers: HashMap<NodeId, Peer>,

    ///

    /// The node's server endpoint for peers to connect to.
    server: NodeServer<R, F, E>,
}

impl<R, F, E> Node<R, F, E>
where
    R: Deserialize<'static> + Serialize + Sized,
    E: std::error::Error + Send + Sync,
    F: Future<Output = core::result::Result<(), E>> + Sync + Send + 'static
{
    /// Creates a new node.
    ///
    /// This creates a connection with all peers and starts it's own server to receive events.
    pub async fn create_node(kind: SocketKind, callback: fn(R) -> F) -> Result<Self> {
        let (incoming, connected_peers) = get_server_and_peers(kind).await?;
        let server = NodeServer {
            incoming,
            callback,
        };

        Ok(Self {
            connected_peers,
            server,
        })
    }



    /// Send a request to a given peer node.
    ///
    /// If the node doesn't exist an `Err(WiskeyError::UnknownNode(node))`
    /// is returned.
    pub async fn send_to_node(&self, node: NodeId, req: &R) -> Result<()> {
        match self.connected_peers.get(&node) {
            Some(peer) => peer.client.send(req).await,
            None => Err(RaftError::UnknownNode(node)),
        }
    }
}


async fn get_server_and_peers(kind: SocketKind) -> Result<(Incoming, HashMap<NodeId, Peer>)> {
    match kind {
        SocketKind::Secure {
            bind_address,
            cert_file,
            key_file,
            peers,
        } => {
            let tls = Some((cert_file.as_path(), key_file.as_path()));
            let server = create_server(bind_address, tls).await?;

            let mut peer_map = HashMap::with_capacity(peers.len());
            for (node_id, peer) in peers {
                let client = create_client(
                    peer.address,
                    Some(peer.dns_name),
                    Some(peer.cert_file.as_path())
                ).await?;

                peer_map.insert(
                    node_id,
                    Peer {
                        address: peer.address,
                        client
                    }
                );
            }

            Ok((server, peer_map))
        },
        SocketKind::Insecure {
            bind_address,
            peers
        } => {
            let server = create_server(bind_address, None).await?;

            let mut peer_map = HashMap::with_capacity(peers.len());
            for (node_id, peer) in peers {
                let client = create_client(peer,None, None).await?;

                peer_map.insert(
                    node_id,
                    Peer {
                        address: peer,
                        client
                    }
                );
            }

            Ok((server, peer_map))
        },
    }
}



