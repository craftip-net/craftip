use crate::datatypes::{get_varint, PacketError};
use std::io::Cursor;
use std::mem::size_of;
use tokio_util::bytes::Buf;

pub type CustomCursor = Cursor<Vec<u8>>;

pub(crate) trait CustomCursorMethods {
    fn new(buf: Vec<u8>) -> Self;
    fn get_varint(&mut self) -> Result<i32, PacketError>;
    fn get_utf8_string(&mut self) -> Result<String, PacketError>;
    fn throw_error_if_smaller(&mut self, size: usize) -> Result<(), PacketError>;
    fn get_utf16_string(&mut self) -> Result<String, PacketError>;
    fn match_bytes(&mut self, bytes: &[u8]) -> bool;
}

impl CustomCursorMethods for CustomCursor {
    fn new(buf: Vec<u8>) -> Self {
        Self::new(buf)
    }
    /// get the varint form buffer and advance cursor
    fn get_varint(&mut self) -> Result<i32, PacketError> {
        let (value, size) = get_varint(&self.get_ref()[self.position() as usize..])?;
        self.set_position(self.position() + size as u64);
        Ok(value)
    }
    /// Returns the string and the size of the string (including the size) in bytes
    fn get_utf8_string(&mut self) -> Result<String, PacketError> {
        let start_postion = self.position();
        let size = self.get_varint()? as usize;
        self.throw_error_if_smaller(size)?;
        let blob =
            self.get_ref()[self.position() as usize..self.position() as usize + size].to_owned();
        let result = String::from_utf8(blob);
        self.set_position(self.position() + size as u64);
        match result {
            Ok(s) => Ok(s),
            Err(_) => {
                self.set_position(start_postion);
                Err(PacketError::NotValidStringEncoding)
            }
        }
    }
    /// throws a PacketError::TooSmall if the buffer is smaller than the given size
    fn throw_error_if_smaller(&mut self, size: usize) -> Result<(), PacketError> {
        if self.remaining() < size {
            return Err(PacketError::TooSmall);
        }
        Ok(())
    }
    /// reads length and string from the buffer
    fn get_utf16_string(&mut self) -> Result<String, PacketError> {
        let courser_start = self.position();
        //assert!(2*size <= slice.len());
        self.throw_error_if_smaller(size_of::<u16>())?;
        let size = self.get_u16() as usize;
        if self.remaining() <= size * 2 {
            return Err(PacketError::TooSmall);
        }
        self.throw_error_if_smaller(size * 2)?;
        let iter = (0..size).map(|_| self.get_u16());

        let result = std::char::decode_utf16(iter).collect::<Result<String, _>>();

        match result {
            Ok(s) => Ok(s),
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
            if self.get_ref()[self.position() as usize + i] != *byte {
                return false;
            }
        }
        self.set_position(self.position() + bytes.len() as u64);
        true
    }
}
