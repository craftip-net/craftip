use crate::datatypes::PacketError;
use crate::socket_packet::SocketPacket;
use bytes::{BufMut, Bytes, BytesMut};
use std::io;
use thiserror::Error;
use tokio_util::codec::{Decoder, Encoder};

/// An error occurred while encoding or decoding a frame
#[derive(Debug, Error)]
pub enum PacketCodecError {
    /// The maximum line length was exceeded.
    #[error("max line length exceeded")]
    MaxLineLengthExceeded,
    #[error("PacketCodecError")]
    PacketCodec(PacketError),
    /// An IO error occurred.
    #[error("Io Error")]
    Io(io::Error),
}

impl PacketCodec {
    /// Returns a `PacketCodec` for splitting up data into packets.
    pub fn new(max_length: usize) -> PacketCodec {
        PacketCodec { max_length }
    }
}

impl From<io::Error> for PacketCodecError {
    fn from(e: io::Error) -> PacketCodecError {
        PacketCodecError::Io(e)
    }
}

impl From<PacketError> for PacketCodecError {
    fn from(e: PacketError) -> PacketCodecError {
        PacketCodecError::PacketCodec(e)
    }
}

#[derive(Debug)]
pub struct PacketCodec {
    max_length: usize,
}

impl Decoder for PacketCodec {
    type Item = SocketPacket;
    type Error = PacketCodecError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<SocketPacket>, PacketCodecError> {
        // otherwise decode gets called very often!
        if buf.is_empty() {
            return Ok(None);
        }
        /*if buf.len() > self.max_length {
            return Err(PacketCodecError::MaxLineLengthExceeded);
        }*/

        SocketPacket::decode_from(buf).map_err(PacketCodecError::from)
    }
}

impl Encoder<Bytes> for PacketCodec {
    type Error = io::Error;

    fn encode(&mut self, data: Bytes, buf: &mut BytesMut) -> Result<(), io::Error> {
        buf.reserve(data.len());
        buf.put(data);
        Ok(())
    }
}

impl Encoder<SocketPacket> for PacketCodec {
    type Error = io::Error;

    fn encode(&mut self, pkg: SocketPacket, buf: &mut BytesMut) -> Result<(), io::Error> {
        pkg.encode_into(buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }
}
