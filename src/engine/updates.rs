//! Vérifie si une version plus récente est publiée dans les Releases GitHub.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = "xLaugh/CrispTrim";
pub const REPO_URL: &str = "https://github.com/xLaugh/CrispTrim";
/// GitHub limite l'API publique à 60 requêtes par heure.
const TTL: f64 = 21600.0; // 6 h

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub url: String,
    pub notes: String,
    pub published: String,
}

#[derive(Clone, Debug)]
pub struct UpdateStatus {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub release: Option<Release>,
    pub error: String,
}

/// « v1.2.3 », « 1.2 » → (1, 2, 3) / (1, 2, 0). Les suffixes (-beta…) sont ignorés.
pub fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let re = regex::Regex::new(r"^\s*v?(\d+)\.(\d+)(?:\.(\d+))?").unwrap();
    let c = re.captures(v)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?, c.get(3).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0)))
}

/// Tag de release accepté par le lanceur (`TAG_RE` du script `launcher` de CrispTrim.exe) :
/// « v1.2 » ou « v1.2.3 », sans suffixe. Les autres tags ne sont pas proposés en mise à jour.
pub fn is_release_tag(tag: &str) -> bool {
    regex::Regex::new(r"^v\d+\.\d+(\.\d+)?$").map(|re| re.is_match(tag)).unwrap_or(false)
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(a), Some(b)) if a > b)
}

/// Dernière release publiée, ou None s'il n'y en a pas.
pub fn fetch_latest() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = ureq::get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &format!("CrispTrim/{VERSION}"))
        .timeout(std::time::Duration::from_secs(5))
        .call();
    let data: serde_json::Value = match resp {
        Ok(r) => r.into_json().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(404, _)) => return Ok(None), // aucune release
        Err(e) => return Err(e.to_string()),
    };
    let s = |k: &str| data[k].as_str().unwrap_or("").to_string();
    let tag = s("tag_name");
    Ok(Some(Release {
        name: if s("name").is_empty() { tag.clone() } else { s("name") },
        url: if s("html_url").is_empty() { format!("{REPO_URL}/releases") } else { s("html_url") },
        notes: s("body").trim().to_string(),
        published: s("published_at"),
        tag,
    }))
}

#[derive(Serialize, Deserialize)]
struct Cache {
    checked: f64,
    release: Option<Release>,
}

/// État de mise à jour, avec un cache de quelques heures. Ne lève jamais d'erreur.
pub fn check(cache_file: &Path, force: bool) -> UpdateStatus {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let cached: Option<Cache> = std::fs::read_to_string(cache_file).ok().and_then(|t| serde_json::from_str(&t).ok());
    let mut error = String::new();
    let release = match &cached {
        Some(c) if !force && now - c.checked < TTL => c.release.clone(),
        _ => match fetch_latest() {
            Ok(r) => {
                if let Some(parent) = cache_file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(cache_file, serde_json::to_string(&Cache { checked: now, release: r.clone() }).unwrap_or_default());
                r
            }
            Err(e) => {
                error = e; // hors ligne, GitHub indisponible…
                cached.and_then(|c| c.release)
            }
        },
    };
    let latest = release.as_ref().map(|r| r.tag.clone());
    UpdateStatus {
        current: VERSION.to_string(),
        available: latest.as_deref().map(|l| is_release_tag(l) && is_newer(l, VERSION)).unwrap_or(false),
        latest,
        release,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
        assert!(is_release_tag("v0.3.0") && is_release_tag("v1.2") && !is_release_tag("v0.3.0-beta") && !is_release_tag("0.3.0"));
        assert!(is_newer("v0.3.0", "0.2.0") && !is_newer("v0.2.0", "0.2.0") && !is_newer("x", "0.1"));
    }
}
