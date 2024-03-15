use std::fs;
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

    let private_key = load_private_key();

    let server = Server {
        server: private_key.get_public_key().get_hostname(),
        local: "localhost:25565".to_string(),
        auth: ServerAuthentication::Key(private_key),
    };
    tracing::info!("Connecting to server: {}", server.server);

    let (_control_tx_new, control_rx) = mpsc::unbounded_channel();
    let (stats_tx, mut _stats_rx) = mpsc::unbounded_channel();

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
    let _result = client.handle().await;

    Ok(())
}


fn load_private_key() -> ServerPrivateKey {
    let project_dirs = directories_next::ProjectDirs::from("", "", "craftip-cli").unwrap();
    let config_dir = project_dirs.config_dir();
    if !config_dir.exists() {
        std::fs::create_dir_all(config_dir).unwrap();
    }
    // read key from file
    let key = std::fs::read(config_dir.join("private_key")).unwrap_or_else(|_| {
        let private_key = ServerPrivateKey::default();
        let bytes = bincode::serialize(&private_key).unwrap();
        fs::write(config_dir.join("private_key"), &bytes).unwrap();
        bytes
    });
    bincode::deserialize(&key).unwrap()
}