#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
mod backend;
mod config;
mod eula;
mod gui_channel;
mod help_popup;
mod updater_gui;

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};

use eframe::egui::{CentralPanel, Color32, IconData, Label, Layout, RichText, TextEdit, Ui};
use eframe::emath::Align;
use eframe::{egui, CreationContext, Storage};
use egui::containers;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::eula::accept_eula;
use crate::gui_channel::{GuiTriggeredChannel, GuiTriggeredEvent, ServerState};
use crate::help_popup::HelpPopup;
use crate::updater_gui::{updater_gui_headline, updater_no_consent};
use client::structs::{Server, ServerAuthentication, STANDARD_LOCAL_PORT};
use shared::crypto::ServerPrivateKey;
use updater::updater::UpdateInfo;
use updater::updater_proto::UpdaterError;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_HASH: &str = env!("GIT_HASH");

#[derive(Debug)]
pub enum UpdateState {
    CheckingForUpdate,
    UpToDate,
    NewVersionFound(UnboundedSender<bool>, UpdateInfo),
    Updating,
    Error(UpdaterError),
}

#[tokio::main]
pub async fn main() -> Result<(), eframe::Error> {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(false)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let (updater_tx, updater_rx) = mpsc::unbounded_channel();
    // this clone is on purpose so updater_tx lives during the whole program
    updater_no_consent(updater_tx.clone());

    let mut viewport = egui::ViewportBuilder::default().with_inner_size([500.0, 400.0]);
    viewport.icon = build_icon();
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "CraftIP",
        options,
        Box::new(|cc| {
            // add context to state to redraw from other threads
            Ok(Box::new(MyApp::new(cc, updater_rx)))
        }),
    )
}

fn build_icon() -> Option<Arc<IconData>> {
    let icon = include_bytes!("../../build/resources/logo-mac.png");
    let image = image::load_from_memory(icon)
        .expect("Image could not be loaded from memory")
        .into_rgba8();
    let (width, height) = image.dimensions();
    Some(Arc::new(IconData {
        rgba: image.into_raw(),
        width,
        height,
    }))
}

pub struct GuiState {
    loading: bool,
    update_status: UpdateState,
    error: Option<String>,
    servers: Option<Vec<ServerPanel>>,
    ctx: Option<egui::Context>,
}

impl GuiState {
    fn new() -> Self {
        Self {
            loading: false,
            update_status: UpdateState::CheckingForUpdate,
            error: None,
            servers: None,
            ctx: None,
        }
    }
    // set_active_server pass in closure the function that will be called on the active server
    fn set_active_server(&mut self, closure: impl FnOnce(&mut ServerPanel)) -> Result<()> {
        self.servers
            .as_mut()
            .ok_or(anyhow::anyhow!("no servers found"))?
            .iter_mut()
            .find(|s| s.state != ServerState::Disconnected)
            .map(closure)
            .context("no active server found")?;
        self.request_repaint();
        Ok(())
    }
    fn set_updater_status(&mut self, new_status: UpdateState) {
        self.update_status = new_status;
        self.request_repaint();
    }
    fn modify(&mut self, closure: impl FnOnce(&mut GuiState)) {
        closure(self);
        self.request_repaint();
    }
    fn set_ctx(&mut self, ctx: egui::Context) {
        self.ctx = Some(ctx);
    }
    fn request_repaint(&mut self) {
        match &self.ctx {
            Some(ctx) => ctx.request_repaint(),
            None => tracing::warn!("No repaint context set!"),
        }
    }
}

struct MyApp {
    state: Arc<Mutex<GuiState>>,
    tx: GuiTriggeredChannel,
    frames_rendered: usize,
    help: HelpPopup,
    eula: bool,
}

