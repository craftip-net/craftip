use crate::config::REPORT_PROBLEM;
use eframe::egui;
use eframe::egui::{Align, Align2, Label, Layout, RichText, TextEdit, Ui, Vec2};
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use updater::updater::CURRENT_VERSION;

#[derive(Default)]
pub struct HelpPopup {
    open: bool,
    email: String,
    text: String,
    promise: Option<Promise<Result<(), String>>>,
    error: Option<String>,
}

impl HelpPopup {
    pub(crate) fn open(&mut self) {
        self.open = true;
    }
}

impl HelpPopup {
    pub(crate) fn render(&mut self, ui: &mut Ui) {
        let mut open = std::mem::take(&mut self.open);
        let mut close = false;
        egui::Window::new("Report a problem")
            .open(&mut open)
            .auto_sized()
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, Vec2::default())
            .show(ui.ctx(), |ui| {
                let label = "This program is still in its early stages. If you have a problem or suggestion, feel free to write it here!";
                let label = Label::new(RichText::new(label));
                ui.add_sized(Vec2::new(300.0, 10.0), label);
                ui.label("");

                ui.set_enabled(self.promise.is_none());

                ui.label("Email for response (optional):");
                ui.text_edit_singleline(&mut self.email);
                ui.label("Comment:");
                let text = TextEdit::multiline(&mut self.text).desired_rows(10);
                ui.add(text);
                ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                    if ui.button("Send").clicked() {
                        self.error = None;
                        let ctx = ui.ctx().clone();
                        let (sender, promise) = Promise::new();
                        let feedback = Feedback::new(self.email.as_str(), self.text.as_str());
                        let request = ehttp::Request::json(REPORT_PROBLEM, &feedback).unwrap();
                        self.promise = Some(promise);
                        ehttp::fetch(request, move |response| {
                            ctx.request_repaint();
                            // response has to match "ok"
                            sender.send(response.and_then(|resp| {
                                resp.text()
                                    .ok_or_else(|| "Error: Got nothing from server".to_string())
                                    .and_then(|resp| {
                                        if resp != "ok" {
                                            Err(resp.to_string())
                                        } else {
                                            Ok(())
                                        }
                                    })
                            }))
                        });
                    }
                    // fuck the borrow checker
                    let mut request_completed = false;
                    // waiting for the response to be ready
                    if let Some(promise) = &self.promise {
                        if let Some(result) = promise.ready() {
                            request_completed = true;
                            match result {
                                Ok(()) => {
                                    close = true;
                                    self.email.clear();
                                    self.text.clear();
                                },
                                Err(e) => self.error = Some(e.clone()),
                            }
                        } else {
                            ui.spinner();
                        }
                    }
                    if request_completed {
                        self.promise = None;
                    }
                    if let Some(error) = &self.error {
                        let error = if error.is_empty() { "Error" } else { error };
                        let label = Label::new(
                            Into::<RichText>::into(error).color(ui.visuals().error_fg_color),
                        )
                        .wrap(true);
                        ui.add_sized(Vec2::new(200.0, 10.0), label);
                    }
                });
            });
        // borrow checker is sometimes a bitch!
        self.open = open && !close;
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Feedback {
    email: String,
    text: String,
    target: String,
    version: String,
}

impl Feedback {
    fn new(email: &str, text: &str) -> Feedback {
        Feedback {
            email: email.to_string(),
            text: text.to_string(),
            target: current_platform::CURRENT_PLATFORM.to_string(),
            version: CURRENT_VERSION.to_string(),
        }
    }
}
