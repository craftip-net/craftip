use futures::future::select;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio_util::bytes::{BufMut, BytesMut};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::disconnect_client::handle_mc_disconnect;
use crate::process_socket::timeout;
use crate::proxy_handler::ClientToProxy;
use crate::register::{clean_up_hostname, Register, Tx};
use shared::addressing::DistributorError;
use shared::config::TIMEOUT_IN_SEC;
use shared::distributor_error;
use shared::minecraft::{MinecraftDataPacket, MinecraftHelloPacket};
use shared::socket_packet::ClientID;

#[derive(Debug)]
pub struct MCClient {
    socket: Option<TcpStream>,
    rx: Option<UnboundedReceiver<MinecraftDataPacket>>,
    addr: SocketAddr,
    proxy_tx: Tx,
    id: ClientID,
    hostname: String,
    connection_time: Instant,
}

pub(crate) async fn handle_minecraft_client(
    mut socket: TcpStream,
    register: Register,
    socket_start: &Instant,
) -> Result<(), DistributorError> {
    let (packet, packet_data) =
        match timeout(&socket_start, first_minecraft_packet(&mut socket)).await {
            //Err(DistributorError::Timeout) => return Ok(()),
            Err(e) => {
                tracing::debug!("Connection incomplete {:?}", e);
                return Ok(());
            }
            Ok(e) => e,
        };
    // this is important to get rid of prefixes that are used to prevent dns cache
    let hostname = clean_up_hostname(&packet.hostname);
    let proxy_tx = register.get_server(hostname).await;

    let Some(proxy_tx) = proxy_tx else {
        tracing::info!(
            "Server not found '{}' original packet hostname '{}'.",
            hostname,
            packet.hostname
        );
        // if there is no proxy connected for the corresponding server
        let _ = tokio::time::timeout(
            Duration::from_secs(TIMEOUT_IN_SEC),
            handle_mc_disconnect(packet, packet_data, &mut socket),
        )
        .await;
        return Ok(());
    };

    let mut client = MCClient::new(proxy_tx.clone(), socket, hostname, packet_data).await?;
    register.add_client().await;
    let res = client
        .handle()
        .await
        .map_err(|e| {
            tracing::error!("{:?}", e);
            e
        })
        .map_err(Into::into);
    register.remove_client().await;

    res
}

impl MCClient {
    /// Create a new instance of `Peer`.
    pub(crate) async fn new(
        proxy_tx: Tx,
        socket: TcpStream,
        hostname: &str,
        start_data: MinecraftDataPacket,
    ) -> anyhow::Result<Self, DistributorError> {
        // Get the client socket address
        let addr = socket
            .peer_addr()
            .map_err(distributor_error!("could not get peer address"))?;
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
            rx: Some(rx),
            proxy_tx,
            addr,
            id,
            hostname: hostname.to_string(),
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

    async fn client_writer(
        mut rx: UnboundedReceiver<MinecraftDataPacket>,
        mut writer: OwnedWriteHalf,
    ) {
        while let Some(pkg) = rx.recv().await {
            if let Err(e) = writer.write_all(pkg.as_ref()).await {
                tracing::debug!("write_all failed {e:?}");
                return;
            }
        }
    }
    /// HANDLE MC CLIENT
    pub async fn handle(&mut self) -> Result<(), DistributorError> {
        let socket = self.socket.take().unwrap();
        let rx = self.rx.take().unwrap();
        let (reader, writer) = socket.into_split();
        // read part of socket
        let reader = tokio::spawn(Self::client_reader(reader, self.proxy_tx.clone(), self.id));
        let writer = tokio::spawn(Self::client_writer(rx, writer));

        let _res = select(reader, writer).await;

        tracing::info!(
            "Minecraft client {} disconnected after {:?} from {}",
            self.addr,
            self.connection_time.elapsed(),
            self.hostname
        );

        let _ = self
            .proxy_tx
            .send(ClientToProxy::RemoveMinecraftClient(self.id));

        Ok(())
    }
}
/// extracts the first packet form socket
/// used to get the virtual hostname
pub(crate) async fn first_minecraft_packet(
    socket: &mut TcpStream,
) -> Result<(MinecraftHelloPacket, MinecraftDataPacket), io::Error> {
    let mut buf = BytesMut::new();
    loop {
        socket.read_buf(&mut buf).await?;
        if let Ok(Some(packet)) = MinecraftHelloPacket::new(&mut buf.clone()) {
            break Ok((packet, MinecraftDataPacket::from(buf.freeze())));
        }
    }
}