impl MyApp {
    fn new(cc: &CreationContext, updater_rx: UnboundedReceiver<UpdateState>) -> Self {
        let storage = cc.storage.unwrap();
        let servers = match storage.get_string("servers") {
            Some(servers) => {
                let servers: Vec<Server> = serde_json::from_str(&servers).unwrap();
                servers
            }
            None => {
                let key = ServerPrivateKey::default();
                let server = Server::new_from_key(key);
                vec![server]
            }
        };
        let server_panels = Some(servers.iter().map(ServerPanel::from).collect());
        let (gui_tx, gui_rx) = mpsc::unbounded_channel();
        let mut state = GuiState::new();
        state.servers = server_panels;
        state.set_ctx(cc.egui_ctx.clone());
        let state = Arc::new(Mutex::new(state));
        let mut controller = backend::Controller::new(gui_rx, updater_rx, state.clone());

        tokio::spawn(async move {
            controller.update().await;
        });

        let eula = matches!(
            cc.storage
                .unwrap()
                .get_string("eula_accepted")
                .map(|s| s == "true"),
            Some(true)
        );

        Self {
            tx: gui_tx,
            state,
            frames_rendered: 0,
            help: Default::default(),
            eula,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames_rendered += 1;
        let mut state = self.state.lock().unwrap();
        // draw ui
        CentralPanel::default().show(ctx, |ui| {
            if !self.eula {
                accept_eula(ui, &mut self.eula);
            }
            self.help.render(ui);
            if self.help.is_open() || !self.eula {
                ui.disable();
            }

            containers::menu::MenuBar::new().ui(ui, |ui| {
                ui.heading("CraftIP");
                if state.loading {
                    ui.spinner();
                }
                if ui.button("Report a problem").clicked() {
                    self.help.open();
                }
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    #[cfg(debug_assertions)]
                    ui.label(RichText::new(format!("{}", self.frames_rendered)).small());
                    updater_gui_headline(ui, &mut state.update_status);
                })
            });
            ui.separator();

            // enable/disable connect, disconnect buttons
            if let Some(servers) = &mut state.servers {
                let already_connected =
                    servers.iter().any(|s| s.state != ServerState::Disconnected);

                for server in servers.iter_mut() {
                    let enabled = !already_connected || server.state != ServerState::Disconnected;
                    server.render(ui, &mut self.tx, enabled)
                }
                if servers.is_empty() {
                    ui.label("No servers found");
                }
                /*if ui.button("+").clicked() {
                    println!("add button clicked");
                }*/
            } else {
                // still loading servers...
                ui.spinner();
            }
            if let Some(error) = &state.error {
                ui.label(RichText::new(error).color(Color32::RED));
                if ui.button("OK").clicked() {
                    state.error = None;
                }
            }
        });
    }
    fn save(&mut self, storage: &mut dyn Storage) {
        tracing::info!("Saving server key...");
        let servers: Vec<Server> = self
            .state
            .lock()
            .unwrap()
            .servers
            .as_ref()
            .unwrap()
            .iter()
            .map(Server::from)
            .collect();
        storage.set_string("servers", serde_json::to_string(&servers).unwrap());
        storage.set_string("eula_accepted", self.eula.to_string());
    }
}

#[derive(Debug, Clone)]
struct ServerPanel {
    server: String,
    auth: ServerAuthentication,
    local: String,
    edit_local: Option<String>,
    state: ServerState,
    ping: Option<u16>,
    error: Option<String>,
}

impl From<&Server> for ServerPanel {
    fn from(server: &Server) -> Self {
        //let name = name.trim_end_matches(&format!(":{}", shared::config::SERVER_PORT)).to_string();
        Self {
            state: ServerState::Disconnected,
            server: server.server.clone(),
            auth: server.auth.clone(),
            local: server.local.clone(),
            error: None,
            edit_local: None,
            ping: None,
        }
    }
}

