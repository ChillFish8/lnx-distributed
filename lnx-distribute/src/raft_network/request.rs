use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum PeerRequest {
    HeartBeat,
    HelloWorld,
}
