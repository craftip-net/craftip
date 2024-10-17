use std::mem::size_of;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio_util::bytes::{Buf, Bytes, BytesMut};

use crate::cursor::{CustomCursor, CustomCursorMethods};
use crate::datatypes::PacketError;

const OLD_MINECRAFT_START: [u8; 27] = [
    0xFE, 0x01, 0xFA, 0x00, 0x0B, 0x00, 0x4D, 0x00, 0x43, 0x00, 0x7C, 0x00, 0x50, 0x00, 0x69, 0x00,
    0x6E, 0x00, 0x67, 0x00, 0x48, 0x00, 0x6F, 0x00, 0x73, 0x00, 0x74,
];

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct MinecraftHelloPacket {
    pub length: usize,
    pub id: i32,
    pub version: i32,
    pub hostname: String,
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
    pub fn new(buf: &mut BytesMut) -> Result<MinecraftHelloPacket, PacketError> {
        match MinecraftHelloPacket::old_ping_pkg(buf) {
            Ok(pkg) => return Ok(pkg),
            Err(PacketError::NotMatching) => {}
            result => {
                return result;
            }
        }
        match MinecraftHelloPacket::old_connect_pkg(buf) {
            Ok(pkg) => return Ok(pkg),
            Err(PacketError::NotMatching) => {}
            result => {
                return result;
            }
        }
        match MinecraftHelloPacket::new_pkg(buf) {
            Ok(pkg) => return Ok(pkg),
            Err(PacketError::NotMatching) => {}
            result => {
                return result;
            }
        }

        Err(PacketError::NotMatching)
    }

    fn old_ping_pkg(buf: &mut BytesMut) -> Result<MinecraftHelloPacket, PacketError> {
        let mut cursor = CustomCursor::new(buf.to_vec());
        if !cursor.match_bytes(&[0xFE, 0x01]) {
            return Err(PacketError::NotMatching);
        }
        // wait for the packet to fully arrive
        cursor.throw_error_if_smaller(32)?;
        // check if the beginning is correct
        if !cursor.match_bytes(&OLD_MINECRAFT_START[cursor.position() as usize..]) {
            return Err(PacketError::NotValid);
        }
        // at pos 27 in buffer
        let rest_data = cursor.get_u16() as usize;
        let version = cursor.get_u8();
        // at pos 30
        let hostname = cursor.get_utf16_string()?;

        if 7 + hostname.len() * 2 != rest_data {
            return Err(PacketError::NotValid);
        }
        cursor.throw_error_if_smaller(size_of::<u32>())?;
        let port = cursor.get_u32();

        Ok(MinecraftHelloPacket {
            length: cursor.position() as usize,
            id: 0,
            version: version as i32,
            port,
            hostname,
        })
    }
    fn old_connect_pkg(buf: &mut BytesMut) -> Result<MinecraftHelloPacket, PacketError> {
        let mut cursor = CustomCursor::new(buf.to_vec());
        if !cursor.match_bytes(&[0x02]) {
            return Err(PacketError::NotMatching);
        }
        // todo test if this is really the version!
        cursor.throw_error_if_smaller(size_of::<u8>())?;
        let version = cursor.get_u8();
        // wait for the packet to fully arrive
        let _username = cursor.get_utf16_string()?;
        let hostname = cursor.get_utf16_string()?;
        cursor.throw_error_if_smaller(size_of::<u32>())?;
        let port = cursor.get_u32();

        Ok(MinecraftHelloPacket {
            length: cursor.position() as usize,
            id: 0,
            version: version as i32,
            port,
            hostname,
        })
    }

    fn new_pkg(buf: &mut BytesMut) -> Result<MinecraftHelloPacket, PacketError> {
        let mut cursor = CustomCursor::new(buf.to_vec());
        let pkg_length = cursor.get_varint()?;
        let pkg_id = cursor.get_varint()?;
        if pkg_id != 0 {
            return Err(PacketError::NotMatching);
        }
        let version = cursor.get_varint()?;
        let hostname = cursor.get_utf8_string()?;
        cursor.throw_error_if_smaller(size_of::<u16>())?;
        let port = cursor.get_u16();
        if cursor.position() as usize != pkg_length as usize {
            return Err(PacketError::NotValid);
        }

        Ok(MinecraftHelloPacket {
            length: cursor.position() as usize,
            id: pkg_id,
            port: port as u32,
            version,
            hostname,
        })
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
}