impl ServerPanel {
    fn render(&mut self, ui: &mut Ui, tx: &mut GuiTriggeredChannel, enabled: bool) {
        let configurable = self.state == ServerState::Disconnected;
        ui.group(|ui| {
            if !enabled {
                ui.disable();
            }
            ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                egui::Grid::new(self.server.as_str())
                    .num_columns(3)
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        ui.style_mut().interaction.selectable_labels = false;
                        ui.add(Label::new("Server IP"));
                        ui.add(Label::new("ℹ"))
                            .on_hover_text("Share this address with your friends so they can join the server.");
                        ui.horizontal(|ui| {
                            ui.label(&self.server);
                            // copy button
                            if ui.button("📋").clicked() {
                                ui.ctx().copy_text(self.server.clone());
                            }
                        });
                        ui.end_row();

                        ui.add(Label::new("local port"));
                        ui.add(Label::new("ℹ"))
                            .on_hover_text("Enter the port of your Minecraft server running on your machine.\n(e.g. \"25565\" or \"localhost:25565\")");

                        ui.horizontal(|ui| {
                            match &mut self.edit_local {
                                None => {
                                    ui.label(&self.local);
                                    ui.vertical(|ui| {
                                        if !configurable {
                                            ui.disable();
                                        }
                                        if ui.button("✏").clicked() {
                                            self.edit_local = Some(self.local.clone());
                                        }
                                    });
                                    if ui.button("📋").clicked() {
                                        ui.ctx().copy_text(self.local.clone());
                                    }
                                }
                                Some(edit_local) => {
                                    let port = TextEdit::singleline(edit_local).desired_width(100.0);
                                    let ok = egui::Button::new(RichText::new("ok").color(Color32::DARK_GREEN));

                                    let update_txt = ui.add(port);
                                    let update_btn = ui.add(ok);

                                    let enter_pressed = update_txt.lost_focus() && ui.ctx().input(|i| { i.key_pressed(egui::Key::Enter) });

                                    if enter_pressed || update_btn.clicked() {
                                        self.local = self.edit_local.take().unwrap();
                                    }
                                    let cancel = egui::Button::new(RichText::new("cancel").color(Color32::RED));
                                    if ui.add(cancel).clicked() {
                                        self.edit_local = None;
                                    }
                                    let reset = egui::Button::new(RichText::new("reset"));
                                    if ui.add(reset).clicked() {
                                        self.edit_local = None;
                                        self.local = STANDARD_LOCAL_PORT.to_string();
                                    }
                                }
                            }
                        });

                        ui.end_row();
                    });

                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
                        match &self.state {
                            ServerState::Disconnected => {
                                ui.disable();
                                if ui.button("🗑").clicked() {
                                    println!("delete button clicked");
                                }
                            }
                            ServerState::Connecting(0) => {
                                ui.label("Connecting...");
                                ui.spinner();
                            }
                            ServerState::Connecting(attempt) => {
                                ui.label(format!("Connecting ({attempt}x)").as_str());
                                ui.spinner();
                            }
                            ServerState::Disconnecting => {
                                ui.label("Disconnecting...");
                                ui.spinner();
                            }
                            ServerState::Connected(connected) => {
                                // leaf green color
                                let label = ui.label(
                                    RichText::new(format!("{connected} Clients"))
                                        .color(Color32::from_rgb(0, 204, 0)),
                                );
                                if let Some(ping) = &self.ping {
                                    label.on_hover_text(
                                        RichText::new(format!("Ping {ping} ms")),
                                    );
                                }

                                ui.label("🔌");
                            }
                        }
                    });
                });
            });
            ui.vertical(|ui| {
                // center error
                if let Some(error) = self.error.clone() {
                    ui.label(RichText::new(error).color(Color32::RED));
                }
                if self.ping.unwrap_or(0) > 100 {
                    ui.label("Users far from Europe could experience high latency. We are working on this.");
                }
                if !enabled || self.edit_local.is_some() {
                    ui.disable()
                }
                //ui.set_enabled(enabled && self.edit_local.is_none());
                let button = match self.state {
                    ServerState::Disconnected => egui::Button::new("Connect"),
                    ServerState::Connecting(_) => egui::Button::new("Stop connecting"),
                    ServerState::Connected(_) => egui::Button::new("Disconnect"),
                    ServerState::Disconnecting => {
                        ui.disable();
                        egui::Button::new("Disconnecting...")
                    }
                };
                if ui
                    .add_sized(
                        egui::vec2(ui.available_width(), 30.0),
                        button,
                    )
                    .clicked()
                {
                    self.error = None;
                    match self.state {
                        ServerState::Connected(_) | ServerState::Connecting(_) => {
                            self.state = ServerState::Disconnecting;
                            tx.send(GuiTriggeredEvent::Disconnect())
                                .expect("failed to send disconnect event");
                        }
                        ServerState::Disconnected => {
                            self.state = ServerState::Connecting(0);
                            let mut server = Server::from(&self.clone());
                            let local = match server.local.parse::<u16>() {
                                Ok(port) => {
                                    format!("localhost:{port}")
                                }
                                Err(_) => server.local.clone()
                            };
                            server.local = local;
                            tx.send(GuiTriggeredEvent::Connect(server))
                                .expect("failed to send disconnect event");
                        }
                        _ => unreachable!("invalid state"),
                    }
                }
            });
        });
    }
}
