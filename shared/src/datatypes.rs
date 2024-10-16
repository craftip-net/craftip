use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PacketError {
    #[error("Packet is too small, missing Bytes")]
    TooSmall,
    #[error("Packet is not valid")]
    NotValid,
    #[error("String encoding is not valid")]
    NotValidStringEncoding,
    #[error("Not a valid first packet")]
    NotValidFirstPacket,
    #[error("Packet is not matching to decoder, do not recognize packet")]
    NotMatching,
    #[error("There has been an error during encoding")]
    EncodingError,
    #[error("Packet too long")]
    TooLong,
}

pub fn get_varint(buf: &[u8], start: usize) -> Result<(i32, usize), PacketError> {
    let mut value: i32 = 0;
    let mut position = 0;

    let mut size: usize = 0;

    loop {
        if size >= 5 {
            return Err(PacketError::NotValid);
        }
        if size + start >= buf.len() {
            return Err(PacketError::NotValid);
        }
        let current_byte = buf[size + start];

        value |= ((current_byte & 0x7F) as i32) << position;

        position += 7;
        size += 1;
        if (current_byte & 0x80) == 0 {
            return Ok((value, size));
        }
    }
}
