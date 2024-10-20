use shared::minecraft::{MinecraftDataPacket, MinecraftHelloPacket, MinecraftHelloPacketType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::bytes::{Buf, BytesMut};

pub(crate) async fn handle_mc_disconnect(
    packet: MinecraftHelloPacket,
    data: MinecraftDataPacket,
    mut socket: TcpStream,
) {
    let resp = packet.generate_response();
    let _res = socket.write_all(resp.as_ref()).await;

    if packet.pkg_type != MinecraftHelloPacketType::Ping {
        return;
    }
    let mut buf = BytesMut::from(&data.0[packet.length..]);
    // waiting for 12 bytes in total 2 for status request + 10 for ping packet
    while buf.len() < 12 {
        if !socket.read_buf(&mut buf).await.is_ok_and(|len| len != 0) {
            return;
        }
    }
    // Status Request https://wiki.vg/Server_List_Ping#Status_Request
    if buf.get_u16() != 0x0100 {
        tracing::debug!("expected to follow with 0x01 0x00");
        return;
    }
    // length 9 and 0x01 is the ping packet https://wiki.vg/Server_List_Ping#Ping_Request
    if buf.get_u16() != 0x0901 {
        tracing::debug!("expected to follow receive a ping packet");
        return;
    }

    let ping = buf.get_u64();

    let mut pong = [0u8; 10];
    pong[0] = 0x09; // length
    pong[1] = 0x01; // pong packet

    // put the ping number into the response
    pong[2..].copy_from_slice(ping.to_be_bytes().iter().as_slice());
    socket.write_all(&pong).await.unwrap();
}
