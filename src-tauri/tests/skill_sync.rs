use std::fs;

use cc_switch_lib::{
    migrate_skills_to_ssot, AppType, ImportSkillSelection, InstalledSkill, SkillApps, SkillService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(dir: &std::path::Path, name: &str) {
    fs::create_dir_all(dir).expect("create skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill\n---\n"),
    )
    .expect("write SKILL.md");
}

#[cfg(unix)]
fn symlink_dir(src: &std::path::Path, dest: &std::path::Path) {
    std::os::unix::fs::symlink(src, dest).expect("create symlink");
}

#[cfg(windows)]
fn symlink_dir(src: &std::path::Path, dest: &std::path::Path) {
    std::os::windows::fs::symlink_dir(src, dest).expect("create symlink");
}

#[test]
fn import_from_apps_respects_explicit_app_selection() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("shared-skill"),
        "Shared",
    );
    write_skill(
        &home
            .join(".config")
            .join("opencode")
            .join("skills")
            .join("shared-skill"),
        "Shared",
    );

    let state = create_test_state().expect("create test state");

    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "shared-skill".to_string(),
            apps: SkillApps {
                opencode: true,
                ..Default::default()
            },
        }],
    )
    .expect("import skills");

    assert_eq!(imported.len(), 1, "expected exactly one imported skill");
    let skill = imported.first().expect("imported skill");
    assert!(
        skill.apps.opencode,
        "explicitly selected OpenCode app should remain enabled"
    );
    assert!(
        !skill.apps.claude && !skill.apps.codex && !skill.apps.gemini,
        "import should no longer infer apps from every matching source path"
    );
}

#[test]
fn import_from_apps_does_not_rewrite_selected_app_directory() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("codex-skill");
    write_skill(&ssot_skill_dir, "Stale SSOT Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "stale ssot").expect("write stale ssot prompt");

    let codex_skill_dir = home.join(".codex").join("skills").join("codex-skill");
    write_skill(&codex_skill_dir, "Live Codex Skill");
    fs::write(codex_skill_dir.join("prompt.md"), "live codex").expect("write live codex prompt");

    let state = create_test_state().expect("create test state");

    let imported = SkillService::import_from_apps(
        &state.db,
        vec![ImportSkillSelection {
            directory: "codex-skill".to_string(),
            apps: SkillApps {
                codex: true,
                ..Default::default()
            },
        }],
    )
    .expect("import skills");

    assert_eq!(imported.len(), 1, "expected exactly one imported skill");
    assert!(
        imported[0].apps.codex,
        "import should preserve the selected Codex app state"
    );
    assert_eq!(
        fs::read_to_string(codex_skill_dir.join("prompt.md")).expect("read live codex prompt"),
        "live codex",
        "import should not replace the app skill directory with SSOT contents"
    );
    assert!(
        !fs::symlink_metadata(&codex_skill_dir)
            .expect("read codex skill metadata")
            .file_type()
            .is_symlink(),
        "import should not replace the app skill directory with a managed symlink"
    );
}

#[test]
fn sync_to_app_removes_disabled_and_orphaned_ssot_symlinks() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_dir = home.join(".cc-switch").join("skills");
    let disabled_skill = ssot_dir.join("disabled-skill");
    let orphan_skill = ssot_dir.join("orphan-skill");
    write_skill(&disabled_skill, "Disabled");
    write_skill(&orphan_skill, "Orphan");

    let opencode_skills_dir = home.join(".config").join("opencode").join("skills");
    fs::create_dir_all(&opencode_skills_dir).expect("create opencode skills dir");
    symlink_dir(&disabled_skill, &opencode_skills_dir.join("disabled-skill"));
    symlink_dir(&orphan_skill, &opencode_skills_dir.join("orphan-skill"));

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:disabled-skill".to_string(),
            name: "Disabled".to_string(),
            description: None,
            directory: "disabled-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps::default(),
            installed_at: 0,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save disabled skill");

    SkillService::sync_to_app(&state.db, &AppType::OpenCode).expect("reconcile skills");

    assert!(
        !opencode_skills_dir.join("disabled-skill").exists(),
        "DB-known disabled skill should be removed from OpenCode live dir"
    );
    assert!(
        !opencode_skills_dir.join("orphan-skill").exists(),
        "orphaned symlink into SSOT should be cleaned up"
    );
}

