/// A set of helper utility when working with Rkyv bytes.
///
/// Note that this is built upon alot of unsafe and assumes that the types
/// and data these operations are being applied to are fully aligned.

use std::marker::PhantomData;
use std::ops::Deref;
use std::pin::Pin;

use anyhow::Result;
use rkyv::{Archive, Serialize};
use rkyv::ser::serializers::AllocSerializer;


/// # Safety
///
/// - The byte slice must represent an archived object
/// - The root of the object must be stored at the end of the slice (this is the default behavior)
pub struct ContainedArchive<T: Archive> {
    inner: Box<[u8]>,
    _t: PhantomData<T>,
}

impl<T: Archive> ContainedArchive<T> {
    pub fn as_inner(&self) -> &T::Archived {
        unsafe { T::from_aligned(&self.inner) }
    }

    pub fn as_mut(&mut self) -> Pin<&mut T::Archived> {
        unsafe { T::from_aligned_mut(&mut self.inner) }
    }
}

impl<T: Archive> Deref for ContainedArchive<T> {
    type Target = T::Archived;

    fn deref(&self) -> &Self::Target {
        unsafe { T::from_aligned(&self.inner) }
    }
}

pub trait FromBytes: Archive {
    /// # Safety
    ///
    /// - The byte slice must represent an archived object
    /// - The root of the object must be stored at the end of the slice (this is the default behavior)
    unsafe fn from_aligned(data: &[u8]) -> &Self::Archived {
        rkyv::archived_root::<Self>(data)
    }

    /// # Safety
    ///
    /// - The byte slice must represent an archived object
    /// - The root of the object must be stored at the end of the slice (this is the default behavior)
    unsafe fn from_aligned_mut(data: &mut [u8]) -> Pin<&mut Self::Archived> {
        rkyv::archived_root_mut::<Self>(Pin::new(data))
    }

    /// # Safety
    ///
    /// - The byte slice must represent an archived object
    /// - The root of the object must be stored at the end of the slice (this is the default behavior)
    unsafe fn from_aligned_contained(data: impl Into<Box<[u8]>>) -> ContainedArchive<Self>
    where
        Self: Sized
    {
        ContainedArchive {
            inner: data.into(),
            _t: PhantomData::default()
        }
    }
}


pub trait AsBytes: Serialize<AllocSerializer<1024>> + Sized {
    fn as_boxed_bytes(&self) -> Result<Box<[u8]>> {
        let bytes = rkyv::to_bytes::<_, 1024>(self)?;
        Ok(bytes.into_boxed_slice())
    }
}


impl<T: Archive> FromBytes for T {}
impl<T: Serialize<AllocSerializer<1024>> + Sized> AsBytes for T {}
