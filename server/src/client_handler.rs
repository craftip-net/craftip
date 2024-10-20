use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::bytes::{BufMut, BytesMut};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::Instant;

use crate::disconnect_client::handle_mc_disconnect;
use crate::process_socket::timeout;
use shared::addressing::{DistributorError, Register, Tx};
use shared::config::TIMEOUT_IN_SEC;
use shared::distributor_error;
use shared::minecraft::{MinecraftDataPacket, MinecraftHelloPacket};
use shared::socket_packet::{ClientID, ClientToProxy};

#[derive(Debug)]
pub struct MCClient {
    socket: Option<TcpStream>,
    rx: UnboundedReceiver<MinecraftDataPacket>,
    addr: SocketAddr,
    proxy_tx: Tx,
    need_for_close: bool,
    id: ClientID,
    hostname: String,
    connection_time: Instant,
}

pub(crate) async fn handle_minecraft_client(
    first_buf: &[u8],
    mut socket: TcpStream,
    register: Arc<Mutex<Register>>,
    socket_start: &Instant,
) -> Result<(), DistributorError> {
    let (packet, packet_data) = match timeout(
        &socket_start,
        first_minecraft_packet(&mut socket, &first_buf),
    )
    .await
    {
        //Err(DistributorError::Timeout) => return Ok(()),
        Err(e) => {
            tracing::debug!("Connection incomplete {:?}", e);
            return Ok(());
        }
        Ok(e) => e,
    };

    let proxy_tx = register.lock().await.servers.get(&packet.hostname).cloned();
    if proxy_tx.is_none() {
        // if there is no proxy connected for the corresponding server
        let _ = tokio::time::timeout(
            Duration::from_secs(TIMEOUT_IN_SEC),
            handle_mc_disconnect(packet, packet_data, &mut socket),
        )
        .await;
        return Ok(());
    }
    let proxy_tx = proxy_tx.ok_or(DistributorError::ServerNotFound(packet.hostname.clone()))?;

    let mut client = MCClient::new(proxy_tx.clone(), socket, packet, packet_data).await?;

    client
        .handle()
        .await
        .map_err(|e| {
            tracing::error!("{:?}", e);
            e
        })
        .map_err(Into::into)
}

impl MCClient {
    /// Create a new instance of `Peer`.
    pub(crate) async fn new(
        proxy_tx: Tx,
        socket: TcpStream,
        hello_packet: MinecraftHelloPacket,
        start_data: MinecraftDataPacket,
    ) -> anyhow::Result<Self, DistributorError> {
        // Get the client socket address
        let addr = socket
            .peer_addr()
            .map_err(distributor_error!("could not get peer address"))?;
        let hostname = hello_packet.hostname;
        let (tx, rx) = mpsc::unbounded_channel();
        tracing::info!("Minecraft client {} connected to {}", addr, hostname);

        let (id_tx, id_rx) = oneshot::channel();
        proxy_tx
            .send(ClientToProxy::AddMinecraftClient(id_tx, tx))
            .map_err(distributor_error!("no client id"))?;

        let id = id_rx.await.map_err(distributor_error!("no client id"))?;

        proxy_tx
            .send(ClientToProxy::Packet(id, start_data))
            .map_err(distributor_error!("could forward first data"))?;

        Ok(MCClient {
            socket: Some(socket),
            rx,
            proxy_tx,
            addr,
            need_for_close: true,
            id,
            hostname,
            connection_time: Instant::now(),
        })
    }
    async fn client_reader(
        mut reader: OwnedReadHalf,
        proxy_tx: UnboundedSender<ClientToProxy>,
        id: ClientID,
    ) {
        loop {
            let mut buf = BytesMut::with_capacity(2048);
            match reader.read_buf(&mut buf).await {
                // The stream has been exhausted.
                Ok(0) => break,
                Ok(_len) => {
                    let packet = MinecraftDataPacket::from(buf.freeze());
                    if let Err(e) = proxy_tx.send(ClientToProxy::Packet(id, packet)) {
                        tracing::error!("could not send to proxy distributor: {}", e);
                        break;
                    }
                }
                // An error occurred.
                Err(e) => {
                    tracing::error!("Error while receiving: {:?}", e);
                    break;
                }
            }
        }
    }
    /// HANDLE MC CLIENT
    pub async fn handle(&mut self) -> Result<(), DistributorError> {
        let socket = self.socket.take().unwrap();
        let (reader, mut writer) = socket.into_split();
        // read part of socke
        let mut reader = tokio::spawn(Self::client_reader(reader, self.proxy_tx.clone(), self.id));

        loop {
            tokio::select! {
                _ = &mut reader => break,
                res = self.rx.recv() => match res {
                    Some(pkg) => {
                        writer.write_all(pkg.as_ref()).await.map_err(distributor_error!("could not feed packet"))?;
                    }
                    None => {
                        self.need_for_close = false;
                        tracing::info!("client channel closed by minecraft server {}", self.addr);
                        break
                    }
                }
            }
        }
        Ok(())
    }
}
/// extracts the first packet form socket
/// used to get the virtual hostname
pub(crate) async fn first_minecraft_packet(
    socket: &mut TcpStream,
    first_buf: &[u8],
) -> Result<(MinecraftHelloPacket, MinecraftDataPacket), io::Error> {
    let mut buf = BytesMut::new();
    buf.put(first_buf);
    socket.try_read_buf(&mut buf)?;
    loop {
        if let Ok(Some(packet)) = MinecraftHelloPacket::new(&mut buf.clone()) {
            break Ok((packet, MinecraftDataPacket::from(buf.freeze())));
        }
        socket.read_buf(&mut buf).await?;
    }
}

impl Drop for MCClient {
    fn drop(&mut self) {
        tracing::info!(
            "Minecraft client {} disconnected after {:?} from {}",
            self.addr,
            self.connection_time.elapsed(),
            self.hostname
        );
        if self.need_for_close {
            let _ = self
                .proxy_tx
                .send(ClientToProxy::RemoveMinecraftClient(self.id));
        }
    }
}
