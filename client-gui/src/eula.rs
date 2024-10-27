use eframe::egui;
use eframe::egui::{Align2, Button, Ui, Vec2};

pub(crate) fn accept_eula(ui: &mut Ui, accepted: &mut bool) {
    egui::Window::new("Accept EULA")
        .auto_sized()
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, Vec2::default())
        .show(ui.ctx(), |ui| {
            ui.style_mut().spacing.interact_size.y = 0.0;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("To run a Minecraft Server, you have\nto accept the ");
                ui.hyperlink_to("Minecraft EULA", "https://aka.ms/MinecraftEULA");
                ui.label(".");
            });

            ui.spacing();

            let button = Button::new("Accept");
            if ui.add_sized(egui::vec2(200.0, 20.0), button).clicked() {
                *accepted = true;
            }
        });
}
