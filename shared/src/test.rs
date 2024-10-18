#[cfg(test)]
mod tests {
    use crate::cursor::{CustomCursor, CustomCursorRead, CustomCursorWrite};
    use std::ops::Deref;
    use tokio_util::bytes::{Buf, BufMut, BytesMut};

    use crate::minecraft::{MinecraftHelloPacket, MinecraftHelloPacketType};

    struct TestHelloPacket {
        name: String,
        packet: MinecraftHelloPacket,
        data: Vec<u8>,
    }

    struct TestVarInt {
        buffer: Vec<u8>,
        value: (i32, usize),
    }

    #[test]
    fn test_hello_packet_ping() {
        let test_vector = vec![
            TestHelloPacket {
                name: "ping with long hostname".to_string(),
                packet: MinecraftHelloPacket {
                    pkg_type: MinecraftHelloPacketType::Legacy,
                    length: 162,
                    id: 0,
                    version: 73,
                    hostname: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .parse()
                        .unwrap(),
                    port: 25565,
                },
                data: vec![
                    254, 1, 250, 0, 11, 0, 77, 0, 67, 0, 124, 0, 80, 0, 105, 0, 110, 0, 103, 0, 72,
                    0, 111, 0, 115, 0, 116, 0, 133, 73, 0, 63, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97,
                    0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 0, 99, 221,
                ],
            },
            TestHelloPacket {
                name: "ping with short hostname".to_string(),
                packet: MinecraftHelloPacket {
                    pkg_type: MinecraftHelloPacketType::Legacy,
                    length: 40,
                    id: 0,
                    version: 73,
                    hostname: "hi".parse().unwrap(),
                    port: 25565,
                },
                data: vec![
                    254, 1, 250, 0, 11, 0, 77, 0, 67, 0, 124, 0, 80, 0, 105, 0, 110, 0, 103, 0, 72,
                    0, 111, 0, 115, 0, 116, 0, 11, 73, 0, 2, 0, 104, 0, 105, 0, 0, 99, 221,
                ],
            },
            TestHelloPacket {
                name: "connect with long hostname".to_string(),
                packet: MinecraftHelloPacket {
                    pkg_type: MinecraftHelloPacketType::Legacy,
                    length: 158,
                    id: 0,
                    version: 73,
                    hostname: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .parse()
                        .unwrap(),
                    port: 25565,
                },
                data: vec![
                    2, 73, 0, 11, 0, 80, 0, 101, 0, 110, 0, 110, 0, 101, 0, 114, 0, 81, 0, 117, 0,
                    101, 0, 101, 0, 110, 0, 63, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0, 97, 0,
                    97, 0, 0, 99, 221,
                ],
            },
            TestHelloPacket {
                name: "connect with short hostname".to_string(),
                packet: MinecraftHelloPacket {
                    pkg_type: MinecraftHelloPacketType::Legacy,
                    length: 50,
                    id: 0,
                    version: 73,
                    hostname: "localhost".parse().unwrap(),
                    port: 25565,
                },
                data: vec![
                    2, 73, 0, 11, 0, 80, 0, 101, 0, 110, 0, 110, 0, 101, 0, 114, 0, 81, 0, 117, 0,
                    101, 0, 101, 0, 110, 0, 9, 0, 108, 0, 111, 0, 99, 0, 97, 0, 108, 0, 104, 0,
                    111, 0, 115, 0, 116, 0, 0, 99, 221,
                ],
            },
            TestHelloPacket {
                name: "connect with too long buffer".to_string(),
                packet: MinecraftHelloPacket {
                    pkg_type: MinecraftHelloPacketType::Legacy,
                    length: 50,
                    id: 0,
                    version: 73,
                    hostname: "localhost".parse().unwrap(),
                    port: 25565,
                },
                data: vec![
                    2, 73, 0, 11, 0, 80, 0, 101, 0, 110, 0, 110, 0, 101, 0, 114, 0, 81, 0, 117, 0,
                    101, 0, 101, 0, 110, 0, 9, 0, 108, 0, 111, 0, 99, 0, 97, 0, 108, 0, 104, 0,
                    111, 0, 115, 0, 116, 0, 0, 99, 221,
                ],
            },
            TestHelloPacket {
                name: "connect with new server".to_string(),
                packet: MinecraftHelloPacket {
                    pkg_type: MinecraftHelloPacketType::Connect,
                    length: 17,
                    id: 0,
                    version: 767,
                    hostname: "localhost".parse().unwrap(),
                    port: 25564,
                },
                data: Vec::from(
                    b"\x10\x00\xff\x05\x09\x6c\x6f\x63\x61\x6c\x68\x6f\x73\x74\x63\xdc\x02",
                ),
            },
            TestHelloPacket {
                name: "Ping new server".to_string(),
                packet: MinecraftHelloPacket {
                    pkg_type: MinecraftHelloPacketType::Ping,
                    length: 17,
                    id: 0,
                    version: 767,
                    hostname: "localhost".parse().unwrap(),
                    port: 25564,
                },
                data: Vec::from(
                    b"\x10\x00\xff\x05\x09\x6c\x6f\x63\x61\x6c\x68\x6f\x73\x74\x63\xdc\x01",
                ),
            },
            TestHelloPacket {
                name: "".to_string(),
                packet: MinecraftHelloPacket {
                    length: 42,
                    pkg_type: MinecraftHelloPacketType::Connect,
                    id: 0,
                    version: 767,
                    hostname: "1234abcd001992312312.t.craftip.net".to_string(),
                    port: 25565,
                },
                data: Vec::from(b")\0\xff\x05\"1234abcd001992312312.t.craftip.netc\xdd\x02"),
            },
        ];
        for test in test_vector {
            println!("Testing {}...", test.name);
            for len in 0..test.data.len() {
                let mut buf = BytesMut::with_capacity(1024);
                buf.put_slice(&test.data[..len]);
                let packet = MinecraftHelloPacket::new(&mut buf).unwrap();
                assert_eq!(packet, None);
            }
            let mut buf = BytesMut::with_capacity(1024);
            buf.put_slice(&test.data);
            let packet = MinecraftHelloPacket::new(&mut buf).unwrap();
            assert_eq!(packet, Some(test.packet.clone()));
        }
    }

