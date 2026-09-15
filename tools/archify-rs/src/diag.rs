//! Diagnostics and check receipts, shaped like archify's JSON receipts so the
//! same tooling can read either implementation's output.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub evidence: serde_json::Value,
}

impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: code.to_string(),
            severity: Severity::Error,
            message: message.into(),
            evidence: serde_json::Value::Null,
        }
    }

    pub fn warning(code: &str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: code.to_string(),
            severity: Severity::Warning,
            message: message.into(),
            evidence: serde_json::Value::Null,
        }
    }

    pub fn with_evidence(mut self, evidence: serde_json::Value) -> Diagnostic {
        self.evidence = evidence;
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub details: Vec<String>,
}

impl Check {
    pub fn new(name: &str) -> Check {
        Check {
            name: name.to_string(),
            ok: true,
            details: Vec::new(),
        }
    }

    pub fn fail(&mut self, detail: impl Into<String>) {
        self.ok = false;
        self.details.push(detail.into());
    }
}
