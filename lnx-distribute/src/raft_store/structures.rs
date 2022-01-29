use std::collections::HashMap;
use openraft::{AppData, EffectiveMembership};
use rkyv::{Archive, Deserialize, Serialize};
use sled::IVec;


#[derive(Archive, Serialize, Deserialize, Copy, Clone, Debug)]
pub struct LogId(u64);


/// The application data request type.
#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
pub struct ClientRequest {
    /// The ID of the client which has sent the request.
    pub client: String,

    /// The serial number of this request.
    pub serial: u64,

    /// The payload of the request
    pub payload: Vec<u8>,
}

impl AppData for ClientRequest {}


/// The state machine of the raft node.
#[derive(Archive, Serialize, Deserialize, Debug, Default, Clone)]
pub struct StoreStateMachine {
    pub last_applied_log: Option<LogId>,

    pub last_membership: Option<EffectiveMembership>,

    /// A mapping of client IDs to their state info.
    pub client_serial_responses: HashMap<String, (u64, Option<String>)>,

    /// The current status of a client by ID.
    pub client_status: HashMap<String, String>,
}
