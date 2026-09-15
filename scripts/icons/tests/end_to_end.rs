//! End-to-end tests against a throwaway repository layout.

use std::fs;
use std::path::PathBuf;

use cc_switch_icons::{check, extract, filter, ts, Config};

const UPSTREAM_OPENAI: &str = "<svg fill=\"currentColor\" fill-rule=\"evenodd\" height=\"1em\" style=\"flex:none;line-height:1\" viewBox=\"0 0 24 24\" width=\"1em\" xmlns=\"http://www.w3.org/2000/svg\"><title>OpenAI</title><path d=\"M1 1\"/></svg>";
const LOCAL_CUSTOM: &str = "<?xml version=\"1.0\"?>\n<svg width=\"64\" height=\"64\" viewBox=\"0 0 64 64\" xmlns=\"http://www.w3.org/2000/svg\">\n  <circle r=\"1\"/>\n</svg>\n";

const INDEX: &str = "// Auto-generated icon index\n// Do not edit manually\n\nimport _photo from \"./photo.png\";\n\nexport const icons: Record<string, string> = {\n  claude: `<svg><title>Claude</title></svg>`,\n};\n\nexport const iconUrls: Record<string, string> = {\n  photo: _photo,\n};\n\nexport const iconList = [\n  ...Object.keys(icons),\n  ...Object.keys(iconUrls),\n].sort();\n";
const METADATA: &str = "import { IconMetadata } from \"@/types/icon\";\n\nexport const iconMetadata: Record<string, IconMetadata> = {\n  claude: {\n    name: \"claude\",\n    displayName: \"Claude\",\n    category: \"ai-provider\",\n    keywords: [\"anthropic\"],\n    defaultColor: \"#D4915D\",\n  },\n  photo: {\n    name: \"photo\",\n    displayName: \"Photo\",\n    category: \"other\",\n    keywords: [\"photo\"],\n  },\n};\n\nexport function getIconMetadata(name: string): IconMetadata | undefined {\n  return iconMetadata[name.toLowerCase()];\n}\n";

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "cc-switch-icons-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let cfg = Config::new(&root);
        fs::create_dir_all(&cfg.icons_dir).unwrap();
        fs::create_dir_all(&cfg.source_dir).unwrap();
        fs::write(cfg.index_path(), INDEX).unwrap();
        fs::write(cfg.metadata_path(), METADATA).unwrap();
        fs::write(cfg.icons_dir.join("photo.png"), b"png").unwrap();
        fs::write(cfg.icons_dir.join("claude.svg"), "<svg/>").unwrap();
        Sandbox { root }
    }

    fn cfg(&self) -> Config {
        Config::new(&self.root)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn extract_registers_new_icons_without_touching_existing_entries() {
    let sandbox = Sandbox::new("extract");
    let cfg = sandbox.cfg();
    fs::write(cfg.source_dir.join("openai.svg"), UPSTREAM_OPENAI).unwrap();
    fs::write(cfg.icons_dir.join("custom.svg"), LOCAL_CUSTOM).unwrap();
    let big = format!("<svg><image href=\"data:x\"/>{}</svg>", "x".repeat(10));
    fs::write(cfg.icons_dir.join("bigone.svg"), &big).unwrap();

    let names: Vec<String> = ["openai", "custom", "bigone", "claude", "missing"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let outcome = extract::run(&cfg, &names, true).unwrap();

    assert_eq!(outcome.extracted, vec!["openai"]);
    assert_eq!(outcome.kept_local, vec!["custom", "bigone", "claude"]);
    assert_eq!(outcome.not_found, vec!["missing"]);
    assert_eq!(outcome.indexed_inline, vec!["openai", "custom"]);
    assert_eq!(outcome.indexed_url, vec!["bigone"]);
    assert_eq!(outcome.metadata_added, vec!["openai", "custom", "bigone"]);

    let index_src = fs::read_to_string(cfg.index_path()).unwrap();
    let index = ts::parse_index(&index_src);
    assert_eq!(index.inline_keys, vec!["claude", "openai", "custom"]);
    assert_eq!(index.url_keys, vec!["photo", "bigone"]);
    assert!(index_src.contains(
        "import _photo from \"./photo.png\";\nimport _bigone from \"./bigone.svg?url\";\n"
    ));
    assert!(index_src.contains(&format!("  openai: `{UPSTREAM_OPENAI}`,")));
    assert!(index_src.contains("  custom: `<svg width=\"1em\" height=\"1em\" viewBox=\"0 0 64 64\" xmlns=\"http://www.w3.org/2000/svg\" style=\"flex:none;line-height:1\"><title>Custom</title><circle r=\"1\"/></svg>`,"));
    // The pre-existing entry is byte-for-byte untouched.
    assert!(index_src.contains("  claude: `<svg><title>Claude</title></svg>`,"));
    assert!(index_src.ends_with("].sort();\n"));

    let metadata_src = fs::read_to_string(cfg.metadata_path()).unwrap();
    assert_eq!(
        ts::parse_metadata(&metadata_src).keys,
        vec!["claude", "photo", "openai", "custom", "bigone"]
    );
    assert!(metadata_src.contains("    displayName: \"OpenAI\",\n    category: \"ai-provider\",\n    keywords: [\"gpt\", \"chatgpt\"],\n    defaultColor: \"#00A67E\","));
    assert!(metadata_src.contains("  custom: {\n    name: \"custom\",\n    displayName: \"Custom\",\n    category: \"ai-provider\",\n    keywords: [\"custom\"],\n  },"));
    assert!(metadata_src.ends_with("}\n"));

    let readme = fs::read_to_string(cfg.icons_dir.join("README.md")).unwrap();
    assert!(readme.contains("- Extracted from upstream: 1"));
    assert!(readme.contains("- missing"));

    // The result is consistent, and a second run is a no-op.
    let report = check::run(&cfg).unwrap();
    assert!(report.is_ok(), "{:?}", report.errors);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let again = extract::run(&cfg, &names, false).unwrap();
    assert!(again.indexed_inline.is_empty() && again.indexed_url.is_empty());
    assert!(again.metadata_added.is_empty());
    assert_eq!(fs::read_to_string(cfg.index_path()).unwrap(), index_src);
}

#[test]
fn extract_rejects_bad_names() {
    let sandbox = Sandbox::new("names");
    let err = extract::run(&sandbox.cfg(), &["../x".to_string()], false).unwrap_err();
    assert!(err.0.contains("invalid icon name"));
}

#[test]
fn filter_keeps_everything_the_index_references() {
    let sandbox = Sandbox::new("filter");
    let cfg = sandbox.cfg();
    fs::write(cfg.icons_dir.join("claude-color.svg"), "<svg>color</svg>").unwrap();
    fs::write(cfg.icons_dir.join("unused.svg"), "<svg/>").unwrap();
    fs::write(cfg.icons_dir.join("vercel.svg"), "<svg/>").unwrap();
    fs::write(cfg.icons_dir.join("extra.svg"), "<svg/>").unwrap();

    let plan = filter::build_plan(&cfg, &["extra".to_string()]).unwrap();
    assert_eq!(plan.delete, vec!["unused.svg"]);
    assert_eq!(
        plan.rename,
        vec![("claude-color.svg".to_string(), "claude.svg".to_string())]
    );
    assert!(
        plan.kept.contains("claude") && plan.kept.contains("vercel") && plan.kept.contains("extra")
    );

    filter::apply(&cfg.icons_dir, &plan).unwrap();
    assert!(!cfg.icons_dir.join("unused.svg").exists());
    assert!(!cfg.icons_dir.join("claude-color.svg").exists());
    assert_eq!(
        fs::read_to_string(cfg.icons_dir.join("claude.svg")).unwrap(),
        "<svg>color</svg>"
    );
    assert!(cfg.icons_dir.join("photo.png").exists());
}

#[test]
fn check_reports_missing_import_target() {
    let sandbox = Sandbox::new("check");
    let cfg = sandbox.cfg();
    fs::remove_file(cfg.icons_dir.join("photo.png")).unwrap();
    let report = check::run(&cfg).unwrap();
    assert!(!report.is_ok());
    assert!(report.errors[0].contains("missing file photo.png"));
}
