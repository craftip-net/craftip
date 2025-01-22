use std::env;
use std::error::Error;

use tokio::net::TcpListener;

use crate::process_socket::process_socket_connection;
use crate::register::Register;

mod client_handler;
mod disconnect_client;
mod process_socket;
mod proxy_handler;
mod register;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(false)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:25565".to_string());

    let mc_listener = TcpListener::bind(&addr).await?;
    tracing::info!("server running on {:?}", mc_listener.local_addr()?);
    let register = Register::default();
    loop {
        let (socket, _addr) = mc_listener.accept().await?;
        let register = register.clone();
        tokio::spawn(async move {
            match process_socket_connection(socket, register).await {
                Ok(_) => tracing::debug!("client disconnected"),
                Err(e) => {
                    tracing::info!("client error: {:?}", e);
                }
            }
        });
    }
}
