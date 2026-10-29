use std::sync::{Arc, Barrier};

use cc_switch_lib::{get_claude_settings_path, update_settings, AppSettings, AppType};

#[path = "support.rs"]
mod support;
use support::{create_test_state, TestEnv};

/// 两个并行运行的测试各自拥有独立的 HOME、设置缓存与数据库，互不可见。
#[test]
fn parallel_test_envs_do_not_share_home_settings_or_db() {
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> =
        ["alpha", "beta"]
            .into_iter()
            .map(|name| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let test_env = TestEnv::new();
                    let home = test_env.home().to_path_buf();
                    let claude_dir = home.join(format!("claude-{name}"));
                    update_settings(AppSettings::default().with_config_dir(
                        &AppType::Claude,
                        claude_dir.to_string_lossy().into_owned(),
                    ))
                    .expect("update settings");
                    let state = create_test_state().expect("create test state");
                    state
                        .db
                        .set_setting("test_env_owner", name)
                        .expect("write db setting");

                    // 两个线程都写完后再读，确保读到的是各自的数据
                    barrier.wait();

                    let settings_path = get_claude_settings_path();
                    assert!(
                        settings_path.starts_with(&claude_dir),
                        "{name}: {settings_path:?} should be under {claude_dir:?}"
                    );
                    assert_eq!(
                        state
                            .db
                            .get_setting("test_env_owner")
                            .expect("read db setting")
                            .as_deref(),
                        Some(name)
                    );
                    assert!(home.join(".cc-switch").join("cc-switch.db").exists());
                    barrier.wait();
                    home
                })
            })
            .collect();

    let homes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("test thread"))
        .collect();
    assert_ne!(homes[0], homes[1]);
    for home in homes {
        assert!(!home.exists(), "TestEnv should remove {home:?} on drop");
    }
}

/// 嵌套的 TestEnv 丢弃后恢复之前注入的路径。
#[test]
fn test_env_restores_previous_paths_on_drop() {
    let outer = TestEnv::new();
    let outer_home = outer.home().to_path_buf();
    {
        let inner = TestEnv::new();
        assert_eq!(cc_switch_lib::paths::current().home(), inner.home());
    }
    assert_eq!(cc_switch_lib::paths::current().home(), outer_home);
    drop(outer);
    assert_ne!(cc_switch_lib::paths::current().home(), outer_home);
}
