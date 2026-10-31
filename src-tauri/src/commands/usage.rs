//! 使用统计相关命令
//!
//! 所有命令都会持有 SQLite 全局连接锁做聚合查询或扫描会话日志，因此统一
//! 是 `async` 命令，并把 DB 工作放进阻塞线程池（CMD-08 / PERF-12），
//! 避免仪表盘查询卡住 UI 线程。

use super::run_blocking_with_state;
use crate::error::AppError;
pub use crate::services::usage_stats::ModelPricingInfo;
use crate::services::usage_stats::*;
use crate::store::AppState;
use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::{AppHandle, State};

/// 获取使用量汇总
#[tauri::command]
pub async fn get_usage_summary(
    handle: AppHandle,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<UsageSummary, AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.get_usage_summary(
            start_date,
            end_date,
            app_type.as_deref(),
            provider_name.as_deref(),
            model.as_deref(),
        )
    })
    .await
}

/// 获取按 app_type 拆分的使用量汇总
#[tauri::command]
pub async fn get_usage_summary_by_app(
    handle: AppHandle,
    start_date: Option<i64>,
    end_date: Option<i64>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<UsageSummaryByApp>, AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.get_usage_summary_by_app(
            start_date,
            end_date,
            provider_name.as_deref(),
            model.as_deref(),
        )
    })
    .await
}

/// 获取每日趋势
#[tauri::command]
pub async fn get_usage_trends(
    handle: AppHandle,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<DailyStats>, AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.get_daily_trends(
            start_date,
            end_date,
            app_type.as_deref(),
            provider_name.as_deref(),
            model.as_deref(),
        )
    })
    .await
}

/// 获取 Provider 统计
#[tauri::command]
pub async fn get_provider_stats(
    handle: AppHandle,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<ProviderStats>, AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.get_provider_stats(
            start_date,
            end_date,
            app_type.as_deref(),
            provider_name.as_deref(),
            model.as_deref(),
        )
    })
    .await
}

/// 获取模型统计
#[tauri::command]
pub async fn get_model_stats(
    handle: AppHandle,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<ModelStats>, AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.get_model_stats(
            start_date,
            end_date,
            app_type.as_deref(),
            provider_name.as_deref(),
            model.as_deref(),
        )
    })
    .await
}

/// 获取请求日志列表
#[tauri::command]
pub async fn get_request_logs(
    handle: AppHandle,
    filters: LogFilters,
    page: u32,
    page_size: u32,
) -> Result<PaginatedLogs, AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.get_request_logs(&filters, page, page_size)
    })
    .await
}

/// 获取单个请求详情
#[tauri::command]
pub async fn get_request_detail(
    handle: AppHandle,
    request_id: String,
) -> Result<Option<RequestLogDetail>, AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.get_request_detail(&request_id)
    })
    .await
}

/// 获取模型定价列表
#[tauri::command]
pub async fn get_model_pricing(handle: AppHandle) -> Result<Vec<ModelPricingInfo>, AppError> {
    run_blocking_with_state(handle, |state| {
        log::info!("获取模型定价列表");
        state.db.ensure_model_pricing_seeded()?;
        let pricing = state.db.list_model_pricing()?;
        log::info!("成功获取 {} 条模型定价数据", pricing.len());
        Ok(pricing)
    })
    .await
}

/// 校验并规范化前端提交的模型定价。
fn validate_model_pricing_input(
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
) -> Result<ModelPricingInfo, AppError> {
    let model_id = model_id.trim().to_string();
    let display_name = display_name.trim().to_string();
    if model_id.is_empty() {
        return Err(AppError::localized(
            "usage.modelIdRequired",
            "模型 ID 不能为空",
            "Model ID is required",
        ));
    }
    if display_name.is_empty() {
        return Err(AppError::localized(
            "usage.displayNameRequired",
            "显示名称不能为空",
            "Display name is required",
        ));
    }

    for (label, value) in [
        ("input_cost", &input_cost),
        ("output_cost", &output_cost),
        ("cache_read_cost", &cache_read_cost),
        ("cache_creation_cost", &cache_creation_cost),
    ] {
        let parsed = Decimal::from_str(value.trim()).map_err(|e| {
            AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格无效: {value} - {e}"),
                format!("{label} price is invalid: {value} - {e}"),
            )
        })?;
        if parsed < Decimal::ZERO {
            return Err(AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格必须为非负数: {value}"),
                format!("{label} price must be non-negative: {value}"),
            ));
        }
    }

    Ok(ModelPricingInfo {
        model_id,
        display_name,
        input_cost_per_million: input_cost.trim().to_string(),
        output_cost_per_million: output_cost.trim().to_string(),
        cache_read_cost_per_million: cache_read_cost.trim().to_string(),
        cache_creation_cost_per_million: cache_creation_cost.trim().to_string(),
    })
}

