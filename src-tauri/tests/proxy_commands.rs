use cc_switch_lib::{AppError, ProxySettingsService};

#[path = "support.rs"]
mod support;
use support::{create_test_state, TestEnv};

#[tokio::test]
async fn default_cost_multiplier_commands_round_trip() {
    let _test_env = TestEnv::new();

    let state = create_test_state().expect("create test state");

    let default = ProxySettingsService::default_cost_multiplier(&state, "claude")
        .await
        .expect("read default multiplier");
    assert_eq!(default, "1");

    ProxySettingsService::set_default_cost_multiplier(&state, "claude", "1.5")
        .await
        .expect("set multiplier");
    let updated = ProxySettingsService::default_cost_multiplier(&state, "claude")
        .await
        .expect("read updated multiplier");
    assert_eq!(updated, "1.5");

    let err = ProxySettingsService::set_default_cost_multiplier(&state, "claude", "not-a-number")
        .await
        .expect_err("invalid multiplier should error");
    // 错误已改为 Localized 类型（支持 i18n）
    match err {
        AppError::Localized { key, .. } => {
            assert_eq!(key, "error.invalidMultiplier");
        }
        other => panic!("expected localized error, got {other:?}"),
    }
}

#[tokio::test]
async fn pricing_model_source_commands_round_trip() {
    let _test_env = TestEnv::new();

    let state = create_test_state().expect("create test state");

    let default = ProxySettingsService::pricing_model_source(&state, "claude")
        .await
        .expect("read default pricing model source");
    assert_eq!(default, "response");

    ProxySettingsService::set_pricing_model_source(&state, "claude", "request")
        .await
        .expect("set pricing model source");
    let updated = ProxySettingsService::pricing_model_source(&state, "claude")
        .await
        .expect("read updated pricing model source");
    assert_eq!(updated, "request");

    let err = ProxySettingsService::set_pricing_model_source(&state, "claude", "invalid")
        .await
        .expect_err("invalid pricing model source should error");
    // 错误已改为 Localized 类型（支持 i18n）
    match err {
        AppError::Localized { key, .. } => {
            assert_eq!(key, "error.invalidPricingMode");
        }
        other => panic!("expected localized error, got {other:?}"),
    }
}
