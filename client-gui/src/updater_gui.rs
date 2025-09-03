use crate::{UpdateState, CURRENT_VERSION};
use eframe::egui::{Button, Color32, RichText, Ui, Widget};
use std::thread;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use updater::updater::Updater;

/// Returns false if update check failed
pub(crate) fn updater_background_thread(updater_tx: UnboundedSender<UpdateState>) -> bool {
    let updater = Updater::new(CURRENT_VERSION);
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
                    updater.restart().unwrap();
                }
            }
        }
    }
    true
}

pub(crate) fn updater_no_consent(updater_tx: UnboundedSender<UpdateState>) {
    thread::spawn(move || match Updater::new(CURRENT_VERSION) {
        Ok(None) => {
            let _ = updater_tx.send(UpdateState::UpToDate);
        }
        Err(e) => {
            let _ = updater_tx.send(UpdateState::Error(e));
        }
        Ok(Some(updater)) => {
            let _ = updater_tx.send(UpdateState::Updating);
            if let Err(e) = updater.update() {
                let _ = updater_tx.send(UpdateState::Error(e));
                return;
            }
            if let Err(e) = updater.restart() {
                let _ = updater_tx.send(UpdateState::Error(e));
            }
        }
    });
}

pub(crate) fn updater_gui_headline(ui: &mut Ui, update_status: &mut UpdateState) {
    ui.label(RichText::new(format!("v{}", CURRENT_VERSION)).small());
    match update_status {
        UpdateState::Error(e) => {
            ui.colored_label(Color32::RED, format!("Updater: {e}"))
                .on_hover_text(format!("{e:?}"));
        }
        UpdateState::CheckingForUpdate => {
            ui.label("Checking for update...");
        }
        UpdateState::UpToDate => {}
        UpdateState::NewVersionFound(sender, info) => {
            let button = Button::new("Update")
                .small()
                .fill(Color32::LIGHT_GREEN)
                .ui(ui);
            if button.clicked() {
                let _ = sender.send(true);
            }
            ui.label(format!("A new version {} is available!", info.version));
        }
        UpdateState::Updating => {
            ui.label("Updating...");
        }
    };
}