    #[test]
    fn test_varint() {
        let test_vector = vec![
            TestVarInt {
                buffer: vec![0x00],
                value: (0, 1),
            },
            TestVarInt {
                buffer: vec![0x01],
                value: (1, 1),
            },
            TestVarInt {
                buffer: vec![0x7f],
                value: (127, 1),
            },
            TestVarInt {
                buffer: vec![0x80, 0x01],
                value: (128, 2),
            },
            TestVarInt {
                buffer: vec![0xff, 0xff, 0xff, 0xff, 0x07],
                value: (2147483647, 5),
            },
            TestVarInt {
                buffer: vec![0xff, 0xff, 0xff, 0xff, 0x0f],
                value: (-1, 5),
            },
            TestVarInt {
                buffer: vec![0x80, 0x80, 0x80, 0x80, 0x08],
                value: (-2147483648, 5),
            },
        ];
        for test in test_vector {
            println!(
                "Testing {:?} which should be {}...",
                test.buffer, test.value.0
            );
            // test incomplete varints
            for len in 0..test.buffer.len() {
                let mut cursor = CustomCursor::new(&test.buffer[..len]);
                let value = cursor.get_varint();
                assert_eq!(
                    value,
                    Ok(None),
                    "If packet is not complete, parser should return too small!"
                );
            }
            // test complete varints
            let mut cursor = CustomCursor::new(&test.buffer);
            let res = cursor.get_varint().unwrap().unwrap();
            assert_eq!(res, test.value.0);
            assert_eq!(
                cursor.remaining(),
                0,
                "Did not advance cursor sufficiently!"
            );
            let mut new_cursor = CustomCursor::new(Vec::new());
            new_cursor.put_varint(test.value.0);
            assert_eq!(new_cursor.get_ref().len() as usize, test.value.1);
            assert_eq!(new_cursor.get_ref()[..], test.buffer[..]);
        }
    }
    #[test]
    fn test_all_numbers() {
        let mut inner = vec![0; 32];
        let mut cursor = CustomCursor::new(&mut inner);
        for i in (i32::MIN..i32::MAX).step_by(1013) {
            cursor.get_mut().clear();
            cursor.put_varint(i);
            cursor.set_position(0);
            assert_eq!(cursor.get_varint(), Ok(Some(i)));
        }
    }

    #[test]
    fn test_utf8_str() {
        let test = vec![2, b'h', b'i'];
        let mut cursor = CustomCursor::new(Vec::new());
        cursor.put_utf8_string("hi");
        assert_eq!(cursor.get_ref(), test.deref());
        cursor.put_utf8_string("hi");
        assert_eq!(
            cursor.get_ref()[..],
            [test.clone(), test.clone()].concat()[..]
        );
    }
    #[test]
    // should not panic!
    fn test_random_bytes() {
        for i in 0..1000 {
            let mut buf = BytesMut::with_capacity(1024);
            let size = rand::random::<usize>() % 1024;

            for _ in 0..size {
                buf.put_u8(rand::random::<u8>());
            }
            println!("Test\t{i} random bytes with len {}...", size);

            //assert_eq!(packet.data, buffer);

            match MinecraftHelloPacket::new(&mut buf) {
                Ok(hello) => {
                    println!("Success: {:?}", hello)
                }
                Err(e) => {
                    println!("Error: {:?}", e);
                }
            }
        }
    }
}
