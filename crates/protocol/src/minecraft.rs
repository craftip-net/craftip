use std::mem;
use std::mem::size_of;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio_util::bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::cursor::{CustomCursor, CustomCursorRead, CustomCursorWrite};
use crate::datatypes::PacketError;

const OLD_MINECRAFT_START: [u8; 27] = [
    0xFE, 0x01, 0xFA, 0x00, 0x0B, 0x00, 0x4D, 0x00, 0x43, 0x00, 0x7C, 0x00, 0x50, 0x00, 0x69, 0x00,
    0x6E, 0x00, 0x67, 0x00, 0x48, 0x00, 0x6F, 0x00, 0x73, 0x00, 0x74,
];

const FORGE_STRING: &str = "\0FORGE";

#[macro_export]
macro_rules! propagate {
    ($expr:expr) => {
        match $expr {
            Err(e) => return Err(e),
            Ok(None) => return Ok(None),
            Ok(Some(value)) => value,
        }
    };
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub enum MinecraftHelloPacketType {
    Legacy,
    Ping,
    Connect,
    Unknown,
}

const ERROR_MSG_FIRST_LINE: &str = "Server not online!";
const ERROR_MSG_LONG: &str = "Ask your friend to start CraftIP!";

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct MinecraftHelloPacket {
    pub length: usize,
    pub pkg_type: MinecraftHelloPacketType,
    pub id: i32,
    pub version: i32,
    pub hostname: String,
    pub forge: bool,
    pub port: u32,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct MinecraftDataPacket(pub Bytes);

impl MinecraftDataPacket {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl From<Bytes> for MinecraftDataPacket {
    fn from(value: Bytes) -> Self {
        Self(value)
    }
}
impl AsRef<[u8]> for MinecraftDataPacket {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl<'de> Deserialize<'de> for MinecraftDataPacket {
    fn deserialize<D>(deserializer: D) -> Result<MinecraftDataPacket, D::Error>
    where
        D: Deserializer<'de>,
    {
        tracing::error!("should not use deserialize of MinecraftDataPacket");
        let vec = Vec::<u8>::deserialize(deserializer)?;
        Ok(MinecraftDataPacket(Bytes::from(vec)))
    }
}
// todo
impl Serialize for MinecraftDataPacket {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        tracing::error!("should not use serialize of MinecraftDataPacket");
        serializer.serialize_bytes(&self.0)
    }
}

impl MinecraftHelloPacket {
    pub fn new(buf: &mut BytesMut) -> Result<Option<MinecraftHelloPacket>, PacketError> {
        if buf.len() < 2 {
            return Ok(None);
        }
        let mut cursor = CustomCursor::new(buf.as_ref());
        if cursor.match_bytes(&OLD_MINECRAFT_START[..2]) {
            cursor.set_position(0);
            MinecraftHelloPacket::old_ping_pkg(&mut cursor)
        } else if cursor.match_bytes(&[0x02]) {
            cursor.set_position(0);
            MinecraftHelloPacket::old_connect_pkg(&mut cursor)
        } else {
            MinecraftHelloPacket::new_pkg(&mut cursor)
        }
    }

    fn old_ping_pkg(
        cursor: &mut CustomCursor<&[u8]>,
    ) -> Result<Option<MinecraftHelloPacket>, PacketError> {
        if cursor.remaining() < OLD_MINECRAFT_START.len() {
            return Ok(None);
        }
        // check if the beginning is correct
        if !cursor.match_bytes(&OLD_MINECRAFT_START) {
            return Err(PacketError::NotValid);
        }
        if cursor.remaining() < size_of::<u16>() + size_of::<u8>() {
            return Ok(None);
        }
        // at pos 27 in buffer
        let rest_data = cursor.get_u16() as usize;
        let version = cursor.get_u8();
        // at pos 30
        let hostname = propagate!(cursor.get_utf16_string());
        if 7 + hostname.len() * size_of::<u16>() != rest_data {
            return Err(PacketError::NotValid);
        }
        if cursor.remaining() < size_of::<u32>() {
            return Ok(None);
        }
        let port = cursor.get_u32();

        Ok(Some(MinecraftHelloPacket {
            pkg_type: MinecraftHelloPacketType::Legacy,
            length: cursor.position() as usize,
            id: 0,
            version: version as i32,
            port,
            forge: false,
            hostname,
        }))
    }
    fn old_connect_pkg(
        cursor: &mut CustomCursor<&[u8]>,
    ) -> Result<Option<MinecraftHelloPacket>, PacketError> {
        if !cursor.match_bytes(&[0x02]) {
            return Err(PacketError::NotMatching);
        }
        // todo test if this is really the version!
        if cursor.remaining() < size_of::<u8>() {
            return Ok(None);
        }
        let version = cursor.get_u8();
        // wait for the packet to fully arrive
        let _username = propagate!(cursor.get_utf16_string());
        let hostname = propagate!(cursor.get_utf16_string());
        if cursor.remaining() < size_of::<u32>() {
            return Ok(None);
        }
        let port = cursor.get_u32();

        Ok(Some(MinecraftHelloPacket {
            pkg_type: MinecraftHelloPacketType::Legacy,
            length: cursor.position() as usize,
            id: 0,
            version: version as i32,
            port,
            hostname,
            // might not be correct always
            forge: false,
        }))
    }

    fn new_pkg(
        cursor: &mut CustomCursor<&[u8]>,
    ) -> Result<Option<MinecraftHelloPacket>, PacketError> {
        let pkg_length = propagate!(cursor.get_varint());
        let pkg_id = propagate!(cursor.get_varint());
        if pkg_id != 0 {
            return Err(PacketError::NotMatching);
        }
        let version = propagate!(cursor.get_varint());
        let hostname = propagate!(cursor.get_utf8_string());
        if cursor.remaining() < size_of::<u16>() {
            return Ok(None);
        }
        let port = cursor.get_u16();
        if cursor.position() as usize != pkg_length as usize {
            return Err(PacketError::NotValid);
        }
        let next_state = propagate!(cursor.get_varint());
        let pkg_type = match next_state {
            1 => MinecraftHelloPacketType::Ping,
            2 => MinecraftHelloPacketType::Connect,
            _ => MinecraftHelloPacketType::Unknown,
        };
        let forge = hostname.ends_with(FORGE_STRING);
        // removes everything after first \0
        let hostname = hostname
            .split_once('\0')
            .map(|(before, _)| before)
            .unwrap_or(&hostname);
        let hostname = hostname.to_string();

        Ok(Some(MinecraftHelloPacket {
            length: cursor.position() as usize,
            pkg_type,
            id: pkg_id,
            port: port as u32,
            version,
            hostname,
            forge,
        }))
    }
}

#[derive(Serialize)]
pub struct ServerListPingResponse {
    pub version: Version,
    pub description: MinecraftText,
    pub players: Players,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
}

#[derive(Serialize)]
pub struct Players {
    pub max: i32,
    pub online: i32,
    pub sample: Vec<Sample>,
}

#[derive(Serialize)]
pub struct Sample {
    pub name: String,
    pub id: String,
}

#[derive(Serialize)]
pub struct Version {
    pub name: String,
    pub protocol: i32,
}
#[derive(Serialize)]
pub struct MinecraftText {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub bold: bool,
}

impl MinecraftHelloPacket {
    pub fn generate_response(&self) -> MinecraftDataPacket {
        match &self.pkg_type {
            MinecraftHelloPacketType::Ping => self.generate_response_ping(),
            MinecraftHelloPacketType::Connect => self.generate_response_connect(),
            MinecraftHelloPacketType::Legacy => self.generate_response_legacy(),
            MinecraftHelloPacketType::Unknown => MinecraftDataPacket(Bytes::new()),
        }
    }

    /// Ping response https://wiki.vg/Protocol#Status_Response
    fn generate_response_ping(&self) -> MinecraftDataPacket {
        let resp = ServerListPingResponse {
            version: Version {
                name: ERROR_MSG_FIRST_LINE.to_string(),
                protocol: 0,
            },
            description: MinecraftText {
                text: "Ask your friend to start CraftIP!".into(),
                color: Some("red".into()),
                bold: false,
            },
            players: Players {
                max: 0,
                online: 0,
                sample: vec![],
            },
            favicon: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let mut cursor = CustomCursor::new(BytesMut::with_capacity(1024));
        cursor.put_varint(0);
        cursor.put_utf8_string(&json);

        MinecraftDataPacket::from_packet_without_len(mem::take(cursor.get_mut()).freeze())
    }
    fn generate_response_connect(&self) -> MinecraftDataPacket {
        let mut cursor = CustomCursor::new(BytesMut::with_capacity(1024));
        cursor.put_varint(0);
        let error_json = serde_json::to_string(&MinecraftText {
            text: format!("{}\n{}", ERROR_MSG_FIRST_LINE, ERROR_MSG_LONG),
            color: Some("red".into()),
            bold: false,
        })
        .unwrap();
        cursor.put_utf8_string(&error_json);

        MinecraftDataPacket::from_packet_without_len(mem::take(cursor.get_mut()).freeze())
    }

    fn generate_response_legacy(&self) -> MinecraftDataPacket {
        let mut buf = BytesMut::with_capacity(256);

        let resp_string = format!(
            "§1\0{}\0{}\0{}\0{}\0{}",
            "127",                // version
            ERROR_MSG_FIRST_LINE, // version readable
            ERROR_MSG_LONG,       // tip of minecraft server
            0,                    // player online
            0                     // player max
        );
        buf.put_u8(0xff); // kick packet
        buf.put_u16(resp_string.len() as u16 - 1); // length - 1 because MC seems to do it like that
        for char in resp_string.encode_utf16() {
            buf.put_u16(char);
        }
        MinecraftDataPacket::from(buf.freeze())
    }
}

impl MinecraftDataPacket {
    pub fn from_packet_without_len(content: Bytes) -> Self {
        let mut buf = BytesMut::with_capacity(content.len() + 5);
        {
            let mut cursor = CustomCursor::new(&mut buf);
            cursor.put_varint(content.len() as i32);
        }
        buf.put(content);
        MinecraftDataPacket::from(buf.freeze())
    }
}

#[cfg(test)]
mod test {
    use crate::minecraft::MinecraftDataPacket;
    use tokio_util::bytes::Bytes;

    #[test]
    fn test_serialization() {
        let reference = "Hello world";
        let pkg = MinecraftDataPacket::from(Bytes::from(reference));
        let serial = bincode::serialize(&pkg).unwrap();
        let deserialized_pkg = bincode::deserialize::<MinecraftDataPacket>(&serial).unwrap();
        assert_eq!(pkg, deserialized_pkg);
    }

    #[test]
    fn craft_minecraft_packet() {
        assert_eq!(
            MinecraftDataPacket::from_packet_without_len(Bytes::from(&b"hello"[..])),
            MinecraftDataPacket::from(Bytes::from(&b"\x05hello"[..]))
        );
    }
}
