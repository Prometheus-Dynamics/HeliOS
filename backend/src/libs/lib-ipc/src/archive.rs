use core::mem::align_of;

use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Place, Serialize as RkyvSerialize,
    api::high::{HighSerializer, HighValidator},
    bytecheck::CheckBytes,
    de::pooling::Pool,
    rancor::{Error as ArchiveError, Source, Strategy},
    ser::{Allocator, Writer, allocator::ArenaHandle},
    util::AlignedVec,
    vec::{ArchivedVec, VecResolver},
    with::{ArchiveWith, DeserializeWith, SerializeWith},
};
use serde::{Serialize, de::DeserializeOwned};

pub type Result<T> = core::result::Result<T, ArchiveError>;

pub fn encode_to_vec<T>(value: &T) -> Result<Vec<u8>>
where
    T: for<'a> RkyvSerialize<HighSerializer<AlignedVec, ArenaHandle<'a>, ArchiveError>>,
{
    let bytes = rkyv::to_bytes::<ArchiveError>(value)?;
    Ok(bytes.as_ref().to_vec())
}

pub fn decode_from_slice<T>(payload: &[u8]) -> Result<T>
where
    T: Archive,
    T::Archived: for<'a> CheckBytes<HighValidator<'a, ArchiveError>> + RkyvDeserialize<T, Strategy<Pool, ArchiveError>>,
{
    if payload.is_empty() || payload.as_ptr().align_offset(align_of::<T::Archived>()) == 0 {
        return rkyv::from_bytes::<T, ArchiveError>(payload);
    }

    // Transport buffers may expose the payload at an arbitrary offset inside a larger slice.
    // Re-pack misaligned slices into an aligned backing buffer before validating/deserializing.
    let mut aligned = AlignedVec::<16>::with_capacity(payload.len());
    aligned.extend_from_slice(payload);
    rkyv::from_bytes::<T, ArchiveError>(&aligned)
}

pub fn encode_serde<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(ArchiveError::new)
}

pub fn decode_serde<T: DeserializeOwned>(payload: &[u8]) -> Result<T> {
    serde_json::from_slice(payload).map_err(ArchiveError::new)
}

pub mod with {
    use super::*;

    pub struct SerdeBytes;

    pub struct SerdeBytesResolver {
        len: usize,
        inner: VecResolver,
    }

    impl<T> ArchiveWith<T> for SerdeBytes
    where
        T: Serialize + DeserializeOwned,
    {
        type Archived = ArchivedVec<u8>;
        type Resolver = SerdeBytesResolver;

        fn resolve_with(_: &T, resolver: Self::Resolver, out: Place<Self::Archived>) {
            ArchivedVec::resolve_from_len(resolver.len, resolver.inner, out);
        }
    }

    impl<T, S> SerializeWith<T, S> for SerdeBytes
    where
        T: Serialize + DeserializeOwned,
        S: rkyv::rancor::Fallible + Allocator + Writer + ?Sized,
        S::Error: Source,
    {
        fn serialize_with(field: &T, serializer: &mut S) -> core::result::Result<Self::Resolver, S::Error> {
            let encoded = serde_json::to_vec(field).map_err(S::Error::new)?;
            let len = encoded.len();
            let inner = ArchivedVec::serialize_from_slice(encoded.as_slice(), serializer)?;
            Ok(SerdeBytesResolver { len, inner })
        }
    }

    impl<T, D> DeserializeWith<ArchivedVec<u8>, T, D> for SerdeBytes
    where
        T: DeserializeOwned,
        D: rkyv::rancor::Fallible + ?Sized,
        D::Error: Source,
    {
        fn deserialize_with(field: &ArchivedVec<u8>, _: &mut D) -> core::result::Result<T, D::Error> {
            serde_json::from_slice(field.as_slice()).map_err(D::Error::new)
        }
    }
}
