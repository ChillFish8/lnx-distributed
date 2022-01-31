use bincode::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;


pub trait FromBytes: DeserializeOwned {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data)
    }
}


pub trait AsBytes: Serialize + Sized {
    fn as_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
    }
}


impl<T: DeserializeOwned> FromBytes for T {}
impl<T: Serialize + Sized> AsBytes for T {}
