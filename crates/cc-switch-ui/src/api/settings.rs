use cc_switch_contract::commands::settings as cmd;
use cc_switch_contract::{AppSettings, IpcError};

use crate::ipc;

pub async fn get_settings() -> Result<AppSettings, IpcError> {
    ipc::invoke_no_args(cmd::GET_SETTINGS).await
}

pub async fn save_settings(settings: &AppSettings) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::SAVE_SETTINGS,
        &serde_json::json!({ "settings": settings }),
    )
    .await
}
