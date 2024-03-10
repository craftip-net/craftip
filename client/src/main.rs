use anyhow::Result;
use client::client::Client;
use client::structs::{Server, ServerAuthentication};
use shared::crypto::ServerPrivateKey;
use tokio::sync::mpsc;

#[tokio::main]
pub async fn main() -> Result<()> {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(false)
        .with_target(false)
        .without_time()
        .finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();
    tracing::info!("Starting client...");

    let private_key = ServerPrivateKey::default();
    let server = Server {
        server: private_key.get_public_key().get_hostname(),
        local: "localhost:25565".to_string(),
        auth: ServerAuthentication::Key(private_key),
    };
    tracing::info!("Connecting to server: {}", server.server);

    let (control_tx_new, control_rx) = mpsc::unbounded_channel();
    let (stats_tx, mut stats_rx) = mpsc::unbounded_channel();

    let mut client = Client::new(server, stats_tx, control_rx);
    // connect
    match client.connect().await {
        Ok(_) => {
            tracing::info!("Connected!");
        }
        Err(e) => {
            tracing::error!("Error connecting: {}", e);
            return Ok(());
        }
    }

    // handle handle connection if connection was successful
    tracing::info!("Handling connection...");
    let result = client.handle().await;

    Ok(())
}
