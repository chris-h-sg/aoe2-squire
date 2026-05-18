use std::time::Duration;

/// Parses a version string like "0.1.2" or "v0.1.2" into a major, minor, and patch number tuple.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let clean = v.trim().strip_prefix('v').unwrap_or(v.trim());
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let major = parts[0].parse::<u32>().ok()?;
    let minor = parts[1].parse::<u32>().ok()?;
    let patch = parts[2].parse::<u32>().ok()?;
    Some((major, minor, patch))
}

/// Checks if `latest` is a higher version than `current`.
pub fn is_update_available(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(curr), Some(lat)) => lat > curr,
        _ => false,
    }
}

/// Fetches the latest release tag name from GitHub for a given repository.
/// Returns an error if the request fails, times out, or the response cannot be parsed.
pub fn fetch_latest_release_tag(repo: &str) -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);

    // Set user-agent (required by GitHub API) and a short 2-second timeout
    let response: serde_json::Value = ureq::get(&url)
        .set("User-Agent", "aoe2-squire")
        .timeout(Duration::from_secs(2))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;

    let tag = response["tag_name"]
        .as_str()
        .ok_or_else(|| "Missing 'tag_name' field in GitHub release JSON".to_string())?;

    Ok(tag.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("  v1.20.300  "), Some((1, 20, 300)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("v1.a.3"), None);
    }

    #[test]
    fn test_is_update_available() {
        // Higher version available
        assert!(is_update_available("0.1.2", "0.1.3"));
        assert!(is_update_available("0.1.2", "v0.1.3"));
        assert!(is_update_available("0.1.2", "v0.2.0"));
        assert!(is_update_available("0.1.2", "v1.0.0"));

        // Multi-digit components
        assert!(is_update_available("0.1.2", "v0.1.10"));
        assert!(is_update_available("0.1.9", "v0.1.10"));
        assert!(is_update_available("0.9.9", "v0.10.0"));
        assert!(is_update_available("0.9.9", "v1.0.0"));

        // No update (same version)
        assert!(!is_update_available("0.1.2", "0.1.2"));
        assert!(!is_update_available("0.1.2", "v0.1.2"));
        assert!(!is_update_available("v0.1.2", "0.1.2"));

        // No update (lower version)
        assert!(!is_update_available("0.1.2", "0.1.1"));
        assert!(!is_update_available("0.1.10", "0.1.2"));
        assert!(!is_update_available("1.0.0", "0.9.9"));

        // Invalid version inputs should not crash and should return false
        assert!(!is_update_available("invalid", "0.1.2"));
        assert!(!is_update_available("0.1.2", "invalid"));
    }
}
