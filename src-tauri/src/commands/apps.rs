use crate::apps::registry::{registry_dto, AppDescriptorDto};
use crate::error::IpcError;

/// 返回后端应用注册表（应用 ID、名称键、同步模式与能力），前端据此渲染应用列表
#[tauri::command]
pub fn get_app_registry() -> Result<Vec<AppDescriptorDto>, IpcError> {
    Ok(registry_dto())
}
