use crate::config::UPDATE_ERROR_REPORT_URL;
use crate::updater_proto::UpdaterError;
use serde::{Deserialize, Serialize};
use ureq::Agent;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct UpdateReport {
    version: String,
    target: String,
    error: String,
}

pub fn report_update_error(agent: Agent, error: &UpdaterError, current_version: &str) {
    let res = agent.post(UPDATE_ERROR_REPORT_URL).send_json(UpdateReport {
        version: current_version.to_string(),
        target: current_platform::CURRENT_PLATFORM.to_string(),
        error: format!("{:?}", error),
    });
    if let Err(err) = res {
        println!("Could not report update error: {:?}", err);
    }
}
