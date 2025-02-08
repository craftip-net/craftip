use reqwest::StatusCode;
use serde::Serialize;
use std::env;
use std::error::Error;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::time::sleep;

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
    // prints debug info how many servers are connected
    tokio::spawn(stats_handler(register.clone()));
    loop {
        let (socket, _addr) = mc_listener.accept().await?;
        let register = register.clone();
        tokio::spawn(process_socket_connection(socket, register));
    }
}

#[derive(Serialize)]
struct StatsPost {
    auth: String,
    clients: usize,
    server: usize,
}

async fn stats_handler(register: Register) {
    let auth = env::var("STATS_AUTH").ok();
    if auth.is_none() {
        tracing::warn!("STATS_AUTH is not set. Stats won't be published")
    }
    let stats_url = env::var("STATS_URL").ok();
    if stats_url.is_none() {
        tracing::warn!("STATS_URL is not set. Stats won't be published")
    }

    let mut prev = (0, 0);
    let client = reqwest::Client::new();
    loop {
        sleep(Duration::from_secs(10)).await;
        let server_count = register.get_server_count().await;
        let client_count = register.get_client_count().await;
        if prev != (server_count, client_count) {
            tracing::info!(
                "Currently {server_count} servers and {client_count} clients connected..."
            );
            prev = (server_count, client_count);
        }

        let Some(auth) = &auth else {
            continue;
        };
        let Some(stats_url) = &stats_url else {
            continue;
        };

        let stats = StatsPost {
            auth: auth.clone(),
            server: server_count,
            clients: client_count,
        };
        let res = client.post(stats_url).json(&stats).send().await;
        if !matches!(res.as_ref().map(|r| r.status()), Ok(StatusCode::OK)) {
            tracing::warn!("Could not publish stats {:?}", res)
        }
    }
}
