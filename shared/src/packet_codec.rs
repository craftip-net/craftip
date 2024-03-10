use crate::datatypes::PacketError;
use crate::datatypes::Protocol;
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
        PacketCodec {
            max_length,
            protocol: Protocol::Unknown,
        }
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

#[derive(Clone, Debug)]
pub struct PacketCodec {
    max_length: usize,
    protocol: Protocol,
}

impl Decoder for PacketCodec {
    type Item = SocketPacket;
    type Error = PacketCodecError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<SocketPacket>, PacketCodecError> {
        // otherwise decode gets called very often!
        if buf.is_empty() {
            return Ok(None);
        }
        if buf.len() > self.max_length {
            return Err(PacketCodecError::MaxLineLengthExceeded);
        }
        let result = match self.protocol {
            // first packet
            Protocol::Unknown => {
                let result = SocketPacket::parse_first_package(buf);
                match result.as_ref() {
                    Ok(SocketPacket::ProxyHello(pkg)) => {
                        tracing::debug!("::::::::::::: Changing connection to proxy protocol version {} ::::::::::::::", pkg.version);
                        self.protocol = Protocol::Proxy(pkg.version as u32);
                    }
                    Ok(SocketPacket::MCHello(pkg)) => {
                        tracing::debug!("::::::::::::: Changing connection to MC protocol version {} ::::::::::::::", pkg.version);
                        self.protocol = Protocol::MC(pkg.version as u32);
                    }
                    _ => {
                        self.protocol = Protocol::Unknown;
                    }
                }
                result
            }
            _ => SocketPacket::parse_packet(buf, &self.protocol),
        };
        match result {
            Ok(packet) => Ok(Some(packet)),
            Err(PacketError::TooSmall) => Ok(None),
            Err(e) => Err(e),
        }
        .map_err(PacketCodecError::from)
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

impl Encoder<BytesMut> for PacketCodec {
    type Error = io::Error;

    fn encode(&mut self, data: BytesMut, buf: &mut BytesMut) -> Result<(), io::Error> {
        buf.reserve(data.len());
        buf.put(data);
        Ok(())
    }
}

impl Encoder<SocketPacket> for PacketCodec {
    type Error = io::Error;

    fn encode(&mut self, pkg: SocketPacket, buf: &mut BytesMut) -> Result<(), io::Error> {
        let data = match pkg {
            SocketPacket::MCHello(packet) => packet.data,
            SocketPacket::MCData(packet) => packet.data,
            SocketPacket::Unknown => {
                tracing::error!("UnknownPacket: {:?}", pkg);
                "UnknownPacket".to_string().into_bytes()
            }
            packet => packet
                .encode()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?,
        };
        buf.reserve(data.len());
        buf.put(&data[..]);
        Ok(())
    }
}
