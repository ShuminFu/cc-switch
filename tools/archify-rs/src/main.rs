use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use archify_rs::diag::Severity;
use archify_rs::receipt::{DeliveryReceipt, Digested, ValidationSummary};
use archify_rs::spec::{DiagramType, Spec};
use archify_rs::validate::{self, Quality, Validation};
use archify_rs::{layout, render, scene::Scene};

/// Validate typed-JSON diagram specifications and render them as
/// self-contained HTML/SVG.
#[derive(Parser)]
#[command(name = "archify-rs", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Target {
    /// Diagram type: architecture, sequence, workflow, or auto
    #[arg(value_name = "TYPE")]
    kind: String,
    /// Path to the JSON specification
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Composition profile: standard or showcase
    #[arg(long, default_value = "showcase")]
    quality: String,
    /// Emit a machine-readable JSON receipt
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Check a specification's structure and geometry without writing output
    Validate(Target),
    /// Render HTML even when validation fails (diagnostics still print)
    Render {
        #[command(flatten)]
        target: Target,
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,
    },
    /// Final acceptance: validate, render, write atomically, report SHA-256
    Deliver {
        #[command(flatten)]
        target: Target,
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,
    },
    /// Write the bundled example specifications and their rendered HTML
    Demo {
        #[arg(value_name = "DIRECTORY")]
        dir: PathBuf,
    },
    /// Report what this build can do
    Doctor,
}

struct Compiled {
    kind: DiagramType,
    spec: Spec,
    scene: Option<Scene>,
    validation: Validation,
    input_bytes: Vec<u8>,
}

fn compile(target: &Target) -> Result<Compiled, String> {
    let quality = Quality::parse(&target.quality).ok_or_else(|| {
        format!(
            "unknown --quality \"{}\" (use standard or showcase)",
            target.quality
        )
    })?;
    let input_bytes = fs::read(&target.input)
        .map_err(|e| format!("cannot read {}: {e}", target.input.display()))?;
    let kind = if target.kind == "auto" {
        let probe: serde_json::Value =
            serde_json::from_slice(&input_bytes).map_err(|e| format!("invalid JSON: {e}"))?;
        let declared = probe
            .get("diagram_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        DiagramType::parse(declared)
            .ok_or_else(|| format!("unsupported diagram_type \"{declared}\""))?
    } else {
        DiagramType::parse(&target.kind)
            .ok_or_else(|| format!("unknown diagram type \"{}\"", target.kind))?
    };
    let spec = Spec::from_json(kind, &input_bytes)?;
    let (scene, validation) = match layout::build(&spec) {
        Ok(scene) => {
            let v = validate::validate(&scene, quality);
            (Some(scene), v)
        }
        Err(diags) => (None, Validation::failed_layout(quality, diags)),
    };
    Ok(Compiled {
        kind,
        spec,
        scene,
        validation,
        input_bytes,
    })
}

fn print_validation(c: &Compiled, target: &Target, command: &str) {
    if target.json {
        let v = &c.validation;
        let receipt = serde_json::json!({
            "schemaVersion": 1,
            "ok": v.ok,
            "command": command,
            "type": c.kind.name(),
            "input": target.input.display().to_string(),
            "checks": v.checks,
            "composition": {
                "profile": v.profile.name(),
                "status": if v.ok { "pass" } else { "fail" },
                "summary": { "errors": v.errors(), "warnings": v.warnings() }
            },
            "diagnostics": v.diagnostics,
        });
        println!("{}", serde_json::to_string_pretty(&receipt).unwrap());
        return;
    }
    let v = &c.validation;
    println!(
        "{} {} — {} ({} profile)",
        if v.ok { "PASS" } else { "FAIL" },
        c.kind,
        target.input.display(),
        v.profile.name()
    );
    for check in &v.checks {
        println!("  [{}] {}", if check.ok { "ok" } else { "!!" }, check.name);
    }
    for d in &v.diagnostics {
        let tag = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        println!("  {tag} [{}] {}", d.code, d.message);
    }
    println!(
        "  {} of {} checks passed, {} errors, {} warnings",
        v.checks_passed(),
        v.checks.len(),
        v.errors(),
        v.warnings()
    );
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot move {} into place: {e}", tmp.display())
    })
}

