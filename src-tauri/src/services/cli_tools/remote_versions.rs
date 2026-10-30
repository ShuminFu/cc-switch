//! 远端最新版本查询（npm dist-tags / GitHub releases / PyPI）与 semver 比较。

/// 该工具在 npm 上的预发布通道 tag(靠前者优先)。仅当本地版本已**严格领先**
/// `latest` 时才会被补查 —— 让主动在抢先通道的用户(如走 Claude Code 的 `next`)
/// 看到与所在通道对齐的"最新版本",同时绝不把稳定通道用户暴露给预发布版。
/// 返回空切片表示该工具只看 `latest`、不补查。
///
/// 为何不通用覆盖所有工具:各家预发布 tag 命名互不统一(codex=alpha/beta/native、
/// gemini=nightly/preview、openclaw=alpha/beta),且 codex 的 beta/native 是
/// `0.1.x` 时间戳式版本、gemini 有误发的 `false` tag —— 这些脏值虽会被
/// `pick_latest_version` 的版本比较挡掉,但维护成本与误报风险不值当,故暂只为
/// Claude Code 启用。
pub(super) fn npm_prerelease_tags(tool: &str) -> &'static [&'static str] {
    match tool {
        "claude" => &["next"],
        _ => &[],
    }
}

/// 解析 "2.1.156" / "2.1.156-beta.1" → (主版本三段, 预发布段)。无法解析返回 None。
/// 与前端 `src/lib/version.ts` 的 parseVersion 语义对称(跨语言各实现一份)。
/// patch 用 u64 以容纳 codex 的 `0.1.2505172116` 时间戳式版本而不溢出。
pub(super) fn parse_semver(v: &str) -> Option<([u64; 3], Vec<String>)> {
    // 忽略 `+build` 元数据,再以首个 `-` 切出预发布段。
    let core_and_pre = v.trim().split('+').next().unwrap_or("");
    let (core, pre) = match core_and_pre.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (core_and_pre, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None; // 多于三段,非法
    }
    let pre_segments = pre
        .map(|p| p.split('.').map(|s| s.to_string()).collect())
        .unwrap_or_default();
    Some(([major, minor, patch], pre_segments))
}

/// 比较两个版本号(遵循 semver:主版本三段优先;core 相等时有预发布 < 无预发布;
/// 预发布段逐段比 —— 数字段按数值、数字段 < 非数字段、非数字段按 ASCII、前缀相同
/// 则段更多者更大)。任一无法解析返回 None,调用方据此保守处理。
pub(super) fn compare_semver(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let (ac, ap) = parse_semver(a)?;
    let (bc, bp) = parse_semver(b)?;
    for i in 0..3 {
        match ac[i].cmp(&bc[i]) {
            Ordering::Equal => continue,
            other => return Some(other),
        }
    }
    match (ap.is_empty(), bp.is_empty()) {
        (true, true) => return Some(Ordering::Equal),
        (true, false) => return Some(Ordering::Greater),
        (false, true) => return Some(Ordering::Less),
        (false, false) => {}
    }
    for (x, y) in ap.iter().zip(bp.iter()) {
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(xv), Ok(yv)) => xv.cmp(&yv),
            (Ok(_), Err(_)) => Ordering::Less, // 数字段 < 非数字段
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.as_str().cmp(y.as_str()),
        };
        if ord != Ordering::Equal {
            return Some(ord);
        }
    }
    Some(ap.len().cmp(&bp.len()))
}

/// 从一次 registry 请求得到的完整 dist-tags 出发,挑选要展示的"最新版本"。
///
/// 规则:默认就是 `latest`;仅当本地版本已**严格领先** `latest`(说明用户主动在
/// 抢先通道)时,才把 `prerelease_tags` 指向的版本纳入比较,取其中能被解析、且
/// 高于 `latest` 的最高者。无法解析或不高于 latest 的脏 tag 一律落选。
pub(super) fn pick_latest_version(
    dist_tags: &serde_json::Map<String, serde_json::Value>,
    prerelease_tags: &[&str],
    local_version: Option<&str>,
) -> Option<String> {
    use std::cmp::Ordering;
    let latest = dist_tags.get("latest").and_then(|v| v.as_str())?;

    // 本地是否严格领先 latest;任一无法解析则按"未领先"保守处理(只看 latest)。
    let local_ahead = local_version
        .and_then(|local| compare_semver(local, latest))
        .map(|ord| ord == Ordering::Greater)
        .unwrap_or(false);
    if prerelease_tags.is_empty() || !local_ahead {
        return Some(latest.to_string());
    }

    let mut best = latest.to_string();
    for tag in prerelease_tags {
        if let Some(candidate) = dist_tags.get(*tag).and_then(|v| v.as_str()) {
            if compare_semver(candidate, &best) == Some(Ordering::Greater) {
                best = candidate.to_string();
            }
        }
    }
    Some(best)
}

