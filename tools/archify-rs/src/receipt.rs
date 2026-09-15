//! Delivery receipts: SHA-256 and byte counts for the exact specification
//! bytes and the exact artifact bytes that were written.

use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Digested {
    pub sha256: String,
    pub bytes: usize,
}

impl Digested {
    pub fn of(bytes: &[u8]) -> Digested {
        Digested {
            sha256: sha256_hex(bytes),
            bytes: bytes.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationSummary {
    pub checks_passed: usize,
    pub check_count: usize,
    pub composition_profile: String,
    pub composition_status: String,
    pub errors: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    pub schema_version: u32,
    pub ok: bool,
    pub command: String,
    #[serde(rename = "type")]
    pub diagram_type: String,
    pub input: String,
    pub output: String,
    pub specification: Digested,
    pub artifact: Digested,
    pub validation: ValidationSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(Digested::of(b"abc").bytes, 3);
    }
}