fn run() -> Result<bool, String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate(target) => {
            let c = compile(&target)?;
            print_validation(&c, &target, "validate");
            Ok(c.validation.ok)
        }
        Command::Render { target, output } => {
            let c = compile(&target)?;
            print_validation(&c, &target, "render");
            if let Some(scene) = &c.scene {
                let html = render::render_html(&c.spec, scene);
                write_atomic(&output, html.as_bytes())?;
                if !target.json {
                    println!("  wrote {} ({} bytes)", output.display(), html.len());
                }
            } else {
                eprintln!("  nothing rendered: the layout failed");
            }
            Ok(c.validation.ok)
        }
        Command::Deliver { target, output } => {
            let c = compile(&target)?;
            if !c.validation.ok || c.scene.is_none() {
                print_validation(&c, &target, "deliver");
                if !target.json {
                    eprintln!(
                        "delivery refused; any previous output at {} is untouched",
                        output.display()
                    );
                }
                return Ok(false);
            }
            let html = render::render_html(&c.spec, c.scene.as_ref().unwrap());
            write_atomic(&output, html.as_bytes())?;
            let v = &c.validation;
            let receipt = DeliveryReceipt {
                schema_version: 1,
                ok: true,
                command: "deliver".into(),
                diagram_type: c.kind.name().into(),
                input: target.input.display().to_string(),
                output: output.display().to_string(),
                specification: Digested::of(&c.input_bytes),
                artifact: Digested::of(html.as_bytes()),
                validation: ValidationSummary {
                    checks_passed: v.checks_passed(),
                    check_count: v.checks.len(),
                    composition_profile: v.profile.name().into(),
                    composition_status: "pass".into(),
                    errors: v.errors(),
                    warnings: v.warnings(),
                },
            };
            if target.json {
                println!("{}", serde_json::to_string_pretty(&receipt).unwrap());
            } else {
                println!(
                    "delivered {} -> {}",
                    target.input.display(),
                    output.display()
                );
                println!(
                    "  spec     sha256 {} ({} bytes)",
                    receipt.specification.sha256, receipt.specification.bytes
                );
                println!(
                    "  artifact sha256 {} ({} bytes)",
                    receipt.artifact.sha256, receipt.artifact.bytes
                );
                println!(
                    "  {}/{} checks, {} profile pass, {} errors, {} warnings",
                    receipt.validation.checks_passed,
                    receipt.validation.check_count,
                    receipt.validation.composition_profile,
                    receipt.validation.errors,
                    receipt.validation.warnings
                );
            }
            Ok(true)
        }
        Command::Demo { dir } => {
            fs::create_dir_all(&dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            let mut all_ok = true;
            for (name, kind, body) in EXAMPLES {
                let spec_path = dir.join(name);
                write_atomic(&spec_path, body.as_bytes())?;
                let target = Target {
                    kind: kind.to_string(),
                    input: spec_path.clone(),
                    quality: "showcase".into(),
                    json: false,
                };
                let c = compile(&target)?;
                let html_path = dir.join(name.replace(".json", ".html"));
                match (&c.scene, c.validation.ok) {
                    (Some(scene), true) => {
                        write_atomic(&html_path, render::render_html(&c.spec, scene).as_bytes())?;
                        println!("ok   {} -> {}", spec_path.display(), html_path.display());
                    }
                    _ => {
                        all_ok = false;
                        print_validation(&c, &target, "demo");
                    }
                }
            }
            Ok(all_ok)
        }
        Command::Doctor => {
            println!("{}", render::GENERATOR);
            println!("[ok] diagram types: architecture, sequence, workflow");
            println!("[ok] profiles: standard, showcase (adds desktop readability)");
            println!("[ok] checks: references, finite_svg, orthogonal_arrows, node_overlap, label_fit, relationship_crossings, relationship_corridors, label_route_clearance, viewbox_containment, timeline, desktop_readability");
            println!("[ok] bundled examples: {}", EXAMPLES.len());
            println!("archify-rs is ready.");
            Ok(true)
        }
    }
}

const EXAMPLES: &[(&str, &str, &str)] = &[
    (
        "upstream-review.architecture.json",
        "architecture",
        include_str!("../tests/fixtures/upstream-review.architecture.json"),
    ),
    (
        "proxy-bridge.sequence.json",
        "sequence",
        include_str!("../tests/fixtures/proxy-bridge.sequence.json"),
    ),
    (
        "catchup.workflow.json",
        "workflow",
        include_str!("../tests/fixtures/catchup.workflow.json"),
    ),
];

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
