use crate::datatypes::PacketError;
use crate::propagate;
use std::io::{Cursor, Read};
use std::mem::size_of;
use std::ops::Not;
use tokio_util::bytes::{Buf, BufMut};

pub type CustomCursor<T> = Cursor<T>;

pub(crate) trait CustomCursorRead {
    fn get_varint(&mut self) -> Result<Option<i32>, PacketError>;
    fn get_utf8_string(&mut self) -> Result<Option<String>, PacketError>;
    fn get_utf16_string(&mut self) -> Result<Option<String>, PacketError>;
    fn match_bytes(&mut self, bytes: &[u8]) -> bool;
}

pub(crate) trait CustomCursorWrite {
    fn put_varint(&mut self, value: i32);
    fn put_utf8_string(&mut self, string: &str);
}

impl<T: AsRef<[u8]>> CustomCursorRead for CustomCursor<T> {
    /// get the varint form buffer and advance cursor
    fn get_varint(&mut self) -> Result<Option<i32>, PacketError> {
        let mut value: i32 = 0;
        let mut position = 0;
        for _ in 0..5 {
            if self.remaining() < size_of::<u8>() {
                return Ok(None);
            }
            let current_byte = self.get_u8();

            value |= ((current_byte & 0x7F) as i32) << position;

            position += 7;

            if (current_byte & 0x80) == 0 {
                return Ok(Some(value));
            }
        }
        Err(PacketError::NotValid)
    }

    /// Returns the string and the size of the string (including the size) in bytes
    fn get_utf8_string(&mut self) -> Result<Option<String>, PacketError> {
        let size = propagate!(self.get_varint()) as usize;
        if self.remaining() < size {
            return Ok(None);
        }
        let blob =
            &self.get_ref().as_ref()[self.position() as usize..self.position() as usize + size];
        let result = String::from_utf8(blob.to_owned());
        self.advance(size);
        match result {
            Ok(s) => Ok(Some(s)),
            Err(_) => Err(PacketError::NotValidStringEncoding),
        }
    }
    /// reads length and string from the buffer
    fn get_utf16_string(&mut self) -> Result<Option<String>, PacketError> {
        let courser_start = self.position();
        if self.remaining() < size_of::<u16>() {
            return Ok(None);
        }
        let size = self.get_u16() as usize;
        if self.remaining() <= size * size_of::<u16>() {
            return Ok(None);
        }
        let iter = (0..size).map(|_| self.get_u16());

        let result = std::char::decode_utf16(iter).collect::<Result<String, _>>();

        match result {
            Ok(s) => Ok(Some(s)),
            Err(_) => {
                self.set_position(courser_start);
                Err(PacketError::NotValidStringEncoding)
            }
        }
    }
    /// matches the bytes in the buffer with the given bytes and if they match advance cursor
    fn match_bytes(&mut self, bytes: &[u8]) -> bool {
        if self.remaining() < bytes.len() {
            return false;
        }
        for (i, byte) in bytes.iter().enumerate() {
            if self.get_ref().as_ref()[self.position() as usize + i] != *byte {
                return false;
            }
        }
        self.advance(bytes.len());
        true
    }
}

impl<T: BufMut> CustomCursorWrite for CustomCursor<T> {
    fn put_varint(&mut self, value: i32) {
        let mut value = value;
        for _ in 0..5 {
            if (value & 0b0111_1111_i32.not()) == 0 {
                self.get_mut().put_u8(value as u8);
                return;
            }
            self.get_mut()
                .put_u8((value & 0b0111_1111) as u8 | 0b1000_0000);
            value = ((value as u32) >> 7) as i32;
        }
    }

    fn put_utf8_string(&mut self, string: &str) {
        self.put_varint(string.len() as i32);
        self.get_mut().put(string.as_bytes());
    }
}
