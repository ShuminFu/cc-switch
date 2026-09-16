//! Port of `src/lib/version.ts`.
//!
//! Lightweight semver comparison used to decide whether a tool update is
//! available. A plain string `!=` would misreport a locally installed
//! prerelease / `next`-channel build (which is *ahead* of `latest`) as
//! "needs update", so the comparison is a real partial order.

use std::cmp::Ordering;
use std::sync::LazyLock;

use regex::Regex;

/// Port of the internal `ParsedVersion` (`version.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVersion {
    /// `[major, minor, patch]`
    core: [u64; 3],
    /// Prerelease identifiers, e.g. `"2.1.156-beta.1"` → `["beta", "1"]`; empty for releases.
    pre: Vec<String>,
}

/// `^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?` — a *prefix* match on the
/// trimmed input, so trailing junk (including `+build`) is ignored.
static VERSION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]+)\.([0-9]+)\.([0-9]+)(?:-([0-9A-Za-z.-]+))?").expect("valid regex")
});

/// Port of `parseVersion` (`version.ts`). Returns `None` when unparsable.
///
/// The TS implementation converts with `Number(...)`, which never fails; here a
/// component that does not fit in `u64` is treated as unparsable.
fn parse_version(v: &str) -> Option<ParsedVersion> {
    let caps = VERSION_PATTERN.captures(v.trim())?;
    let core = [
        caps.get(1)?.as_str().parse::<u64>().ok()?,
        caps.get(2)?.as_str().parse::<u64>().ok()?,
        caps.get(3)?.as_str().parse::<u64>().ok()?,
    ];
    let pre = match caps.get(4) {
        Some(m) if !m.as_str().is_empty() => m.as_str().split('.').map(str::to_string).collect(),
        _ => Vec::new(),
    };
    Some(ParsedVersion { core, pre })
}

fn is_numeric_segment(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit())
}

/// Compare a numeric prerelease segment; oversized values fall back to a
/// length-then-lexical comparison (equivalent for canonical decimal strings).
fn compare_numeric_segments(a: &str, b: &str) -> Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => {
            let a = a.trim_start_matches('0');
            let b = b.trim_start_matches('0');
            a.len().cmp(&b.len()).then_with(|| a.cmp(b))
        }
    }
}

/// Port of `comparePre` (`version.ts`): semver prerelease precedence.
///
/// - both empty → equal
/// - a version *with* a prerelease is lower than one without
/// - otherwise segment-wise: numeric vs numeric by value, numeric < non-numeric,
///   non-numeric by ASCII; an equal prefix means more segments wins.
fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    if a.is_empty() && b.is_empty() {
        return Ordering::Equal;
    }
    if a.is_empty() {
        return Ordering::Greater;
    }
    if b.is_empty() {
        return Ordering::Less;
    }
    for (ai, bi) in a.iter().zip(b.iter()) {
        let a_num = is_numeric_segment(ai);
        let b_num = is_numeric_segment(bi);
        if a_num && b_num {
            let ordering = compare_numeric_segments(ai, bi);
            if ordering != Ordering::Equal {
                return ordering;
            }
        } else if a_num {
            return Ordering::Less;
        } else if b_num {
            return Ordering::Greater;
        } else if ai != bi {
            return ai.cmp(bi);
        }
    }
    a.len().cmp(&b.len())
}

/// Port of `compareVersions` (`version.ts`).
///
/// `Greater` means `a` is newer than `b`, `Less` means older, `Equal` means equal
/// **or undecidable**: if either side cannot be parsed the result is `Equal`
/// (conservative — never triggers an update prompt).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (Some(pa), Some(pb)) = (parse_version(a), parse_version(b)) else {
        return Ordering::Equal;
    };
    for (x, y) in pa.core.iter().zip(pb.core.iter()) {
        let ordering = x.cmp(y);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    compare_pre(&pa.pre, &pb.pre)
}

/// Port of `isUpdateAvailable` (`version.ts`).
///
/// `true` only when `latest` is strictly newer than `current`. A missing or
/// empty version on either side yields `false`, and so does a local build that
/// is *ahead* of `latest` (prerelease / `next` channel).
pub fn is_update_available(current: Option<&str>, latest: Option<&str>) -> bool {
    match (current, latest) {
        (Some(current), Some(latest)) if !current.is_empty() && !latest.is_empty() => {
            compare_versions(latest, current) == Ordering::Greater
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_core_segments() {
        assert_eq!(compare_versions("2.1.156", "2.1.154"), Ordering::Greater);
        assert_eq!(compare_versions("2.1.154", "2.1.156"), Ordering::Less);
        assert_eq!(compare_versions("2.2.0", "2.1.999"), Ordering::Greater);
        assert_eq!(compare_versions("3.0.0", "2.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("2.1.156", "2.1.156"), Ordering::Equal);
    }

    #[test]
    fn prerelease_is_lower_than_release() {
        assert_eq!(
            compare_versions("2.1.156-beta.1", "2.1.156"),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("2.1.156", "2.1.156-rc.1"),
            Ordering::Greater
        );
    }

    #[test]
    fn prerelease_segments_follow_semver() {
        assert_eq!(
            compare_versions("1.0.0-beta.2", "1.0.0-beta.11"),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("1.0.0-alpha", "1.0.0-beta"),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("1.0.0-beta", "1.0.0-beta.1"),
            Ordering::Less
        );
    }

    #[test]
    fn unparsable_is_conservatively_equal() {
        assert_eq!(compare_versions("", "2.1.154"), Ordering::Equal);
        assert_eq!(compare_versions("unknown", "2.1.154"), Ordering::Equal);
    }

    #[test]
    fn prefix_match_ignores_trailing_junk_and_trims() {
        assert_eq!(compare_versions(" 2.1.156 ", "2.1.156"), Ordering::Equal);
        assert_eq!(
            compare_versions("2.1.156+build.7", "2.1.156"),
            Ordering::Equal
        );
        assert_eq!(compare_versions("v2.1.156", "2.1.156"), Ordering::Equal);
        assert_eq!(
            compare_versions("0.1.2505172116", "0.1.2505172115"),
            Ordering::Greater
        );
    }

    #[test]
    fn numeric_prerelease_segment_beats_none_and_loses_to_alpha() {
        assert_eq!(compare_versions("1.0.0-1", "1.0.0-alpha"), Ordering::Less);
        assert_eq!(
            compare_versions("1.0.0-alpha.1", "1.0.0-alpha"),
            Ordering::Greater
        );
    }

    #[test]
    fn update_available_only_when_latest_is_strictly_newer() {
        assert!(is_update_available(Some("2.1.154"), Some("2.1.156")));
    }

    #[test]
    fn local_next_channel_ahead_of_latest_is_not_an_update() {
        assert!(!is_update_available(Some("2.1.156"), Some("2.1.154")));
    }

    #[test]
    fn equal_versions_are_not_an_update() {
        assert!(!is_update_available(Some("2.1.156"), Some("2.1.156")));
    }

    #[test]
    fn missing_versions_are_not_an_update() {
        assert!(!is_update_available(None, Some("2.1.156")));
        assert!(!is_update_available(Some("2.1.156"), None));
        assert!(!is_update_available(Some(""), Some("")));
    }
}
