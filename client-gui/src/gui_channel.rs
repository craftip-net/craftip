use crate::ServerPanel;
use client::structs::Server;
use tokio::sync::mpsc;

pub type GuiTriggeredChannel = mpsc::UnboundedSender<GuiTriggeredEvent>;

#[derive(Debug, Clone)]
pub enum GuiTriggeredEvent {
    Connect(Server),
    Disconnect(),
}

impl From<&ServerPanel> for Server {
    fn from(server_panel: &ServerPanel) -> Self {
        Self {
            server: server_panel.server.clone(),
            local: server_panel.local.clone(),
            auth: server_panel.auth.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    Disconnected,
    Connecting(u64),
    Connected,
    Disconnecting,
}