#[test]
fn uninstall_skill_creates_backup_before_removing_ssot() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("backup-skill");
    write_skill(&ssot_skill_dir, "Backup Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "backup me").expect("write prompt.md");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:backup-skill".to_string(),
            name: "Backup Skill".to_string(),
            description: Some("Back me up before uninstall".to_string()),
            directory: "backup-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                ..Default::default()
            },
            installed_at: 123,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let result = SkillService::uninstall(&state.db, "local:backup-skill").expect("uninstall skill");
    let backup_path = result.backup_path.expect("backup path should be returned");
    let backup_dir = std::path::PathBuf::from(&backup_path);

    assert!(backup_dir.exists(), "backup directory should exist");
    assert!(
        backup_dir.join("skill").join("SKILL.md").exists(),
        "backup should include SKILL.md"
    );
    assert_eq!(
        fs::read_to_string(backup_dir.join("skill").join("prompt.md"))
            .expect("read backed up prompt"),
        "backup me"
    );

    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(backup_dir.join("meta.json")).expect("read backup metadata"),
    )
    .expect("parse backup metadata");
    assert_eq!(metadata["skill"]["directory"], "backup-skill");
    assert_eq!(metadata["skill"]["name"], "Backup Skill");

    assert!(
        !ssot_skill_dir.exists(),
        "SSOT skill directory should be removed after uninstall"
    );
    assert!(
        state
            .db
            .get_installed_skill("local:backup-skill")
            .expect("query skill")
            .is_none(),
        "database row should be deleted after uninstall"
    );
}

#[test]
fn restore_skill_backup_restores_files_to_ssot_and_current_app() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("restore-skill");
    write_skill(&ssot_skill_dir, "Restore Skill");
    fs::write(ssot_skill_dir.join("prompt.md"), "restore me").expect("write prompt.md");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:restore-skill".to_string(),
            name: "Restore Skill".to_string(),
            description: Some("Bring the files back".to_string()),
            directory: "restore-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                ..Default::default()
            },
            installed_at: 456,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let uninstall =
        SkillService::uninstall(&state.db, "local:restore-skill").expect("uninstall skill");
    let backup_id = std::path::Path::new(
        &uninstall
            .backup_path
            .expect("backup path should be returned on uninstall"),
    )
    .file_name()
    .expect("backup dir name")
    .to_string_lossy()
    .to_string();

    let restored = SkillService::restore_from_backup(&state.db, &backup_id, &AppType::Claude)
        .expect("restore from backup");

    assert_eq!(restored.directory, "restore-skill");
    assert!(restored.apps.claude, "restored skill should enable Claude");
    assert!(
        !restored.apps.codex && !restored.apps.gemini && !restored.apps.opencode,
        "restore should only enable the selected app"
    );
    assert!(
        home.join(".cc-switch")
            .join("skills")
            .join("restore-skill")
            .join("prompt.md")
            .exists(),
        "restored skill should exist in SSOT"
    );
    assert!(
        home.join(".claude")
            .join("skills")
            .join("restore-skill")
            .join("prompt.md")
            .exists(),
        "restored skill should sync to the selected app"
    );
    assert!(
        state
            .db
            .get_installed_skill("local:restore-skill")
            .expect("query restored skill")
            .is_some(),
        "restored skill should be written back to the database"
    );
}

