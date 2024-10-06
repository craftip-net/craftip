#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
mod backend;
mod gui_channel;
mod updater;
mod updater_proto;

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui::{CentralPanel, Color32, IconData, Label, Layout, RichText, TextEdit, Ui};
use eframe::emath::Align;
use eframe::{egui, CreationContext, Storage, Renderer};
use tokio::sync::mpsc;

use crate::gui_channel::{GuiTriggeredChannel, GuiTriggeredEvent, ServerState};
use client::structs::{Server, ServerAuthentication};
use shared::crypto::ServerPrivateKey;
use crate::updater::CURRENT_VERSION;

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

    let mut viewport = egui::ViewportBuilder::default().with_inner_size([500.0, 400.0]);
    viewport.icon = build_icon();
    let options = eframe::NativeOptions {
        renderer: Renderer::Wgpu,
        viewport,
        ..Default::default()
    };

    thread::spawn(move || {
        let mut updater = updater::Updater::default();
        if updater.check_for_update().unwrap() {
            updater.update().unwrap();
            updater.restart().unwrap();
        }
    });
    eframe::run_native(
        "CraftIP",
        options,
        Box::new(|cc| {
            // add context to state to redraw from other threads
            Ok(Box::new(MyApp::new(cc)))
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
    error: Option<String>,
    servers: Option<Vec<ServerPanel>>,
    ctx: Option<egui::Context>,
}

impl GuiState {
    fn new() -> Self {
        Self {
            loading: false,
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
}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
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
        let mut controller = backend::Controller::new(gui_rx, state.clone());

        tokio::spawn(async move {
            controller.update().await;
        });

        Self {
            tx: gui_tx,
            state,
            frames_rendered: 0,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames_rendered += 1;
        let mut state = self.state.lock().unwrap();
        // draw ui
        CentralPanel::default().show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.heading("CraftIP");
                if state.loading {
                    ui.spinner();
                }
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    ui.label(RichText::new(format!("v{}", CURRENT_VERSION)).small());
                    #[cfg(debug_assertions)]
                    ui.label(RichText::new(format!("{}", self.frames_rendered)).small());
                });
            });
            ui.separator();

            // enable/disable connect, disconnect buttons
            if let Some(servers) = &mut state.servers {
                let already_connected =
                    servers.iter().any(|s| s.state != ServerState::Disconnected);

                servers.iter_mut().for_each(|server| {
                    let enabled = !already_connected || server.state != ServerState::Disconnected;
                    server.render(ui, &mut self.tx, enabled)
                });
                if servers.is_empty() {
                    ui.label("No servers found");
                }
                if ui.button("+").clicked() {
                    println!("add button clicked");
                }
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
            .map(|s| Server::from(s))
            .collect();
        storage.set_string("servers", serde_json::to_string(&servers).unwrap());
    }
}

#[derive(Debug, Clone)]
struct ServerPanel {
    server: String,
    auth: ServerAuthentication,
    connected: u16,
    local: String,
    edit_local: Option<String>,
    state: ServerState,
    error: Option<String>,
}

impl From<&Server> for ServerPanel {
    fn from(server: &Server) -> Self {
        //let name = name.trim_end_matches(&format!(":{}", shared::config::SERVER_PORT)).to_string();
        Self {
            state: ServerState::Disconnected,
            server: server.server.clone(),
            auth: server.auth.clone(),
            connected: 0,
            local: server.local.clone(),
            error: None,
            edit_local: None,
        }
    }
}

impl ServerPanel {
    fn render(&mut self, ui: &mut Ui, tx: &mut GuiTriggeredChannel, enabled: bool) {
        let configurable = self.state == ServerState::Disconnected;
        ui.group(|ui| {
            ui.set_enabled(enabled);
            ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                egui::Grid::new(self.server.as_str())
                    .num_columns(2)
                    .spacing([40.0, 4.0])
                    .show(ui, |ui| {
                        ui.add(Label::new("Server IP"))
                            .on_hover_text("Share this address with your friends so they can join the server.");

                        ui.horizontal(|ui| {
                            ui.label(&self.server);
                            // copy button
                            if ui.button("📋").clicked() {
                                ui.output_mut(|o| o.copied_text = self.server.clone());
                            }
                        });
                        ui.end_row();

                        ui.add(Label::new("local port"))
                            .on_hover_text("Enter the Port the Minecraft Server is running on your machine\nIf you want to open the word in LAN use the default port 25565");

                        ui.horizontal(|ui| {
                            match &mut self.edit_local {
                                None => {

                                    ui.label(&self.local);
                                    ui.vertical(|ui| {
                                        ui.set_enabled(configurable);
                                        if ui.button("✏").clicked() {
                                            self.edit_local = Some(self.local.clone());
                                        }
                                    });
                                    if ui.button("📋").clicked() {
                                        ui.output_mut(|o| o.copied_text = self.local.clone());
                                    }
                                }
                                Some(edit_local) => {
                                    let port = TextEdit::singleline(edit_local).desired_width(100.0);
                                    let ok = egui::Button::new(RichText::new("✔").color(Color32::DARK_GREEN));

                                    let update_txt = ui.add(port);
                                    let update_btn = ui.add(ok);

                                    let enter_pressed = update_txt.lost_focus() && ui.ctx().input(|i| {i.key_pressed(egui::Key::Enter)});

                                    if enter_pressed || update_btn.clicked() {
                                        self.local = self.edit_local.take().unwrap();
                                    }
                                    let cancel = egui::Button::new(RichText::new("❌").color(Color32::RED));
                                    if ui.add(cancel).clicked() {
                                        self.edit_local = None;
                                    }
                                }
                            }
                        });

                        ui.end_row();
                    });

                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
                        match self.state {
                            ServerState::Disconnected => {
                                if ui.button("🗑").clicked() {
                                    println!("delete button clicked");
                                }
                            }
                            ServerState::Connecting => {
                                ui.label("Connecting...");
                                ui.spinner();
                            }

                            ServerState::Disconnecting => {
                                ui.label("Disconnecting...");
                                ui.spinner();
                            }
                            ServerState::Connected => {
                                // leaf green color
                                ui.label(
                                    RichText::new(format!("{} Clients", self.connected))
                                        .color(Color32::from_rgb(0, 204, 0)),
                                );
                                ui.label("🔌");
                            }
                        }
                    });
                });
            });
            let (btn_txt,enabled) = match self.state {
                ServerState::Disconnected => ("Connect", true),
                ServerState::Connecting => ("Stop connecting", true),
                ServerState::Connected => ("Disconnect", true),
                ServerState::Disconnecting => ("Disconnecting...", false),
            };

            ui.vertical(|ui| {
                // center error
                if let Some(error) = self.error.clone() {
                    ui.label(RichText::new(error).color(Color32::RED));
                }
                ui.set_enabled(enabled && self.edit_local.is_none());
                if ui
                    .add_sized(
                        egui::vec2(ui.available_width(), 30.0),
                        egui::Button::new(btn_txt),
                    )
                    .clicked()
                {
                    self.error = None;
                    match self.state {
                        ServerState::Connected | ServerState::Connecting => {
                            self.state = ServerState::Disconnecting;
                            tx.send(GuiTriggeredEvent::Disconnect())
                                .expect("failed to send disconnect event");
                        }
                        ServerState::Disconnected => {
                            self.state = ServerState::Connecting;
                            let mut server = Server::from(&self.clone());
                            let local = match server.local.parse::<u16>() {
                                Ok(port) => {
                                    format!("localhost:{}", port)
                                },
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
