use std::fmt;

use bincode::{
    Decode, Encode,
    config::{self, Fixint, LittleEndian},
    decode_from_slice, encode_to_vec,
    error::{DecodeError, EncodeError},
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, SeqAccess, Visitor},
    ser::SerializeTuple,
};

/// Returns the bincode configuration used across IPC envelopes.
#[inline]
pub fn bincode_config() -> config::Configuration<LittleEndian, Fixint, config::Limit<{ MAX_BINCODE_BYTES }>> {
    config::legacy().with_limit::<{ MAX_BINCODE_BYTES }>()
}

const MAX_BINCODE_BYTES: usize = 8 * 1024 * 1024; // 8 MiB upper bound for decoded payloads

#[derive(Debug)]
pub struct TaggedEncodeError {
    pub kind: u16,
    pub source: EncodeError,
}

impl TaggedEncodeError {
    #[must_use]
    pub fn new(kind: u16, source: EncodeError) -> Self {
        Self { kind, source }
    }

    #[must_use]
    pub fn into_inner(self) -> EncodeError {
        self.source
    }
}

impl fmt::Display for TaggedEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to encode tagged payload {}: {}", self.kind, self.source)
    }
}

impl std::error::Error for TaggedEncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub trait TaggedEncode {
    fn encode_envelope(&self) -> Result<TaggedEnvelope, TaggedEncodeError>;
}

/// Envelope used to prefix IPC payloads with an explicit variant/tag identifier.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct TaggedEnvelope {
    pub kind: u16,
    pub payload: Vec<u8>,
}

impl TaggedEnvelope {
    #[must_use]
    pub fn new(kind: u16, payload: Vec<u8>) -> Self {
        Self { kind, payload }
    }
}

impl Serialize for TaggedEnvelope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element(&self.kind)?;
        tuple.serialize_element(&self.payload)?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for TaggedEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TaggedEnvelopeVisitor;

        impl<'de> Visitor<'de> for TaggedEnvelopeVisitor {
            type Value = TaggedEnvelope;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("tuple (kind, payload) for IPC envelope")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let kind: u16 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let payload: Vec<u8> = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(TaggedEnvelope { kind, payload })
            }
        }

        deserializer.deserialize_tuple(2, TaggedEnvelopeVisitor)
    }
}

/// Error returned when a tagged payload cannot be decoded.
#[derive(Debug)]
pub enum TaggedDecodeError {
    UnknownKind(u16),
    Decode { kind: u16, source: DecodeError },
}

impl fmt::Display for TaggedDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKind(kind) => write!(f, "unknown tagged payload kind {kind}"),
            Self::Decode { kind, source } => write!(f, "failed to decode tagged payload {kind}: {source}"),
        }
    }
}

impl std::error::Error for TaggedDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnknownKind(_) => None,
            Self::Decode { source, .. } => Some(source),
        }
    }
}

/// Serializes a value into a tagged envelope.
pub fn serialize_payload<T: Encode>(kind: u16, value: &T) -> Result<TaggedEnvelope, EncodeError> {
    let payload = encode_to_vec(value, bincode_config())?;
    Ok(TaggedEnvelope::new(kind, payload))
}

