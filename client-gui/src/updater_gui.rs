use eframe::egui::{Button, Color32, Ui, Widget};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use crate::{updater, UpdateState};


/// Returns false if update check failed
pub(crate) fn updater_background_thread(updater_tx: UnboundedSender<UpdateState>) -> bool {
    let updater = updater::Updater::new();
    match updater {
        Ok(None) => {
            let _ = updater_tx.send(UpdateState::UpToDate);
            return true;
        }
        Err(e) => {
            let _ = updater_tx.send(UpdateState::Error(e));
            return false;
        }
        Ok(Some(updater)) => {
            let (execute_update_tx, mut execute_update_rx) = mpsc::unbounded_channel();
            let update_info = updater.get_update_info();
            let _ = updater_tx.send(UpdateState::NewVersionFound(execute_update_tx, update_info));

            // waiting for update to be triggered by UI
            let res = execute_update_rx.blocking_recv().unwrap_or(false);

            if res {
                let _ = updater_tx.send(UpdateState::Updating);
                if let Err(e) = updater.update() {
                    let _ = updater_tx.send(UpdateState::Error(e));
                } else {
                    updater.restart();
                }
            }
        }
    }
    return true;
}

pub(crate) fn updater_gui_headline(ui: &mut Ui, update_status: &mut UpdateState) {
    match update_status {
        UpdateState::Error(e) => {
            ui.colored_label(Color32::RED, format!("Error while updating: {}", e));
        },
        UpdateState::CheckingForUpdate => {
            ui.label("checking for update");
        },
        UpdateState::UpToDate => {
            ui.label("Up to date");
        },
        UpdateState::NewVersionFound(sender, info) => {
            let button = Button::new("Update").small().fill(Color32::LIGHT_GREEN).ui(ui);
            if button.clicked() {
                let _ = sender.send(true);
            }
            ui.label(format!("A new version {} is available!", info.version));
        },
        UpdateState::Updating => {
            ui.label("Updating...");
        }
    };
}