use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use crate::gui_channel::GuiTriggeredEvent;
use crate::gui_channel::ServerState;
use crate::{GuiState, UpdateState};
use client::client::Client;
use client::structs::Stats;
use client::structs::{ClientError, ControlTx};
use shared::config::TIMEOUT_IN_SEC;

pub struct Controller {
    pub gui_rx: UnboundedReceiver<GuiTriggeredEvent>,
    pub updater_rx: UnboundedReceiver<UpdateState>,
    pub state: Arc<Mutex<GuiState>>,
}

impl Controller {
    pub fn new(
        gui_rx: UnboundedReceiver<GuiTriggeredEvent>,
        updater_rx: UnboundedReceiver<UpdateState>,
        state: Arc<Mutex<GuiState>>,
    ) -> Self {
        Self {
            gui_rx,
            updater_rx,
            state,
        }
    }

    pub async fn update(&mut self) {
        let mut connection_task: Option<JoinHandle<()>> = None;
        let (stats_tx, mut stats_rx) = mpsc::unbounded_channel();
        loop {
            tokio::select! {
                result = stats_rx.recv() => {
                    if result.is_none() {
                        tracing::info!("Stats channel closed");
                        break;
                    }
                    let result = result.unwrap();
                    match result {
                        Stats::ClientsConnected(clients) => {
                            tracing::info!("Clients connected: {}", clients);
                            self.state.lock().unwrap().set_active_server(|s| {
                                s.state = ServerState::Connected(clients as u64);
                            }).unwrap();
                        }
                        Stats::Connected => {}
                        Stats::Ping(_ping) => {}
                    }
                }
                event = self.updater_rx.recv() => {
                    self.state.lock().unwrap().set_updater_status(event.unwrap());
                },
                event = self.gui_rx.recv() => {
                    if event.is_none() {
                        tracing::info!("GUI channel closed");
                        break;
                    }
                    let event = event.unwrap();
                    match event {
                        GuiTriggeredEvent::Connect(server) => {
                            let mut server = server.clone();
                            tracing::info!("Connecting to server: {}", server.server);
                            if !server.local.contains(':') {
                                server.server = format!("{}:{}", server.server, server.local);
                            }


                            let client = Client::new(server, stats_tx.clone());
                            let state = self.state.clone();

                            connection_task = Some(tokio::spawn(connection_loop(client, state.clone())));

                        }
                        GuiTriggeredEvent::Disconnect() => {
                            // sleep async 1 sec
                            if let Some(control_tx) = connection_task.take() {
                                control_tx.abort();
                            }
                            self.state.lock().unwrap().set_active_server(|s|s.state = ServerState::Disconnected).unwrap()
                        }
                    }
                }
            }
        }
    }
}

async fn connection_loop(mut client: Client, state: Arc<Mutex<GuiState>>) {
    let mut connection_attempt = 0;
    loop {
        if connection_attempt != 0 {
            sleep(Duration::from_secs(5)).await;
        }
        let connection_result = timeout(Duration::from_secs(TIMEOUT_IN_SEC), client.connect())
            .await
            .unwrap_or_else(|_| Err(ClientError::Timeout));
        state
            .lock()
            .unwrap()
            .set_active_server(|s| match &connection_result {
                Ok(()) => {
                    s.state = ServerState::Connected(0);
                    s.error = None;
                }
                Err(e) => {
                    s.error = Some(format!("Error connecting: {}", e));
                    s.state = ServerState::Connecting(connection_attempt);
                    tracing::error!("Error connecting: {}", e);
                }
            })
            .unwrap();
        if connection_result.is_err() {
            connection_attempt += 1;
            continue;
        }

        // handle connection if connection was successful
        connection_attempt = 0;
        match client.handle().await {
            Ok(()) => return,
            Err(err) => {
                tracing::error!("Found the following error: {:?}", err);
                state
                    .lock()
                    .unwrap()
                    .set_active_server(|s| s.error = Some(format!("Connection error: {}", err)))
                    .expect("Could not find active server")
            }
        }
    }
}
