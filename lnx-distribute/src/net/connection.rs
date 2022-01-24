use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, Sender, Receiver};
use quinn::{Endpoint, NewConnection, Incoming, Connection};
use serde::Serialize;
use tokio::task::JoinHandle;

use crate::{Result, RaftError};
use super::tls::{
    get_insecure_client_config,
    get_secure_server_config,
    get_insecure_server_config,
    get_secure_client_config,
    read_cert,
    read_key,
};


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


pub(crate) struct Client {
    sender: Sender<Vec<u8>>,
}

impl Client {
    pub(crate) async fn send(&self, v: impl Serialize) -> Result<()> {
        let data = bincode::serialize(&v)?;

        self.sender.send(data)
            .await
            .map_err(|_| RaftError::ClientConnectionError("client actor was dropped".to_string()))?;

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
        sender: tx
    })
}


async fn run_client(
    connect_address: SocketAddr,
    server_name: Option<String>,
    endpoint: Endpoint,
    mut events: Receiver<Vec<u8>>,
) {
    let mut connection_tries: u32 = 0;
    let server_name = server_name.unwrap_or_else(|| String::from("localhost"));

    while connection_tries <= 4 {
        let conn = match endpoint.connect(connect_address, &server_name) {
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

        if let Err(e) = drive_connection(conn, &mut events).await {
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
        error!("aborting connection reties. Failed to establish a connection within the maximum retry threshold.");
    }
}


async fn drive_connection(
    conn: NewConnection,
    events: &mut Receiver<Vec<u8>>,
) -> anyhow::Result<()> {
    let conn = conn.connection;

    let mut handles = vec![];
    while let Some(data) = events.recv().await {
        let mut stream = conn.open_uni().await?;
        let handle = tokio::spawn(async move {
            if let Err(e) = stream.write_all(&data).await {
                warn!("stream was interrupted while transferring data");
                return Err(e.into())
            };
            Ok::<_, anyhow::Error>(())
        });

        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}