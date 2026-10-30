//! Custom endpoints management
//!
//! Handles CRUD operations for provider custom endpoints.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::settings::CustomEndpoint;
use crate::store::AppState;

/// Get custom endpoints list for a provider
pub fn get_custom_endpoints(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<Vec<CustomEndpoint>, AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let Some(provider) = providers.get(provider_id) else {
        return Ok(vec![]);
    };
    let Some(meta) = provider.meta.as_ref() else {
        return Ok(vec![]);
    };
    if meta.custom_endpoints.is_empty() {
        return Ok(vec![]);
    }

    let mut result: Vec<_> = meta.custom_endpoints.values().cloned().collect();
    result.sort_by_key(|ep| std::cmp::Reverse(ep.added_at));
    Ok(result)
}

/// Add a custom endpoint to a provider
pub fn add_custom_endpoint(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    let normalized = url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err(AppError::localized(
            "provider.endpoint.url_required",
            "URL 不能为空",
            "URL cannot be empty",
        ));
    }

    state
        .db
        .add_custom_endpoint(app_type.as_str(), provider_id, &normalized)?;
    Ok(())
}

/// Remove a custom endpoint from a provider
pub fn remove_custom_endpoint(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    let normalized = url.trim().trim_end_matches('/').to_string();
    state
        .db
        .remove_custom_endpoint(app_type.as_str(), provider_id, &normalized)?;
    Ok(())
}

/// Mark an endpoint as used.
///
/// Intentionally a no-op that is kept only so the `update_endpoint_last_used`
/// command stays callable for older frontends. `last_used` has never been
/// persisted: the `provider_endpoints` table has no such column and
/// `save_provider` moves `meta.custom_endpoints` out of the row before writing
/// it. The previous implementation therefore re-read every provider and
/// rewrote the whole provider row (a lost-update hazard against concurrent
/// edits) while storing nothing. No UI reads `lastUsed`, so the write is
/// removed rather than given a schema migration.
pub fn update_endpoint_last_used(
    _state: &AppState,
    _app_type: AppType,
    _provider_id: &str,
    _url: String,
) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::Provider;
    use std::sync::Arc;

    fn total_changes(db: &Database) -> i64 {
        db.conn
            .lock()
            .expect("lock")
            .query_row("SELECT total_changes()", [], |row| row.get(0))
            .expect("total_changes")
    }

    #[test]
    fn update_endpoint_last_used_does_not_rewrite_provider_row() {
        let db = Arc::new(Database::memory().expect("db"));
        let state = AppState::new(db.clone());
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("claude", &provider).expect("save");
        add_custom_endpoint(
            &state,
            AppType::Claude,
            "p",
            "https://relay.example.com".to_string(),
        )
        .expect("add");

        let before = total_changes(&db);
        update_endpoint_last_used(
            &state,
            AppType::Claude,
            "p",
            "https://relay.example.com/".to_string(),
        )
        .expect("mark used");
        let after = total_changes(&db);
        assert_eq!(before, after, "marking an endpoint used must not write");

        let eps = get_custom_endpoints(&state, AppType::Claude, "p").expect("list");
        assert_eq!(eps.len(), 1);
    }
}
