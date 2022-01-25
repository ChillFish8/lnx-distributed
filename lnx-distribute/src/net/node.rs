use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use futures_util::StreamExt;
use hashbrown::HashMap;
use quinn::{Incoming, NewConnection, ReadToEndError, RecvStream, SendStream};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::connection::{create_server, Client};
use crate::net::connection::create_client;
use crate::{Error, Result};

pub type NodeId = u64;

const MAX_SERVER_READ_SIZE: usize = 512 << 20; // ~500MB
const MAX_HANDSHAKE_SIZE: usize = 32 << 10; // ~32KB

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
        peers: HashMap<NodeId, TlsAddress>,
    },
}

/// A remote peer that the node can interact with.
pub struct Peer<Req>
where
    Req: DeserializeOwned + Serialize + Sized + Sync + Send + 'static,
{
    /// The socket address of the peer.
    address: SocketAddr,

    /// The connection to the client.
    ///
    /// This is a write-only stream. A node is not supposed to wait
    /// or care about it's peers.
    client: Client,

    _request: PhantomData<Req>,
}

impl<Req> Peer<Req>
where
    Req: DeserializeOwned + Serialize + Sized + Sync + Send + 'static,
{
    /// Create a new peer from a given SocketAddress and client handle.
    fn new(address: SocketAddr, client: Client) -> Self {
        Self {
            address,
            client,
            _request: PhantomData::default(),
        }
    }

    /// Send a request to the peer.
    pub async fn send<Resp: DeserializeOwned + Sized>(&self, req: &Req) -> Result<Resp> {
        self.client.send(req).await
    }

    /// Re-attempts to connect to the peer if it's stagnated.
    ///
    /// This is a no-op if it's already running.
    pub async fn wake(&self) -> Result<()> {
        self.client.wake().await
    }

    /// Shutdown the node.
    pub async fn shutdown(&self) -> Result<()> {
        self.client.shutdown().await
    }
}

#[instrument(name = "read-request", skip(rx))]
async fn read_request<Req>(max_size: usize, rx: RecvStream) -> anyhow::Result<Req>
where
    Req: DeserializeOwned + Sized + Send + Sync + 'static,
{
    let data = match rx.read_to_end(max_size).await {
        Ok(buff) => buff,
        Err(ReadToEndError::TooLong) => {
            warn!("Peer attempted to send a payload greater than the max read limit!");
            return Err(anyhow!("peer request too long"));
        },
        Err(ReadToEndError::Read(e)) => {
            warn!(
                "Unable to read data from peer due to error {}",
                anyhow::Error::from(e)
            );
            return Err(anyhow!("peer request too long"));
        },
    };

    let req: Req = match bincode::deserialize(&data) {
        Ok(req) => req,
        Err(_) => {
            warn!("Peer sent a invalid request payload");
            return Err(anyhow!("peer request invalid"));
        },
    };

    Ok(req)
}

/// Handles a new Bi-directional stream.
///
/// This is unaware of it's parent connection and should treat each
/// stream as if it was a separate connection.
#[instrument(name = "peer-connection", skip(callback, tx, rx))]
async fn handle_stream<CB, Req, F>(
    remote: SocketAddr,
    callback: Arc<CB>,
    mut tx: SendStream,
    rx: RecvStream,
) where
    CB: Send + Sync + 'static + Fn(Req) -> F,
    Req: DeserializeOwned + Sized + Send + Sync + 'static,
    F: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
{
    let req = match read_request(MAX_SERVER_READ_SIZE, rx).await {
        Ok(r) => r,
        Err(_) => return,
    };

    let data = match (callback.as_ref())(req).await {
        Ok(data) => data,
        Err(e) => {
            warn!("During handling for peer ({}) the node server callback encountered an error {}", remote, e);
            return;
        },
    };

    if let Err(e) = tx.write_all(&data).await {
        warn!(
            "During handling for peer ({}) the node server failed to \
            write all data to peer stream {}",
            remote,
            anyhow::Error::from(e)
        );
    };
}

async fn setup_handshake<OC, Req, F>(
    on_connection: Arc<OC>,
    rx: RecvStream,
    mut tx: SendStream,
) -> anyhow::Result<()>
where
    OC: Send + Sync + 'static + Fn(Req) -> F,
    Req: DeserializeOwned + Sized + Send + Sync + 'static,
    F: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
{
    let req: Req = read_request(MAX_HANDSHAKE_SIZE, rx).await?;
    let data = (on_connection.as_ref())(req).await?;

    tx.write_all(&data).await?;

    Ok(())
}

#[instrument(name = "peer-connections", skip(conn, on_connection, callback))]
async fn handle_connection<CB, OC, Req, F, F2>(
    conn: NewConnection,
    remote: SocketAddr,
    on_connection: Arc<OC>,
    callback: Arc<CB>,
) where
    CB: Send + Sync + 'static + Fn(Req) -> F,
    OC: Send + Sync + 'static + Fn(Req) -> F2,
    Req: DeserializeOwned + Sized + Send + Sync + 'static,
    F: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
    F2: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
{
    let mut streams = conn.bi_streams;

    info!("New peer connected");
    match streams.next().await {
        Some(Ok((tx, rx))) => {
            if let Err(e) = setup_handshake(on_connection, rx, tx).await {
                warn!("Peer failed to upgrade handshake due to error {}", e);
                return;
            }
        },
        _ => {
            info!("Peer errored while completing handshake");
            return;
        },
    }
    info!("Peer handshake complete!");

    while let Some(Ok((tx, rx))) = streams.next().await {
        tokio::spawn(handle_stream(remote, callback.clone(), tx, rx));
    }
}

