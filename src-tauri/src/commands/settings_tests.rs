use super::{merge_settings_for_save, validate_app_config_dir_override};
use crate::settings::{
    AppSettings, CodexOfficialHistoryUnifyMigration, CodexProviderTemplateMigration,
    CodexThirdPartyHistoryProviderBucketMigration, LocalMigrations, S3SyncSettings,
    WebDavSyncSettings,
};

#[test]
fn save_settings_should_preserve_existing_webdav_when_payload_omits_it() {
    let existing = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let incoming = AppSettings::default();
    let merged = merge_settings_for_save(incoming, &existing);

    assert!(merged.webdav_sync.is_some());
    assert_eq!(
        merged.webdav_sync.as_ref().map(|v| v.base_url.as_str()),
        Some("https://dav.example.com")
    );
}

#[test]
fn save_settings_should_keep_incoming_webdav_when_present() {
    let existing = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.old.example.com".to_string(),
            username: "old".to_string(),
            password: "old-pass".to_string(),
            ..WebDavSyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let incoming = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.new.example.com".to_string(),
            username: "new".to_string(),
            password: "new-pass".to_string(),
            ..WebDavSyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let merged = merge_settings_for_save(incoming, &existing);

    assert_eq!(
        merged.webdav_sync.as_ref().map(|v| v.base_url.as_str()),
        Some("https://dav.new.example.com")
    );
}

#[test]
fn save_settings_should_keep_backend_signed_sync_flag() {
    let mut existing = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            ..WebDavSyncSettings::default()
        }),
        s3_sync: Some(S3SyncSettings {
            bucket: "bucket".to_string(),
            ..S3SyncSettings::default()
        }),
        ..AppSettings::default()
    };
    existing.webdav_sync.as_mut().unwrap().status.signed_sync = true;
    existing.s3_sync.as_mut().unwrap().status.signed_sync = true;
    let floor = Some("2026-03-02T10:00:00Z".to_string());
    existing
        .webdav_sync
        .as_mut()
        .unwrap()
        .status
        .last_signed_manifest_created_at = floor.clone();
    existing
        .s3_sync
        .as_mut()
        .unwrap()
        .status
        .last_signed_manifest_created_at = floor.clone();

    // A stale frontend copy without the flag must not lift the downgrade guard.
    let incoming = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            ..WebDavSyncSettings::default()
        }),
        s3_sync: Some(S3SyncSettings {
            bucket: "bucket".to_string(),
            ..S3SyncSettings::default()
        }),
        ..AppSettings::default()
    };
    let merged = merge_settings_for_save(incoming, &existing);
    let webdav_status = merged.webdav_sync.unwrap().status;
    let s3_status = merged.s3_sync.unwrap().status;
    assert!(webdav_status.signed_sync);
    assert!(s3_status.signed_sync);
    // Nor reset the rollback floor (CCS-239).
    assert_eq!(webdav_status.last_signed_manifest_created_at, floor);
    assert_eq!(s3_status.last_signed_manifest_created_at, floor);
}

/// Regression test: frontend always receives empty password from
/// get_settings_for_frontend(). If a component accidentally spreads
/// the full settings object into save_settings, the empty password
/// must NOT overwrite the existing one.
#[test]
fn save_settings_should_preserve_password_when_incoming_has_empty_password() {
    let existing = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        }),
        ..AppSettings::default()
    };

    // Simulate frontend sending settings with cleared password
    let incoming = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "".to_string(),
            ..WebDavSyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let merged = merge_settings_for_save(incoming, &existing);

    assert_eq!(
        merged.webdav_sync.as_ref().map(|v| v.password.as_str()),
        Some("secret"),
        "empty password from frontend must not overwrite existing password"
    );
}

