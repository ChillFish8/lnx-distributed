use std::io::Cursor;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::Duration;
use anyhow::anyhow;

use tokio::sync::mpsc::{self, Sender, Receiver};
use tokio::sync::oneshot;
use quinn::{Endpoint, NewConnection, Incoming, ReadToEndError};
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

use crate::{Result, Error};
use super::tls::{
    get_insecure_client_config,
    get_secure_server_config,
    get_insecure_server_config,
    get_secure_client_config,
    read_cert,
    read_key,
};


/// The max response buffer size.
///
/// We dont really expect our peer responses to be any bigger than this.
const MAX_BUFFER_SIZE: usize = 256 << 10;


/// Creates a new server listener.
///
/// If tls is `Some((cert, key))` then the system will try create a
/// secure server from the given cert and key paths.
///
/// If tls is `None` then a insecure server is produced.
pub(crate) async fn create_server(
    bind: SocketAddr,
    tls: Option<(&Path, &Path)>,
) -> Result<Incoming> {
    let cfg = match tls {
        Some((cert, key)) => {
            let cert = read_cert(cert).await?;
            let key = read_key(key).await?;

            get_secure_server_config(cert, key)
        },
        None => get_insecure_server_config()
    }?;

    let (_endpoint, incoming) = Endpoint::server(cfg, bind)?;

    Ok(incoming)
}


struct EventHandle {
    data: Vec<u8>,
    responder: oneshot::Sender<Vec<u8>>,
}

enum EventOp {
    Message(EventHandle),
    Shutdown,
    Retry,
}

pub(crate) struct Client {
    sender: Sender<EventOp>,
}

impl Client {
    pub(crate) async fn send<R: DeserializeOwned + Sized>(&self, v: impl Serialize) -> Result<R> {
        let data = bincode::serialize(&v)?;

        let (responder, rx) = oneshot::channel();
        let handle = EventHandle {
            data,
            responder,
        };

        self.sender.send(EventOp::Message(handle))
            .await
            .map_err(|_| Error::ClientConnectionError("client actor was dropped".to_string()))?;

        let data = rx.await
            .map_err(|_| Error::ClientConnectionError("system failed to receive response from client".to_string()))?;

        let t = bincode::deserialize_from(Cursor::new(data))?;

        Ok(t)
    }

    pub(crate) async fn wake(&self) -> Result<()> {
        self.sender.send(EventOp::Retry)
            .await
            .map_err(|_| Error::ClientConnectionError("client actor was dropped".to_string()))?;

        Ok(())
    }

    pub(crate) async fn shutdown(&self) -> Result<()> {
        self.sender.send(EventOp::Shutdown)
            .await
            .map_err(|_| Error::ClientConnectionError("client actor was dropped".to_string()))?;

        Ok(())
    }
}


/// Creates a new client connection.
///
/// If tls is `Some((cert, key))` then the system will try create a
/// secure client from the given cert and key paths.
///
/// If tls is `None` then a insecure client is produced.
pub(crate) async fn create_client(
    connect: SocketAddr,
    server_name: Option<String>,
    tls: Option<&Path>,
) -> Result<Client> {
    let cfg = match tls {
        Some(cert) => {
            let cert = read_cert(cert).await?;
            get_secure_client_config(cert)?
        },
        None => get_insecure_client_config()
    };

    let client_address = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 0);
    let mut endpoint = Endpoint::client(client_address)?;
    endpoint.set_default_client_config(cfg);

    let (tx, rx) = mpsc::channel(5);

    tokio::spawn(run_client(
        connect,
        server_name,
        endpoint,
        rx,
    ));

    Ok(Client {
        sender: tx,
    })
}


async fn run_client(
    connect_address: SocketAddr,
    server_name: Option<String>,
    endpoint: Endpoint,
    mut events: Receiver<EventOp>,
) {
    let server_name = server_name.unwrap_or_else(|| String::from("localhost"));

    loop {
        if let Err(e) = handle_running_connection(connect_address, &server_name, &endpoint, &mut events).await {
            warn!("node connection lost due to error {}", e);
        } else {
            break
        }

        info!("Waiting on node retry event or abort signal");

        let mut shutdown = true;
        while let Some(op) = events.recv().await {
            match op {
                EventOp::Shutdown => {
                    info!("Shutting down node");
                    shutdown = true;
                    break;
                },
                EventOp::Retry => {
                    info!("Node is attempting to be re-woken");
                    shutdown = false;
                    break;
                },
                EventOp::Message(_) => {}
            }
        }

        if shutdown {
            break;
        }
    }

    info!("Node connection has been aborted")
}



async fn handle_running_connection(
    connect_address: SocketAddr,
    server_name: &str,
    endpoint: &Endpoint,
    events: &mut Receiver<EventOp>,
) -> anyhow::Result<()> {
    let mut connection_tries: u32 = 0;

    while connection_tries <= 4 {
        let conn = match endpoint.connect(connect_address, server_name) {
            Ok(new) => match new.await {
                Ok(conn) => conn,
                Err(e) => {
                    warn!("failed to establish connection due to error {:?} retry_no={}", e, connection_tries);
                    connection_tries += 1;
                    tokio::time::sleep(Duration::from_secs(
                    2i32.pow(connection_tries) as u64
                    )).await;
                    continue
                },
            },
            Err(e) => {
                warn!("failed to establish connection due to error {:?} retry_no={}", e, connection_tries);
                connection_tries += 1;
                tokio::time::sleep(Duration::from_secs(
                    2i32.pow(connection_tries) as u64
                )).await;
                continue
            }
        };

        // Connection success
        connection_tries = 0;

        if let Err(e) = drive_connection(conn, events).await {
            warn!("connection dropped due to error {}, retry_no={}", e, connection_tries);

            connection_tries += 1;
            tokio::time::sleep(Duration::from_secs(
                2i32.pow(connection_tries) as u64
            )).await;
        } else {
            break;
        }
    }

    if connection_tries > 4 {
        Err(anyhow!("aborting connection reties. Failed to establish a connection within the maximum retry threshold."))
    } else {
        Ok(())
    }
}


async fn drive_connection(
    conn: NewConnection,
    events: &mut Receiver<EventOp>,
) -> anyhow::Result<()> {
    let conn = conn.connection;

    while let Some(event_op) = events.recv().await {
        let event = match event_op {
            EventOp::Message(ev) => ev,
            EventOp::Retry => continue,
            EventOp::Shutdown => break,
        };

        let (mut tx, rx) = conn.open_bi().await?;
        tokio::spawn(async move {
            if let Err(e) = tx.write_all(&event.data).await {
                warn!("stream was interrupted while transferring data");
                return Err(e.into())
            };

            match rx.read_to_end(MAX_BUFFER_SIZE).await {
                Ok(buff) => {
                    let _ = event.responder.send(buff);
                },
                Err(ReadToEndError::TooLong) => {
                    warn!("Unusual peer activity: Sending responses large then max buffer size.");
                    return Err(anyhow!("unusual peer activity detected"))
                },
                Err(ReadToEndError::Read(e)) => {
                    warn!("client returned an error during read");
                    return Err(e.into())
                }
            }

            Ok::<_, anyhow::Error>(())
        });
    }


    Ok(())
}