/// The RPC server that the node exposes in order for peers to communicate with it.
pub struct NodeServer {
    /// The server endpoint connection
    incoming: Incoming,
}

impl From<Incoming> for NodeServer {
    fn from(v: Incoming) -> Self {
        Self { incoming: v }
    }
}

impl NodeServer {
    pub async fn serve<CB, OC, Req, F, F2>(
        &mut self,
        on_connection: OC,
        callback: CB,
    ) -> Result<()>
    where
        CB: Send + Sync + 'static + Fn(Req) -> F,
        OC: Send + Sync + 'static + Fn(Req) -> F2,
        Req: DeserializeOwned + Sized + Send + Sync + 'static,
        F: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
        F2: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
    {
        let on_connection = Arc::new(on_connection);
        let callback = Arc::new(callback);

        while let Some(next) = self.incoming.next().await {
            let conn = next.await?;
            let remote = conn.connection.remote_address();
            tokio::spawn(handle_connection(
                conn,
                remote,
                on_connection.clone(),
                callback.clone(),
            ));
        }

        Ok(())
    }
}

pub struct PeersHandle<Req>
where
    Req: DeserializeOwned + Serialize + Sized + Sync + Send + 'static,
{
    /// A set of peer nodes to communicate with.
    peers: Arc<RwLock<HashMap<NodeId, Peer<Req>>>>,
}

impl<Req> Clone for PeersHandle<Req>
where
    Req: DeserializeOwned + Serialize + Sized + Sync + Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            peers: self.peers.clone(),
        }
    }
}

impl<Req> PeersHandle<Req>
where
    Req: DeserializeOwned + Serialize + Sized + Sync + Send + 'static,
{
    pub(crate) fn new(base: HashMap<NodeId, Peer<Req>>) -> Self {
        Self {
            peers: Arc::new(RwLock::new(base)),
        }
    }

    /// Sends a new request to the given peer and gets the response back.
    pub(crate) async fn send_to_peer<Resp: DeserializeOwned + Sized>(
        &self,
        node_id: NodeId,
        req: &Req,
    ) -> Result<Resp> {
        match self.peers.read().await.get(&node_id) {
            Some(node) => node.send(req).await,
            None => Err(Error::UnknownNode(node_id)),
        }
    }

    /// Sends a wakeup call to all peers.
    ///
    /// Any peers which have failed connections will attempt to reconnect.
    pub(crate) async fn retry_all_peers(&self) -> Result<()> {
        let lock = self.peers.read().await;

        for peer in lock.values() {
            peer.wake().await?;
        }

        Ok(())
    }

    /// Shutdown all peers.
    ///
    /// This sends a shutdown signal to all peers.
    /// Peer actors will finish handling requests before shutting down.
    pub(crate) async fn shutdown_all_peers(&self) -> Result<()> {
        let lock = self.peers.read().await;

        for peer in lock.values() {
            peer.shutdown().await?;
        }

        Ok(())
    }
}

/// A RPC node.
///
/// A node maintains a write-only connection to it's peers
/// with it's peers intern forming a connection with the node's given server
/// address.
pub struct Node<Req>
where
    Req: DeserializeOwned + Serialize + Sized + Send + Sync + 'static,
{
    peers: PeersHandle<Req>,

    /// The node's server endpoint for peers to connect to.
    server: NodeServer,
}

impl<Req> Node<Req>
where
    Req: DeserializeOwned + Serialize + Sized + Send + Sync + 'static,
{
    /// Creates a new node.
    ///
    /// This creates a connection with all peers and starts it's own server to receive events.
    pub async fn create_node(kind: SocketKind) -> Result<Self> {
        let (incoming, peers) = get_server_and_peers(kind).await?;
        let server = NodeServer::from(incoming);

        Ok(Self {
            peers: PeersHandle::new(peers),
            server,
        })
    }

    /// Creates a new peer handle to communicate with peers.
    pub fn handle(&self) -> PeersHandle<Req> {
        self.peers.clone()
    }

    pub async fn serve<CB, VH, F, VHF>(
        &mut self,
        validate_handshake: VH,
        callback: CB,
    ) -> Result<()>
    where
        VH: Send + Sync + 'static + Fn(Req) -> VHF,
        CB: Send + Sync + 'static + Fn(Req) -> F,
        F: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
        VHF: Future<Output = anyhow::Result<Vec<u8>>> + Sync + Send + 'static,
    {
        self.server
            .serve(
                validate_handshake,
                callback,
            )
            .await
    }
}

async fn get_server_and_peers<Req>(
    kind: SocketKind,
) -> Result<(Incoming, HashMap<NodeId, Peer<Req>>)>
where
    Req: DeserializeOwned + Serialize + Sized + Sync + Send + 'static,
{
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
                    Some(peer.cert_file.as_path()),
                )
                .await?;

                peer_map.insert(node_id, Peer::new(peer.address, client));
            }

            Ok((server, peer_map))
        },
        SocketKind::Insecure {
            bind_address,
            peers,
        } => {
            let server = create_server(bind_address, None).await?;

            let mut peer_map = HashMap::with_capacity(peers.len());
            for (node_id, peer) in peers {
                let client = create_client(peer, None, None).await?;

                peer_map.insert(node_id, Peer::new(peer, client));
            }

            Ok((server, peer_map))
        },
    }
}