/// When both incoming and existing have no password, merge should
/// work without panicking and keep the empty state.
#[test]
fn save_settings_should_handle_both_empty_passwords() {
    let existing = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "".to_string(),
            ..WebDavSyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let incoming = AppSettings {
        webdav_sync: Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "".to_string(),
            ..WebDavSyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let merged = merge_settings_for_save(incoming, &existing);

    assert_eq!(
        merged.webdav_sync.as_ref().map(|v| v.password.as_str()),
        Some("")
    );
}

#[test]
fn save_settings_should_preserve_existing_s3_when_payload_omits_it() {
    let existing = AppSettings {
        s3_sync: Some(S3SyncSettings {
            bucket: "bucket".to_string(),
            access_key_id: "ak".to_string(),
            secret_access_key: "secret".to_string(),
            ..S3SyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let incoming = AppSettings::default();
    let merged = merge_settings_for_save(incoming, &existing);

    assert!(merged.s3_sync.is_some());
    assert_eq!(
        merged
            .s3_sync
            .as_ref()
            .map(|v| v.secret_access_key.as_str()),
        Some("secret")
    );
}

#[test]
fn save_settings_should_preserve_s3_secret_when_incoming_has_empty_secret() {
    let existing = AppSettings {
        s3_sync: Some(S3SyncSettings {
            bucket: "bucket".to_string(),
            access_key_id: "ak".to_string(),
            secret_access_key: "secret".to_string(),
            ..S3SyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let incoming = AppSettings {
        s3_sync: Some(S3SyncSettings {
            bucket: "bucket".to_string(),
            access_key_id: "ak".to_string(),
            secret_access_key: "".to_string(),
            ..S3SyncSettings::default()
        }),
        ..AppSettings::default()
    };

    let merged = merge_settings_for_save(incoming, &existing);

    assert_eq!(
        merged
            .s3_sync
            .as_ref()
            .map(|v| v.secret_access_key.as_str()),
        Some("secret")
    );
}

#[test]
fn save_settings_should_preserve_local_migrations_when_payload_omits_it() {
    let existing = AppSettings {
        local_migrations: Some(LocalMigrations {
            codex_third_party_history_provider_bucket_v1: Some(
                CodexThirdPartyHistoryProviderBucketMigration {
                    completed_at: "2026-05-20T00:00:00Z".to_string(),
                    target_provider_id: "custom".to_string(),
                    source_provider_ids: vec!["rightcode".to_string()],
                    migrated_jsonl_files: 2,
                    migrated_state_rows: 3,
                    scanned_history_files: true,
                },
            ),
            codex_provider_template_v1: Some(CodexProviderTemplateMigration {
                completed_at: "2026-05-20T00:01:00Z".to_string(),
                migrated_provider_ids: vec!["legacy".to_string()],
            }),
            codex_official_history_unify_v1: Some(CodexOfficialHistoryUnifyMigration {
                completed_at: "2026-06-12T00:00:00Z".to_string(),
                target_provider_id: "custom".to_string(),
                migrated_jsonl_files: 5,
                migrated_state_rows: 7,
                codex_config_dir: None,
            }),
            deleted_builtin_model_pricing: vec!["claude-sonnet-5".to_string()],
        }),
        ..AppSettings::default()
    };

    let incoming = AppSettings::default();
    let merged = merge_settings_for_save(incoming, &existing);

    let migration = merged
        .local_migrations
        .as_ref()
        .and_then(|migrations| {
            migrations
                .codex_third_party_history_provider_bucket_v1
                .as_ref()
        })
        .expect("local migration marker should be preserved");
    assert_eq!(migration.target_provider_id, "custom");
    assert_eq!(migration.migrated_jsonl_files, 2);
    assert_eq!(migration.migrated_state_rows, 3);

    let template_migration = merged
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_provider_template_v1.as_ref())
        .expect("template migration marker should be preserved");
    assert_eq!(
        template_migration.migrated_provider_ids,
        vec!["legacy".to_string()]
    );

    let unify_migration = merged
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_official_history_unify_v1.as_ref())
        .expect("official unify migration marker should be preserved");
    assert_eq!(unify_migration.migrated_jsonl_files, 5);
    assert_eq!(unify_migration.migrated_state_rows, 7);
    assert_eq!(
        merged
            .local_migrations
            .as_ref()
            .map(|migrations| migrations.deleted_builtin_model_pricing.clone()),
        Some(vec!["claude-sonnet-5".to_string()]),
        "deleted builtin pricing markers should be preserved"
    );
}

/// incoming 带有 local_migrations（哪怕是空的）也不能覆盖后端维护的标记。
#[test]
fn save_settings_should_keep_backend_migration_markers_over_incoming() {
    let existing = AppSettings {
        local_migrations: Some(LocalMigrations {
            codex_third_party_history_provider_bucket_v1: None,
            codex_provider_template_v1: None,
            codex_official_history_unify_v1: Some(CodexOfficialHistoryUnifyMigration {
                completed_at: "2026-06-12T00:00:00Z".to_string(),
                target_provider_id: "custom".to_string(),
                migrated_jsonl_files: 1,
                migrated_state_rows: 2,
                codex_config_dir: None,
            }),
            deleted_builtin_model_pricing: Vec::new(),
        }),
        ..AppSettings::default()
    };

    let incoming = AppSettings {
        local_migrations: Some(LocalMigrations::default()),
        ..AppSettings::default()
    };
    let merged = merge_settings_for_save(incoming, &existing);

    assert!(merged
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_official_history_unify_v1.as_ref())
        .is_some());
}

/// 后端清掉 marker 后（如关闭统一会话开关）、前端缓存刷新前的全量保存
/// 会携带旧 marker；merge 必须忽略它，否则被"复活"的标记会让重新开启
/// 时误判已迁移而漏迁。
#[test]
fn save_settings_should_ignore_stale_incoming_migration_markers() {
    let existing = AppSettings::default();

    let incoming = AppSettings {
        local_migrations: Some(LocalMigrations {
            codex_official_history_unify_v1: Some(CodexOfficialHistoryUnifyMigration {
                completed_at: "2026-06-12T00:00:00Z".to_string(),
                target_provider_id: "custom".to_string(),
                migrated_jsonl_files: 1,
                migrated_state_rows: 2,
                codex_config_dir: None,
            }),
            ..LocalMigrations::default()
        }),
        ..AppSettings::default()
    };
    let merged = merge_settings_for_save(incoming, &existing);

    assert!(merged.local_migrations.is_none());
}

#[cfg(not(target_os = "windows"))]
#[test]
fn app_config_dir_override_rejects_paths_outside_home() {
    let home = std::path::Path::new("/home/ccs-075-user");
    for raw in [
        "/etc",
        "/tmp/attacker",
        "relative/dir",
        "/home/ccs-075-user/../other",
        "/home/ccs-075-userx",
    ] {
        assert!(
            validate_app_config_dir_override(raw, home, None).is_err(),
            "{raw} should be rejected"
        );
    }
}

#[cfg(not(target_os = "windows"))]
#[test]
fn app_config_dir_override_accepts_allowed_paths() {
    let home = std::path::Path::new("/home/ccs-075-user");
    assert!(validate_app_config_dir_override("", home, None).is_ok());
    assert!(validate_app_config_dir_override("  ", home, None).is_ok());
    assert!(
        validate_app_config_dir_override("/home/ccs-075-user/.cc-switch-alt", home, None).is_ok()
    );
    // 当前已生效的覆盖值可以原样保存
    let current = std::path::Path::new("/srv/ccs-075-current");
    assert!(validate_app_config_dir_override("/srv/ccs-075-current", home, Some(current)).is_ok());
    // 目录选择器选出的路径即使不在主目录下也可以使用
    let picked = "/mnt/ccs-075-picked";
    assert!(validate_app_config_dir_override(picked, home, None).is_err());
    crate::picked_paths::register(crate::picked_paths::PickPurpose::Directory, picked);
    assert!(validate_app_config_dir_override(picked, home, None).is_ok());
}

/// 前端整体保存与后端增量写入（迁移标记、同步设置）并发时不丢写
mod save_race {
    use super::super::persist_settings_for_save;
    use crate::paths::{self, Paths};
    use crate::settings::{
        get_settings, get_settings_for_frontend, mark_codex_provider_template_migrated,
        scoped_settings_store, set_webdav_sync_settings, AppSettings,
        CodexProviderTemplateMigration, SharedSettingsStore, WebDavSyncSettings,
    };

    fn template_marker(completed_at: &str) -> CodexProviderTemplateMigration {
        CodexProviderTemplateMigration {
            completed_at: completed_at.to_string(),
            migrated_provider_ids: Vec::new(),
        }
    }

    fn saved_file() -> serde_json::Value {
        let path = paths::current().settings_path();
        serde_json::from_slice(&std::fs::read(path).expect("read settings.json"))
            .expect("settings.json is valid json")
    }

    #[test]
    fn save_after_marker_commit_keeps_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _paths = paths::scoped(Paths::with_home(temp.path()));
        let _settings = scoped_settings_store();

        // 前端拿到的快照早于迁移标记写入
        let snapshot = get_settings_for_frontend();
        mark_codex_provider_template_migrated(template_marker("marker")).expect("mark");

        let mut incoming = snapshot;
        incoming.show_in_tray = !incoming.show_in_tray;
        let (previous, _) = persist_settings_for_save(incoming.clone()).expect("save");

        assert!(previous.local_migrations.is_some());
        let current = get_settings();
        assert_eq!(current.show_in_tray, incoming.show_in_tray);
        assert_eq!(
            current
                .local_migrations
                .as_ref()
                .and_then(|m| m.codex_provider_template_v1.as_ref())
                .map(|m| m.completed_at.as_str()),
            Some("marker")
        );
        assert_eq!(saved_file(), serde_json::to_value(&current).unwrap());
    }

    #[test]
    fn concurrent_sync_settings_and_saves_keep_file_and_memory_in_sync() {
        const ITERATIONS: usize = 200;
        const SAVERS: usize = 2;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().to_path_buf();
        let _paths = paths::scoped(Paths::with_home(&home));
        let _settings = scoped_settings_store();
        let shared = SharedSettingsStore::current();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let savers: Vec<_> = (0..SAVERS)
            .map(|n| {
                let shared = shared.clone();
                let home = home.clone();
                let done = done.clone();
                std::thread::spawn(move || {
                    let _paths = paths::scoped(Paths::with_home(&home));
                    let _settings = shared.install();
                    let mut i = 0usize;
                    while !done.load(std::sync::atomic::Ordering::SeqCst) {
                        // 不携带 webdav 的保存（如普通设置页保存）应保留现有同步设置
                        let incoming = AppSettings {
                            webdav_sync: None,
                            language: Some(format!("saver-{n}-{i}")),
                            ..get_settings_for_frontend()
                        };
                        persist_settings_for_save(incoming).expect("save");
                        i += 1;
                    }
                })
            })
            .collect();

        for i in 0..ITERATIONS {
            set_webdav_sync_settings(Some(WebDavSyncSettings {
                base_url: format!("https://dav.example.com/{i}"),
                username: "alice".to_string(),
                password: format!("secret-{i}"),
                ..WebDavSyncSettings::default()
            }))
            .expect("set webdav");
            mark_codex_provider_template_migrated(template_marker(&format!("marker-{i}")))
                .expect("mark");
        }
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        for saver in savers {
            saver.join().expect("saver thread");
        }

        let current = get_settings();
        let last = ITERATIONS - 1;
        let webdav = current.webdav_sync.as_ref().expect("webdav kept");
        assert_eq!(webdav.base_url, format!("https://dav.example.com/{last}"));
        assert_eq!(webdav.password, format!("secret-{last}"));
        assert_eq!(
            current
                .local_migrations
                .as_ref()
                .and_then(|m| m.codex_provider_template_v1.as_ref())
                .map(|m| m.completed_at.clone()),
            Some(format!("marker-{last}"))
        );
        // 密码移入凭据存储（测试构建为目录内的 secrets.json），文件中只留空字段
        let mut expected = current.clone();
        if let Some(sync) = expected.webdav_sync.as_mut() {
            sync.password.clear();
        }
        assert_eq!(saved_file(), serde_json::to_value(&expected).unwrap());
        let secrets = std::fs::read_to_string(
            paths::current()
                .settings_path()
                .with_file_name(crate::secret_store::SECRET_FILE_NAME),
        )
        .expect("read secrets.json");
        assert!(secrets.contains(&format!("\"secret-{last}\"")), "{secrets}");
    }
}
