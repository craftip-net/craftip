use std::mem::size_of;
use std::net::SocketAddr;
use std::ops::{BitAnd, BitOr, Not};

use crate::config::MAXIMUM_PACKET_SIZE;
use crate::crypto::{ChallengeDataType, SignatureDataType};
use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use tokio::sync::mpsc::UnboundedSender;

use crate::datatypes::PacketError;
use crate::minecraft::MinecraftDataPacket;
use crate::proxy::{ProxyConnectedResponse, ProxyDataPacket, ProxyHelloPacket};

pub type PingPacket = u16;
pub type ClientID = u16;

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum SocketPacket {
    ProxyHello(ProxyHelloPacket),
    #[serde(with = "BigArray")]
    ProxyAuthRequest(ChallengeDataType),
    #[serde(with = "BigArray")]
    ProxyAuthResponse(SignatureDataType),
    ProxyHelloResponse(ProxyConnectedResponse),
    ProxyJoin(ClientID),
    ProxyDisconnect(ClientID),
    ProxyDisconnectAck(ClientID),
    ProxyError(String),
    // todo change packet type
    ProxyData(ProxyDataPacket),
    ProxyPing(PingPacket),
    ProxyPong(PingPacket),
    Unknown,
}

pub enum DisconnectReason {
    Disconnected,
    SocketClosed,
}

impl From<ProxyHelloPacket> for SocketPacket {
    fn from(packet: ProxyHelloPacket) -> Self {
        SocketPacket::ProxyHello(packet)
    }
}
impl From<ProxyConnectedResponse> for SocketPacket {
    fn from(packet: ProxyConnectedResponse) -> Self {
        SocketPacket::ProxyHelloResponse(packet)
    }
}

impl From<ProxyDataPacket> for SocketPacket {
    fn from(packet: ProxyDataPacket) -> Self {
        SocketPacket::ProxyData(packet)
    }
}

impl SocketPacket {
    pub fn encode_into(&self, buf: &mut BytesMut) -> Result<(), PacketError> {
        if let SocketPacket::ProxyData(data) = self {
            // data + client id length
            let length = data.data.len() + size_of::<u16>();
            buf.reserve(length + size_of::<u16>());
            let start = (length as u16).bitor(1u16 << 15);
            buf.put_u16(start);
            buf.put_u16(data.client_id);
            buf.put(data.data.as_ref());
            return Ok(());
        }

        let packet = bincode::serialize(&self).map_err(|_| PacketError::EncodingError)?;
        let packet_length = packet.len() as u16;
        buf.put_u16(packet_length);
        buf.put(&packet[..]);
        Ok(())
    }
}

impl SocketPacket {
    pub fn decode_from(buf: &mut BytesMut) -> Result<Option<SocketPacket>, PacketError> {
        // check if buffer is long enough to get length
        if buf.len() < size_of::<u16>() {
            return Ok(None);
        }

        // get first 2 bytes
        let start = u16::from_be_bytes([buf[0], buf[1]]);
        // the first bit defines, if the packet is a data packet
        let length = start.bitand((1u16 << 15).not()) as usize;

        let is_data_packet = start.bitand(1 << 15) != 0;
        // check if buffer is at least as long as length + length field
        if buf.len() < length + size_of::<u16>() {
            return Ok(None);
        }
        if length > MAXIMUM_PACKET_SIZE {
            return Err(PacketError::TooLong);
        }

        buf.advance(size_of::<u16>());

        if is_data_packet {
            let client_id = buf.get_u16();
            let data_len = length - size_of::<u16>();
            let data = buf.split_to(data_len);
            return Ok(Some(SocketPacket::ProxyData(ProxyDataPacket {
                client_id,
                data: MinecraftDataPacket(data.freeze()),
            })));
        }
        let packet = buf.split_to(length);
        let result = bincode::deserialize::<SocketPacket>(&packet).unwrap();
        // decode bincode packet
        Ok(Some(result))
    }
}

/// Custom packet type for tokio channels to be able to close the client socket by the proxy
/// uses Packet type as a generic type
/// or Close to close the socket
#[derive(Debug)]
pub enum ClientToProxy {
    Packet(SocketAddr, MinecraftDataPacket),
    AddMinecraftClient(SocketAddr, UnboundedSender<MinecraftDataPacket>),
    RemoveMinecraftClient(SocketAddr),
}

#[cfg(test)]
mod tests {
    use crate::proxy::ProxyDataPacket;
    use crate::socket_packet::SocketPacket;
    use bytes::BytesMut;

    fn create_packets() -> Vec<SocketPacket> {
        let ping = SocketPacket::ProxyPing(1);
        let err = SocketPacket::ProxyError("hi".into());
        let data = SocketPacket::ProxyData(ProxyDataPacket {
            client_id: 0x1234,
            data: vec![9, 10, 11, 12, 13],
        });
        return vec![ping, data, err];
    }
    #[test]
    fn encode_decode() {
        let packets = create_packets();

        let mut buf = BytesMut::new();
        let mut lengths = Vec::new();

        // Encode each packet and store its length
        for packet in &packets {
            println!("Encoding {:?}", packet);
            let start_len = buf.len();
            packet.encode_into(&mut buf).unwrap();
            lengths.push(buf.len() - start_len);
        }
        println!(">>> packet indexes {:?}", lengths);
        println!(">>> {:?}", buf);

        // Initialize the current index for lengths

        for (i, packet) in packets.iter().enumerate() {
            println!("Try to decode {} bytes in packet {:?} ", lengths[i], packet);
            let before_parse = buf.len();
            let decoded_packet = SocketPacket::decode_from(&mut buf).unwrap().unwrap();
            assert_eq!(packet, &decoded_packet);
            let parsed_bytes = before_parse - buf.len();
            assert_eq!(
                lengths[i], parsed_bytes,
                "Parsed different amount than encoded"
            );
        }

        // Ensure both buffers are empty
        assert!(buf.is_empty());
    }
    #[test]
    fn partial_decoding() {
        let packets = create_packets();

        let mut buf = BytesMut::new();
        let mut packet_indices = Vec::new();

        // Encode each packet and store its length
        for packet in &packets {
            println!("Encoding {:?}", packet);
            packet.encode_into(&mut buf).unwrap();
            packet_indices.push(buf.len());
        }
        let packet_indices = packet_indices;
        let mut parsed_packets = vec![];
        for i in 0..=buf.len() {
            println!("Testing {}/{} Bytes parsed", i, buf.len());
            let mut buf = BytesMut::from(&buf[..i]);
            parsed_packets = vec![];
            'inner: loop {
                let packet = SocketPacket::decode_from(&mut buf);
                match packet {
                    Ok(Some(packet)) => {
                        parsed_packets.push(packet.clone());
                    }
                    Ok(None) => {
                        let amout_that_should_be_parsed =
                            packet_indices.iter().filter(|e| e <= &&i).count();
                        assert_eq!(packets[..amout_that_should_be_parsed], parsed_packets[..]);
                        break 'inner;
                    }
                    Err(e) => panic!("{}", e),
                }
            }
        }
        assert_eq!(packets, parsed_packets, "Did not decode all packets");
    }
}
