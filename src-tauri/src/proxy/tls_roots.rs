//! One trust-store recipe for every upstream HTTP stack (PROXY-40).
//!
//! The pooled reqwest client, the hyper-util fallback client and the raw
//! header-case path all trust:
//!
//! 1. the Mozilla/webpki roots (baseline),
//! 2. the OS certificate store (corporate and TLS-intercepting proxy CAs such
//!    as Clash or mitmproxy, once installed in the system store), and
//! 3. optional extra CA certificates from a PEM file: the path in the
//!    `CC_SWITCH_EXTRA_CA_FILE` environment variable, or else
//!    `~/.cc-switch/extra-ca.pem` when it exists.
//!
//! reqwest gets (1) and (2) from its `rustls-tls` + `rustls-tls-native-roots`
//! features and (3) through [`extra_ca_certs`]; the hyper paths build their
//! store with [`root_store`]. Both read the extra certificates from the same
//! function, so the stacks cannot drift apart again. Changes to the file are
//! picked up on the next app start.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;

/// Environment variable naming an extra CA bundle (PEM).
pub const EXTRA_CA_ENV: &str = "CC_SWITCH_EXTRA_CA_FILE";
/// File name of the extra CA bundle in the app config directory.
pub const EXTRA_CA_FILE_NAME: &str = "extra-ca.pem";

/// Where the extra CA bundle is read from, if anywhere.
pub fn extra_ca_path() -> Option<PathBuf> {
    resolve_extra_ca_path(
        std::env::var_os(EXTRA_CA_ENV),
        &crate::config::get_app_config_dir(),
    )
}

fn resolve_extra_ca_path(env: Option<std::ffi::OsString>, config_dir: &Path) -> Option<PathBuf> {
    if let Some(path) = env.filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let default = config_dir.join(EXTRA_CA_FILE_NAME);
    default.exists().then_some(default)
}

/// Parse every certificate in a PEM bundle. Unparsable entries are skipped
/// with a warning (one bad entry must not drop the others).
pub fn parse_pem_certs(pem: &[u8], source: &Path) -> Vec<CertificateDer<'static>> {
    let mut certs = Vec::new();
    for item in CertificateDer::pem_slice_iter(pem) {
        match item {
            Ok(cert) => certs.push(cert),
            Err(e) => log::warn!(
                "[TLS] skipping unparsable certificate in {}: {e}",
                source.display()
            ),
        }
    }
    certs
}

fn load_extra_ca_certs() -> Vec<CertificateDer<'static>> {
    let Some(path) = extra_ca_path() else {
        return Vec::new();
    };
    match std::fs::read(&path) {
        Ok(pem) => {
            let certs = parse_pem_certs(&pem, &path);
            log::info!(
                "[TLS] trusting {} extra CA certificate(s) from {}",
                certs.len(),
                path.display()
            );
            certs
        }
        Err(e) => {
            log::warn!("[TLS] cannot read extra CA file {}: {e}", path.display());
            Vec::new()
        }
    }
}

/// Extra CA certificates shared by every client (loaded once per process).
pub fn extra_ca_certs() -> &'static [CertificateDer<'static>] {
    static CERTS: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();
    CERTS.get_or_init(load_extra_ca_certs)
}

/// Root store for the hyper paths: webpki + native + `extra`.
pub fn root_store_with(extra: &[CertificateDer<'static>]) -> rustls::RootCertStore {
    let mut store = rustls::RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let native = rustls_native_certs::load_native_certs();
    let (native_added, _errors) = store.add_parsable_certificates(native.certs);
    let (extra_added, extra_rejected) = store.add_parsable_certificates(extra.iter().cloned());
    if extra_rejected > 0 {
        log::warn!("[TLS] {extra_rejected} extra CA certificate(s) rejected");
    }
    log::debug!("[TLS] root store: webpki + {native_added} native + {extra_added} extra certs");
    store
}

/// Root store for the hyper paths with the configured extra CAs.
pub fn root_store() -> rustls::RootCertStore {
    root_store_with(extra_ca_certs())
}

/// Add the extra CAs to a reqwest builder (it already has webpki + native).
pub fn apply_to_reqwest(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    add_certs_to_reqwest(builder, extra_ca_certs())
}

fn add_certs_to_reqwest(
    mut builder: reqwest::ClientBuilder,
    certs: &[CertificateDer<'static>],
) -> reqwest::ClientBuilder {
    for cert in certs {
        match reqwest::Certificate::from_der(cert.as_ref()) {
            Ok(cert) => builder = builder.add_root_certificate(cert),
            Err(e) => log::warn!("[TLS] reqwest rejected an extra CA certificate: {e}"),
        }
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-signed test CA (generated once with openssl, CN=cc-switch test CA).
    const TEST_CA_PEM: &str = include_str!("../../tests/fixtures/proxy/test-ca.pem");

    #[test]
    fn parses_every_certificate_in_a_bundle_and_skips_garbage() {
        let bundle = format!("{TEST_CA_PEM}\n# comment\n{TEST_CA_PEM}");
        let certs = parse_pem_certs(bundle.as_bytes(), Path::new("bundle.pem"));
        assert_eq!(certs.len(), 2);
        let broken = "-----BEGIN CERTIFICATE-----\nnot base64!\n-----END CERTIFICATE-----\n";
        assert!(parse_pem_certs(broken.as_bytes(), Path::new("broken.pem")).is_empty());
    }

    #[test]
    fn hyper_root_store_includes_the_extra_ca() {
        let extra = parse_pem_certs(TEST_CA_PEM.as_bytes(), Path::new("test-ca.pem"));
        let base = root_store_with(&[]).len();
        let with_extra = root_store_with(&extra).len();
        assert_eq!(with_extra, base + 1);
    }

    #[test]
    fn reqwest_accepts_the_extra_ca() {
        let extra = parse_pem_certs(TEST_CA_PEM.as_bytes(), Path::new("test-ca.pem"));
        let client = add_certs_to_reqwest(reqwest::Client::builder(), &extra).build();
        assert!(client.is_ok());
    }

    #[test]
    fn bundle_path_comes_from_env_or_the_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(resolve_extra_ca_path(None, dir.path()), None);
        assert_eq!(resolve_extra_ca_path(Some("".into()), dir.path()), None);
        let default = dir.path().join(EXTRA_CA_FILE_NAME);
        std::fs::write(&default, TEST_CA_PEM).unwrap();
        assert_eq!(resolve_extra_ca_path(None, dir.path()), Some(default));
        assert_eq!(
            resolve_extra_ca_path(Some("/etc/corp.pem".into()), dir.path()),
            Some(PathBuf::from("/etc/corp.pem")),
            "the environment variable wins"
        );
    }

    #[test]
    fn every_client_path_uses_the_shared_recipe() {
        // The hyper TLS config and the reqwest builder must both go through
        // this module (PROXY-40: the stacks used to trust different roots).
        let hyper = include_str!("hyper_client.rs");
        assert!(hyper.contains("tls_roots::root_store()"));
        let http = include_str!("http_client.rs");
        assert!(http.contains("tls_roots::apply_to_reqwest("));
    }
}
