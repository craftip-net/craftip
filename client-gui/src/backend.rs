use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::gui_channel::GuiTriggeredEvent;
use crate::gui_channel::ServerState;
use crate::{GuiState, UpdateState};
use client::client::Client;
use client::structs::ControlTx;
use client::structs::{Control, Stats};

pub struct Controller {
    pub gui_rx: UnboundedReceiver<GuiTriggeredEvent>,
    pub updater_rx: UnboundedReceiver<UpdateState>,
    pub state: Arc<Mutex<GuiState>>,
}

impl Controller {
    pub fn new(gui_rx: UnboundedReceiver<GuiTriggeredEvent>, updater_rx: UnboundedReceiver<UpdateState>, state: Arc<Mutex<GuiState>>) -> Self {
        Self { gui_rx, updater_rx, state }
    }

    pub async fn update(&mut self) {
        let mut control_tx: Option<ControlTx> = None;
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
                                s.connected = clients;
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

                            let (control_tx_new, control_rx) = mpsc::unbounded_channel();
                            control_tx = Some(control_tx_new);

                            let state = self.state.clone();
                            let mut client = Client::new(server, stats_tx.clone(), control_rx);
                            tokio::spawn(async move {
                                // connect
                                match client.connect().await {
                                    Ok(_) => {
                                        state.lock().unwrap().set_active_server(|s| {
                                            s.state = ServerState::Connected;
                                            s.connected = 0;
                                            s.error = None;
                                        }).unwrap();
                                    }
                                    Err(e) => {
                                        tracing::error!("Error connecting: {}", e);
                                        state.lock().unwrap().set_active_server(|s| {
                                            s.error = Some(format!("Error connecting: {}", e));
                                            s.state = ServerState::Disconnected;
                                        }).unwrap();
                                        return;
                                    }
                                }

                                // handle handle connection if connection was successful
                                let err = client.handle().await;
                                state.lock().unwrap().set_active_server(|s| {
                                    if let Err(e) = err {
                                        s.error = Some(format!("Error connecting: {}", e));
                                    }
                                    s.state = ServerState::Disconnected;
                                }).unwrap();
                            });
                        }
                        GuiTriggeredEvent::Disconnect() => {
                            // sleep async 1 sec
                            if let Some(control_tx) = &control_tx {
                                control_tx.send(Control::Disconnect).unwrap();
                            }
                        }
                    }
                }
            }
        }
    }
}