#[test]
fn delete_skill_backup_removes_backup_directory() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home
        .join(".cc-switch")
        .join("skills")
        .join("delete-backup-skill");
    write_skill(&ssot_skill_dir, "Delete Backup Skill");

    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&InstalledSkill {
            id: "local:delete-backup-skill".to_string(),
            name: "Delete Backup Skill".to_string(),
            description: Some("Remove my backup".to_string()),
            directory: "delete-backup-skill".to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                claude: true,
                ..Default::default()
            },
            installed_at: 789,
            content_hash: None,
            updated_at: 0,
        })
        .expect("save skill");

    let uninstall =
        SkillService::uninstall(&state.db, "local:delete-backup-skill").expect("uninstall skill");
    let backup_path = uninstall
        .backup_path
        .expect("backup path should be returned on uninstall");
    let backup_id = std::path::Path::new(&backup_path)
        .file_name()
        .expect("backup dir name")
        .to_string_lossy()
        .to_string();

    assert!(
        std::path::Path::new(&backup_path).exists(),
        "backup directory should exist before deletion"
    );

    SkillService::delete_backup(&backup_id).expect("delete backup");

    assert!(
        !std::path::Path::new(&backup_path).exists(),
        "backup directory should be removed"
    );
    assert!(
        SkillService::list_backups()
            .expect("list backups")
            .into_iter()
            .all(|entry| entry.backup_id != backup_id),
        "deleted backup should no longer appear in backup list"
    );
}

#[test]
fn migration_snapshot_overrides_multi_source_directory_inference() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    write_skill(
        &home.join(".claude").join("skills").join("demo-skill"),
        "Demo",
    );
    write_skill(
        &home
            .join(".config")
            .join("opencode")
            .join("skills")
            .join("demo-skill"),
        "Demo",
    );

    let state = create_test_state().expect("create test state");
    state
        .db
        .set_setting(
            "skills_ssot_migration_snapshot",
            r#"[{"directory":"demo-skill","app_type":"claude"}]"#,
        )
        .expect("seed migration snapshot");

    let count = migrate_skills_to_ssot(&state.db).expect("migrate skills to ssot");
    assert_eq!(count, 1, "expected one migrated skill");

    let skills = state.db.get_all_installed_skills().expect("get skills");
    let migrated = skills
        .values()
        .find(|skill| skill.directory == "demo-skill")
        .expect("migrated demo-skill");

    assert!(
        migrated.apps.claude,
        "legacy snapshot should preserve Claude enablement"
    );
    assert!(
        !migrated.apps.opencode,
        "migration should no longer infer OpenCode enablement from a duplicate directory alone"
    );
}

fn skill_row(id: &str, directory: &str) -> InstalledSkill {
    InstalledSkill {
        id: id.to_string(),
        name: "Hostile".to_string(),
        description: None,
        directory: directory.to_string(),
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps {
            claude: true,
            ..Default::default()
        },
        installed_at: 1,
        content_hash: None,
        updated_at: 0,
    }
}

