use anyhow::Result;
use openraft::raft::Entry;

use lnx_utils::bytes::{FromBytes, AsBytes};

use super::structures::ClientRequest;

static KEYSPACE: &str = "raft_log_store";


pub struct LogStore {
    db: sled::Tree,
}

impl LogStore {
    pub fn new(db: &sled::Db) -> Result<Self> {
        Ok(Self {
            db: db.open_tree(KEYSPACE)?
        })
    }

    pub fn append_logs(&self, logs: &[&Entry<ClientRequest>]) -> Result<()> {
        for log in logs {
            self.db.insert(
                log.log_id.as_bytes()?,
                log.payload.as_bytes()?,
            )?;
        }

        Ok(())
    }

}
