use std::collections::HashMap;

use rkyv::{Archive, Serialize, Deserialize};
use openraft::{AppData, AppDataResponse, EffectiveMembership, LogId, SnapshotMeta};
use parking_lot::RwLock;



/// The application data response type.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct ClientResponse(Option<Vec<u8>>);

impl AppDataResponse for ClientResponse {}


/// The application snapshot.
#[derive(Debug)]
pub struct MemStoreSnapshot {
    pub meta: SnapshotMeta,

    /// The data of the state machine at the time of this snapshot.
    pub data: Vec<u8>,
}




// /// An in-memory storage system implementing the `RaftStorage` trait.
// pub struct MemStore {
//     last_purged_log_id: RwLock<Option<LogId>>,
//
//     /// The Raft log.
//     log: RwLock<BTreeMap<u64, Entry<ClientRequest>>>,
//
//     /// The Raft state machine.
//     sm: RwLock<MemStoreStateMachine>,
//
//     /// The current hard state.
//     vote: RwLock<Option<Vote>>,
//
//     snapshot_idx: Arc<Mutex<u64>>,
//
//     /// The current snapshot.
//     current_snapshot: RwLock<Option<MemStoreSnapshot>>,
// }