/// 更新模型定价
#[tauri::command]
#[specta::specta]
pub async fn update_model_pricing(
    handle: AppHandle,
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
) -> Result<(), AppError> {
    let pricing = validate_model_pricing_input(
        model_id,
        display_name,
        input_cost,
        output_cost,
        cache_read_cost,
        cache_creation_cost,
    )?;

    run_blocking_with_state(handle, move |state| {
        state.db.upsert_model_pricing(&pricing)?;

        let model_id = &pricing.model_id;
        if let Err(e) = state.db.backfill_missing_usage_costs_for_model(model_id) {
            log::warn!("模型定价更新后回填历史用量成本失败 (model_id={model_id}): {e}");
        }
        Ok(())
    })
    .await
}

/// 检查 Provider 使用限额
#[tauri::command]
pub async fn check_provider_limits(
    handle: AppHandle,
    provider_id: String,
    app_type: String,
) -> Result<crate::services::usage_stats::ProviderLimitStatus, AppError> {
    // Validate the webview-supplied app id before it reaches the DAO (CMD-23).
    let app_type = crate::app_config::AppType::from_str(&app_type)?
        .as_str()
        .to_string();
    run_blocking_with_state(handle, move |state| {
        state.db.check_provider_limits(&provider_id, &app_type)
    })
    .await
}

/// 删除模型定价
#[tauri::command]
#[specta::specta]
pub async fn delete_model_pricing(handle: AppHandle, model_id: String) -> Result<(), AppError> {
    run_blocking_with_state(handle, move |state| {
        state.db.delete_model_pricing(&model_id)?;
        log::info!("已删除模型定价: {model_id}");
        Ok(())
    })
    .await
}

/// 手动触发会话日志同步
#[tauri::command]
pub async fn sync_session_usage(
    state: State<'_, AppState>,
) -> Result<crate::services::session_usage::SessionSyncResult, AppError> {
    use crate::services::session_usage::{sync_all_session_usage, SyncTrigger};
    // 文件扫描是阻塞操作：放到 blocking 线程池；与后台同步单飞（等待其完成后再跑）
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_all_session_usage(&db, SyncTrigger::Manual).unwrap_or_default()
    })
    .await
    .map_err(|e| AppError::Database(format!("会话用量同步任务失败: {e}")))
}

/// 获取数据来源分布
#[tauri::command]
pub async fn get_usage_data_sources(
    handle: AppHandle,
) -> Result<Vec<crate::services::session_usage::DataSourceSummary>, AppError> {
    run_blocking_with_state(handle, |state| {
        crate::services::session_usage::get_data_source_breakdown(&state.db)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::validate_model_pricing_input;

    #[test]
    fn pricing_input_is_trimmed_and_validated() {
        let ok = validate_model_pricing_input(
            " m ".into(),
            " M ".into(),
            " 1 ".into(),
            "2".into(),
            "0".into(),
            "0.5".into(),
        )
        .expect("valid input");
        assert_eq!(ok.model_id, "m");
        assert_eq!(ok.display_name, "M");
        assert_eq!(ok.input_cost_per_million, "1");

        assert!(validate_model_pricing_input(
            " ".into(),
            "M".into(),
            "1".into(),
            "1".into(),
            "1".into(),
            "1".into()
        )
        .is_err());
        assert!(validate_model_pricing_input(
            "m".into(),
            "M".into(),
            "-1".into(),
            "1".into(),
            "1".into(),
            "1".into()
        )
        .is_err());
    }
}