/// Deserializes a payload that was previously encoded into an envelope.
pub fn deserialize_payload<T: Decode<()>>(payload: &[u8]) -> Result<T, DecodeError> {
    let (value, _) = decode_from_slice(payload, bincode_config())?;
    Ok(value)
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ipc_tagged_encode_field {
    ($value:expr) => {
        $value
    };
    ($value:expr, with_serde) => {
        ::bincode::serde::Compat($value)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ipc_tagged_field_type {
    ($ty:ty) => {
        $ty
    };
    ($ty:ty, with_serde) => {
        ::bincode::serde::Compat<$ty>
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ipc_tagged_decode_field {
    ($value:ident) => {
        $value
    };
    ($value:ident, with_serde) => {
        $value.0
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ipc_tagged_tuple_expr {
    ($single:expr) => {
        ($single,)
    };
    ($first:expr, $($rest:expr),+) => {
        ($first, $($rest),+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ipc_tagged_tuple_type {
    ($single:ty) => {
        ($single,)
    };
    ($first:ty, $($rest:ty),+) => {
        ($first, $($rest),+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ipc_tagged_tuple_pattern {
    ($single:ident) => {
        ($single,)
    };
    ($first:ident, $($rest:ident),+) => {
        ($first, $($rest),+)
    };
}

#[macro_export]
macro_rules! tagged_enum {
    (
        impl $enum:path => $kind:path
        $(, unknown = $enum_unknown:ident)?
        {
            $(struct $svariant:ident { $( $sfield:ident : $sty:ty $(=> $smodifier:ident)? ),+ $(,)? }, )*
            $(tuple $tvariant:ident ( $tty:ty ), )*
        }
    ) => {
        impl $crate::envelope::TaggedEncode for $enum {
            fn encode_envelope(&self) -> Result<$crate::envelope::TaggedEnvelope, $crate::envelope::TaggedEncodeError> {
                type __TaggedEnum = $enum;
                type __TaggedKind = $kind;
                match self {
                    $(
                        __TaggedEnum::$svariant { $($sfield),* } => {
                            let payload = $crate::__ipc_tagged_tuple_expr!(
                                $(
                                    $crate::__ipc_tagged_encode_field!($sfield $(, $smodifier)?)
                                ),*
                            );
                            $crate::envelope::serialize_payload(__TaggedKind::$svariant.to_u16(), &payload)
                                .map_err(|source| $crate::envelope::TaggedEncodeError::new(__TaggedKind::$svariant.to_u16(), source))
                        }
                    ),*
                    $(
                        __TaggedEnum::$tvariant(inner) => {
                            $crate::envelope::serialize_payload(__TaggedKind::$tvariant.to_u16(), inner)
                                .map_err(|source| $crate::envelope::TaggedEncodeError::new(__TaggedKind::$tvariant.to_u16(), source))
                        }
                    ),*
                    $( __TaggedEnum::$enum_unknown { kind, payload } => Ok($crate::envelope::TaggedEnvelope::new(*kind, payload.clone())), )?
                }
            }
        }

        impl ::core::convert::From<$enum> for $crate::envelope::TaggedEnvelope {
            fn from(value: $enum) -> Self {
                match $crate::envelope::TaggedEncode::encode_envelope(&value) {
                    Ok(env) => env,
                    Err(err) => {
                        error!(kind = err.kind, error = %err, "failed to serialize tagged payload; sending empty envelope");
                        $crate::envelope::TaggedEnvelope::new(err.kind, Vec::new())
                    }
                }
            }
        }

        impl ::core::convert::From<&$enum> for $crate::envelope::TaggedEnvelope {
            fn from(value: &$enum) -> Self {
                match $crate::envelope::TaggedEncode::encode_envelope(value) {
                    Ok(env) => env,
                    Err(err) => {
                        error!(kind = err.kind, error = %err, "failed to serialize tagged payload; sending empty envelope");
                        $crate::envelope::TaggedEnvelope::new(err.kind, Vec::new())
                    }
                }
            }
        }

        impl ::core::convert::TryFrom<$crate::envelope::TaggedEnvelope> for $enum {
            type Error = $crate::envelope::TaggedDecodeError;

            fn try_from(envelope: $crate::envelope::TaggedEnvelope) -> Result<Self, Self::Error> {
                let kind_value = envelope.kind;
                let payload = envelope.payload;
                type __TaggedEnum = $enum;
                type __TaggedKind = $kind;
                match __TaggedKind::from_u16(kind_value) {
                    Some(kind) => {
                        match kind {
                            $(
                                __TaggedKind::$svariant => {
                                    let tuple: $crate::__ipc_tagged_tuple_type!(
                                        $(
                                            $crate::__ipc_tagged_field_type!($sty $(, $smodifier)?)
                                        ),*
                                    ) = $crate::envelope::deserialize_payload(&payload)
                                            .map_err(|source| $crate::envelope::TaggedDecodeError::Decode { kind: kind_value, source })?;
                                    let $crate::__ipc_tagged_tuple_pattern!($($sfield),*) = tuple;
                                    Ok(__TaggedEnum::$svariant { $($sfield: $crate::__ipc_tagged_decode_field!($sfield $(, $smodifier)?)),* })
                                }
                            ),*
                            $(
                                __TaggedKind::$tvariant => {
                                    let inner: $tty = $crate::envelope::deserialize_payload(&payload)
                                        .map_err(|source| $crate::envelope::TaggedDecodeError::Decode { kind: kind_value, source })?;
                                    Ok(__TaggedEnum::$tvariant(inner))
                                }
                            ),*
                        }
                    }
                    None => {
                        $( return Ok(__TaggedEnum::$enum_unknown { kind: kind_value, payload }); )?
                        Err($crate::envelope::TaggedDecodeError::UnknownKind(kind_value))
                    }
                }
            }
        }
    };
}
