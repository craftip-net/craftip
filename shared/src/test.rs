#[cfg(test)]
mod tests {
    use bytes::{BufMut, BytesMut};

    use crate::datatypes::get_varint;
    use crate::minecraft::MinecraftHelloPacket;

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
                    length: 16,
                    id: 0,
                    version: 761,
                    hostname: "localhost".parse().unwrap(),
                    port: 25565,
                },
                data: vec![
                    16, 0, 249, 5, 9, 108, 111, 99, 97, 108, 104, 111, 115, 116, 99, 221,
                ],
            },
        ];
        test_vector.iter().for_each(|test| {
            println!("Testing {}...", test.name);
            let mut buf = BytesMut::with_capacity(1024);
            buf.put_slice(&test.data);
            let packet = MinecraftHelloPacket::new(&mut buf).unwrap();

            assert_eq!(packet, test.packet);
        });
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
        test_vector.iter().for_each(|test| {
            println!("Testing {:?}...", test.value);
            let value = get_varint(&*test.buffer.clone(), 0).unwrap();
            assert_eq!(value, test.value);
        });
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