#[test]
fn uninstall_rejects_traversal_directory_from_db_without_touching_fs() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // A directory outside the skills roots that hostile rows point at.
    let victim = home.join("victim");
    write_skill(&victim, "Victim");
    fs::write(victim.join("keep.txt"), "keep").expect("write victim file");
    let ssot_dir = home.join(".cc-switch").join("skills");
    fs::create_dir_all(&ssot_dir).expect("create ssot");
    fs::create_dir_all(home.join(".claude").join("skills")).expect("create app dir");

    let state = create_test_state().expect("create test state");
    let absolute = victim.to_string_lossy().to_string();
    let hostile = [
        ("hostile:dotdot", "../../victim"),
        ("hostile:abs", absolute.as_str()),
        ("hostile:mixed", "a/../../victim"),
        ("hostile:parent", ".."),
        ("hostile:backslash", "..\\..\\victim"),
    ];

    for (id, directory) in hostile {
        state
            .db
            .save_skill(&skill_row(id, directory))
            .expect("save hostile skill row");
        let err = SkillService::uninstall(&state.db, id)
            .expect_err("uninstall must reject a traversal directory");
        assert!(
            err.to_string().contains("INVALID_SKILL_DIRECTORY"),
            "unexpected error for {directory}: {err}"
        );
        assert!(
            state.db.get_installed_skill(id).expect("query").is_some(),
            "row must be kept when uninstall is refused"
        );
        assert!(
            victim.join("keep.txt").exists(),
            "uninstall of {directory} must not delete outside the skills root"
        );

        let err = SkillService::remove_from_app(directory, &AppType::Claude)
            .expect_err("remove_from_app must reject a traversal directory");
        assert!(err.to_string().contains("INVALID_SKILL_DIRECTORY"));
        assert!(victim.join("keep.txt").exists());

        assert!(
            SkillService::sync_to_app_dir(directory, &AppType::Claude).is_err(),
            "sync_to_app_dir must reject {directory}"
        );
    }

    assert!(
        fs::read_dir(&ssot_dir).expect("read ssot").next().is_none(),
        "nothing should be copied into the SSOT for hostile rows"
    );
    let backup_dir = home.join(".cc-switch").join("skill-backups");
    assert!(
        !backup_dir.exists()
            || fs::read_dir(&backup_dir)
                .expect("read backups")
                .next()
                .is_none(),
        "no backup should be taken of a path outside the skills root"
    );

    // Bulk sync skips the hostile rows instead of failing or escaping.
    SkillService::sync_to_app(&state.db, &AppType::Claude).expect("bulk sync skips hostile rows");
    assert!(victim.join("keep.txt").exists());
}

#[test]
fn uninstall_still_removes_valid_skill_after_directory_validation() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("valid-skill");
    write_skill(&ssot_skill_dir, "Valid");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&skill_row("local:valid-skill", "valid-skill"))
        .expect("save skill");
    SkillService::sync_to_app_dir("valid-skill", &AppType::Claude).expect("sync to claude");
    let app_skill = home.join(".claude").join("skills").join("valid-skill");
    assert!(app_skill.join("SKILL.md").exists());

    SkillService::uninstall(&state.db, "local:valid-skill").expect("uninstall valid skill");
    assert!(!ssot_skill_dir.exists());
    assert!(!app_skill.exists() && !app_skill.is_symlink());
}

#[test]
fn restore_from_backup_rejects_traversal_directory_in_metadata() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let ssot_skill_dir = home.join(".cc-switch").join("skills").join("meta-skill");
    write_skill(&ssot_skill_dir, "Meta");
    let state = create_test_state().expect("create test state");
    state
        .db
        .save_skill(&skill_row("local:meta-skill", "meta-skill"))
        .expect("save skill");

    let backup_path = SkillService::uninstall(&state.db, "local:meta-skill")
        .expect("uninstall skill")
        .backup_path
        .expect("backup path");
    let backup_dir = std::path::PathBuf::from(&backup_path);
    let backup_id = backup_dir
        .file_name()
        .expect("backup id")
        .to_string_lossy()
        .to_string();

    // Tamper with the metadata so restore would copy outside the SSOT root.
    let meta_path = backup_dir.join("meta.json");
    let mut metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&meta_path).expect("read meta"))
            .expect("parse meta");
    metadata["skill"]["directory"] = serde_json::Value::String("../../escaped".to_string());
    fs::write(
        &meta_path,
        serde_json::to_string(&metadata).expect("serialize meta"),
    )
    .expect("write meta");

    let err = SkillService::restore_from_backup(&state.db, &backup_id, &AppType::Claude)
        .expect_err("restore must reject a traversal directory");
    assert!(err.to_string().contains("INVALID_SKILL_DIRECTORY"), "{err}");
    assert!(!home.join("escaped").exists());
    assert!(!home.join(".cc-switch").join("escaped").exists());
    assert!(
        state
            .db
            .get_all_installed_skills()
            .expect("list skills")
            .is_empty(),
        "no row should be written"
    );
}