/// 查询某工具要展示的"最新版本"。npm 工具在本地领先 latest 时会按预发布通道补查（见
/// `fetch_npm_latest_for_tool` / `npm_prerelease_tags`）；OpenCode 在 npm 查询失败时回退
/// GitHub releases，Hermes 查 PyPI。
pub(super) async fn fetch_latest_version(
    client: &reqwest::Client,
    tool: &str,
    local: Option<&str>,
) -> Option<String> {
    match tool {
        "claude" => {
            fetch_npm_latest_for_tool(client, "@anthropic-ai/claude-code", tool, local).await
        }
        "codex" => fetch_npm_latest_for_tool(client, "@openai/codex", tool, local).await,
        "gemini" => fetch_npm_latest_for_tool(client, "@google/gemini-cli", tool, local).await,
        "grok" => fetch_npm_latest_for_tool(client, "@xai-official/grok", tool, local).await,
        "opencode" => {
            if let Some(version) =
                fetch_npm_latest_for_tool(client, "opencode-ai", tool, local).await
            {
                Some(version)
            } else {
                fetch_github_latest_version(client, "anomalyco/opencode").await
            }
        }
        "openclaw" => fetch_npm_latest_for_tool(client, "openclaw", tool, local).await,
        "hermes" => fetch_pypi_latest_version(client, "hermes-agent").await,
        _ => None,
    }
}

/// 拉取 npm 包的完整 dist-tags(单次请求即含 latest/next/beta/...)。
pub(super) async fn fetch_npm_dist_tags(
    client: &reqwest::Client,
    package: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let url = format!("https://registry.npmjs.org/{package}");
    let resp = client.get(&url).send().await.ok()?;
    let json = resp.json::<serde_json::Value>().await.ok()?;
    json.get("dist-tags")?.as_object().cloned()
}

/// 查询某 npm 工具要展示的"最新版本":取 `latest`,并在本地版本领先时按工具的
/// 预发布通道(见 `npm_prerelease_tags`)补查 —— 复用同一次 registry 响应,无额外请求。
pub(super) async fn fetch_npm_latest_for_tool(
    client: &reqwest::Client,
    package: &str,
    tool: &str,
    local_version: Option<&str>,
) -> Option<String> {
    let dist_tags = fetch_npm_dist_tags(client, package).await?;
    pick_latest_version(&dist_tags, npm_prerelease_tags(tool), local_version)
}

/// Helper function to fetch latest version from GitHub releases
pub(super) async fn fetch_github_latest_version(
    client: &reqwest::Client,
    repo: &str,
) -> Option<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    match client
        .get(&url)
        .header("User-Agent", "cc-switch")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("tag_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.strip_prefix('v').unwrap_or(s).to_string())
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// Helper function to fetch latest version from PyPI
pub(super) async fn fetch_pypi_latest_version(
    client: &reqwest::Client,
    package: &str,
) -> Option<String> {
    let url = format!("https://pypi.org/pypi/{package}/json");
    match client.get(&url).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("info")
                    .and_then(|info| info.get("version"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_semver() {
        use std::cmp::Ordering;
        assert_eq!(
            compare_semver("2.1.156", "2.1.154"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare_semver("2.1.154", "2.1.156"), Some(Ordering::Less));
        assert_eq!(compare_semver("2.1.156", "2.1.156"), Some(Ordering::Equal));
        // 预发布 < 同核心正式版
        assert_eq!(
            compare_semver("2.1.156-beta.1", "2.1.156"),
            Some(Ordering::Less)
        );
        // core 更高的预发布仍高于较低的正式版（gemini nightly 场景）
        assert_eq!(
            compare_semver("0.45.0-nightly.1", "0.44.1"),
            Some(Ordering::Greater)
        );
        // 大 patch（codex 时间戳式）不溢出
        assert_eq!(
            compare_semver("0.1.2505172116", "0.135.0"),
            Some(Ordering::Less)
        );
        // 无法解析返回 None（gemini 的 `false` 脏 tag）
        assert_eq!(compare_semver("false", "1.0.0"), None);
    }

    #[test]
    fn test_pick_latest_version() {
        use serde_json::json;
        let tags = json!({
            "latest": "2.1.154",
            "next": "2.1.156",
            "stable": "2.1.145"
        });
        let map = tags.as_object().unwrap();

        // 本地领先 latest（在 next 通道）→ 补查到 next，数字对齐
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.156")),
            Some("2.1.156".to_string())
        );
        // 本地等于 latest → 不补查，仍显示 latest
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.154")),
            Some("2.1.154".to_string())
        );
        // 本地落后 latest（稳定通道用户）→ 不补查，不被推向预发布版
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.145")),
            Some("2.1.154".to_string())
        );
        // 无预发布白名单 → 永远只看 latest（不解析 local，避免脏 local 触发）
        assert_eq!(
            pick_latest_version(map, &[], Some("2.1.156")),
            Some("2.1.154".to_string())
        );
        // 本地版本未知 → 保守只看 latest
        assert_eq!(
            pick_latest_version(map, &["next"], None),
            Some("2.1.154".to_string())
        );
    }

    #[test]
    fn test_pick_latest_version_filters_dirty_prerelease() {
        use serde_json::json;
        // 模拟 codex：beta 是低于 latest 的时间戳式脏版本
        let tags = json!({
            "latest": "0.135.0",
            "beta": "0.1.2505172116"
        });
        let map = tags.as_object().unwrap();
        // 即便本地领先 latest，低于 latest 的脏 beta 也不会被选
        assert_eq!(
            pick_latest_version(map, &["beta"], Some("0.200.0")),
            Some("0.135.0".to_string())
        );
    }
}
