//! Argument validation at the IPC boundary (CMD-23).
//!
//! Commands that make the backend send HTTP requests to a webview-supplied
//! URL, or read a webview-supplied file, check the argument here before
//! handing it to a service.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use url::{Host, Url};

/// Hosts of cloud instance-metadata services.
const METADATA_HOSTS: &[&str] = &["metadata.google.internal", "metadata.goog", "metadata"];

fn is_forbidden_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_link_local() // 169.254.0.0/16, incl. the 169.254.169.254 metadata service
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
}

fn is_forbidden_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_forbidden_ipv4(v4);
    }
    let first = ip.segments()[0];
    ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
        || ip == Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254) // AWS IMDS over IPv6
}

/// Check a URL the backend is asked to fetch: http(s) only, and never a
/// link-local, unspecified, multicast or cloud-metadata address.
///
/// Loopback and private networks stay allowed on purpose: local model
/// servers (Ollama, LM Studio, LiteLLM) and in-house gateways are common
/// provider endpoints.
pub(crate) fn validate_outbound_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim();
    let url = Url::parse(trimmed).map_err(|e| format!("Invalid URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "Unsupported URL scheme '{}': only http and https are allowed",
            url.scheme()
        ));
    }
    let forbidden = match url.host() {
        None => return Err("URL has no host".to_string()),
        Some(Host::Ipv4(ip)) => is_forbidden_ipv4(ip),
        Some(Host::Ipv6(ip)) => is_forbidden_ipv6(ip),
        Some(Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            METADATA_HOSTS.contains(&domain.as_str())
                || domain
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| matches!(ip, IpAddr::V4(v4) if is_forbidden_ipv4(v4)))
        }
    };
    if forbidden {
        return Err(format!(
            "Refusing to send a request to {}: link-local and metadata addresses are not allowed",
            url.host_str().unwrap_or_default()
        ));
    }
    Ok(url)
}

/// [`validate_outbound_url`] for optional or possibly empty arguments.
pub(crate) fn validate_optional_outbound_url(raw: Option<&str>) -> Result<(), String> {
    match raw.map(str::trim) {
        Some(value) if !value.is_empty() => validate_outbound_url(value).map(|_| ()),
        _ => Ok(()),
    }
}

/// Check a skill archive path from the file dialog: an existing regular
/// file with a `.zip` or `.skill` extension.
pub(crate) fn validate_skill_archive_path(path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(extension.as_deref(), Some("zip" | "skill")) {
        return Err(format!(
            "Not a skill archive (expected .zip or .skill): {}",
            path.display()
        ));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("Not a file: {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_url_allows_public_and_local_endpoints() {
        for url in [
            "https://api.deepseek.com/v1",
            "http://127.0.0.1:11434",
            "http://localhost:1234/v1",
            "http://192.168.1.10:4000",
            "http://[::1]:8080",
        ] {
            assert!(validate_outbound_url(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn outbound_url_rejects_schemes_and_metadata_addresses() {
        for url in [
            "file:///etc/passwd",
            "ftp://example.com",
            "javascript:alert(1)",
            "http://169.254.169.254/latest/meta-data",
            "http://[fe80::1]/",
            "http://[::ffff:169.254.169.254]/",
            "http://[fd00:ec2::254]/",
            "http://0.0.0.0:8080",
            "http://metadata.google.internal/computeMetadata/v1",
            "http://METADATA.GOOGLE.INTERNAL./",
            "not a url",
        ] {
            assert!(validate_outbound_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn optional_url_skips_empty_values() {
        assert!(validate_optional_outbound_url(None).is_ok());
        assert!(validate_optional_outbound_url(Some("  ")).is_ok());
        assert!(validate_optional_outbound_url(Some("http://169.254.1.1")).is_err());
    }

    #[test]
    fn skill_archive_path_requires_zip_or_skill_file() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("a.ZIP");
        std::fs::write(&zip, b"PK").unwrap();
        assert!(validate_skill_archive_path(&zip).is_ok());
        let txt = dir.path().join("a.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(validate_skill_archive_path(&txt).is_err());
        assert!(validate_skill_archive_path(&dir.path().join("missing.zip")).is_err());
        let folder = dir.path().join("dir.zip");
        std::fs::create_dir(&folder).unwrap();
        assert!(validate_skill_archive_path(&folder).is_err());
    }
}
