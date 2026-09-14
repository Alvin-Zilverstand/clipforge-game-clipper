use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_UPDATE_REPO: &str = "Alvin-Zilverstand/clipforge-game-clipper";
const UPDATE_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    #[serde(default)]
    pub size: Option<u64>,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UpdateCheck {
    pub update_available: bool,
    pub latest_version: String,
    pub installer_file: Option<String>,
    pub download_url: Option<String>,
    pub published_at: Option<String>,
    pub release_notes: Option<String>,
    pub message: String,
}

#[derive(Debug)]
pub enum UpdateCheckError {
    Http(String),
    Json(String),
    NoReleaseFound,
    NoInstallerAsset(String),
    Io(io::Error),
}

impl fmt::Display for UpdateCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(message) => write!(formatter, "Update check HTTP error: {message}"),
            Self::Json(message) => write!(formatter, "Could not parse update response: {message}"),
            Self::NoReleaseFound => write!(formatter, "No published releases found yet."),
            Self::NoInstallerAsset(version) => write!(
                formatter,
                "Release {version} has no MSI or NSIS installer asset."
            ),
            Self::Io(error) => write!(formatter, "Update file error: {error}"),
        }
    }
}

impl std::error::Error for UpdateCheckError {}

impl From<io::Error> for UpdateCheckError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn http_client(current_version: &str) -> Result<reqwest::blocking::Client, UpdateCheckError> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("ClipForge/{current_version}"))
        .timeout(Duration::from_secs(UPDATE_TIMEOUT_SECS))
        .build()
        .map_err(|error| UpdateCheckError::Http(error.to_string()))
}

pub fn check_for_updates(
    repo: &str,
    current_version: &str,
) -> Result<UpdateCheck, UpdateCheckError> {
    let client = http_client(current_version)?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let response = client
        .get(&url)
        .send()
        .map_err(|error| UpdateCheckError::Http(error.to_string()))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(UpdateCheckError::NoReleaseFound);
    }
    if !response.status().is_success() {
        return Err(UpdateCheckError::Http(format!(
            "GitHub returned {} for {}",
            response.status(),
            url
        )));
    }

    let release: GitHubRelease = response
        .json()
        .map_err(|error| UpdateCheckError::Json(error.to_string()))?;

    build_update_check(current_version, &release)
}

pub fn build_update_check(
    current_version: &str,
    release: &GitHubRelease,
) -> Result<UpdateCheck, UpdateCheckError> {
    let latest_version = normalize_version(&release.tag_name);
    let newer_release_exists = compare_versions(current_version, &latest_version) == Ordering::Less;
    let installer = select_installer_asset(&release.assets);

    // A newer tag without a published installer is not something the user can
    // download, so never claim the app is installable in that state.
    let update_available = newer_release_exists && installer.is_some();

    let message = if !newer_release_exists {
        "You are on the latest version.".to_string()
    } else if installer.is_none() {
        format!(
            "A newer version ({latest_version}) exists, but no installer has been published for it yet."
        )
    } else {
        let file = installer
            .as_ref()
            .map(|asset| asset.name.as_str())
            .unwrap_or("installer");
        format!(
            "A newer version ({latest_version}) is available — {file}, ready to download."
        )
    };

    Ok(UpdateCheck {
        update_available,
        latest_version,
        installer_file: installer.as_ref().map(|asset| asset.name.clone()),
        download_url: installer.map(|asset| asset.browser_download_url),
        published_at: release.published_at.clone(),
        release_notes: release.body.clone(),
        message,
    })
}

pub fn select_installer_asset(assets: &[GitHubAsset]) -> Option<GitHubAsset> {
    assets
        .iter()
        .find(|asset| asset.name.ends_with(".msi"))
        .or_else(|| {
            assets.iter().find(|asset| {
                asset.name.ends_with(".exe") && !asset.name.to_lowercase().contains("uninstall")
            })
        })
        .cloned()
}

pub fn download_installer(
    url: &str,
    destination_dir: &Path,
    file_name: &str,
    current_version: &str,
) -> Result<PathBuf, UpdateCheckError> {
    std::fs::create_dir_all(destination_dir)?;
    let destination = destination_dir.join(file_name);
    let client = http_client(current_version)?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|error| UpdateCheckError::Http(error.to_string()))?;
    if !response.status().is_success() {
        return Err(UpdateCheckError::Http(format!(
            "Download failed with {}",
            response.status()
        )));
    }

    let mut file = std::fs::File::create(&destination)?;
    io::copy(&mut response, &mut file)
        .map_err(|error| UpdateCheckError::Io(error))?;
    Ok(destination)
}

pub fn normalize_version(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

pub fn compare_versions(current: &str, latest: &str) -> Ordering {
    numeric_parts(current).cmp(&numeric_parts(latest))
}

fn numeric_parts(version: &str) -> Vec<u32> {
    version
        .trim_start_matches('v')
        .split('.')
        .map(|segment| {
            segment
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .unwrap_or(0)
        })
        .take(3)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_version_is_newer_than_current() {
        assert_eq!(compare_versions("0.1.0", "0.2.0"), Ordering::Less);
        assert_eq!(compare_versions("0.1.9", "0.2.0"), Ordering::Less);
        assert_eq!(compare_versions("0.9.9", "1.0.0"), Ordering::Less);
    }

    #[test]
    fn old_version_is_not_newer() {
        assert_eq!(compare_versions("0.2.0", "0.1.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.3.0", "0.3.0"), Ordering::Equal);
    }

    #[test]
    fn installed_040_detects_published_050_as_available_update() {
        let release = GitHubRelease {
            tag_name: "v0.0.5".to_string(),
            published_at: Some("2026-09-08T00:00:00Z".to_string()),
            body: None,
            assets: vec![GitHubAsset {
                name: "ClipForge_0.1.1_x64_en-US.msi".to_string(),
                size: Some(1_000_000),
                browser_download_url: "https://example.test/ClipForge_0.1.1.msi".to_string(),
            }],
        };
        let check = build_update_check("0.0.4", &release).expect("check");

        assert!(check.update_available, "v0.0.4 must see v0.0.5 as an update");
        assert_eq!(check.latest_version, "0.0.5");
        assert_eq!(
            check.installer_file.as_deref(),
            Some("ClipForge_0.1.1_x64_en-US.msi")
        );
    }

    #[test]
    fn same_version_is_reported_as_up_to_date() {
        let release = GitHubRelease {
            tag_name: "v0.1.1".to_string(),
            published_at: None,
            body: None,
            assets: vec![GitHubAsset {
                name: "ClipForge_0.1.1_x64-setup.exe".to_string(),
                size: None,
                browser_download_url: "https://example.test/setup.exe".to_string(),
            }],
        };
        let check = build_update_check("0.1.1", &release).expect("check");

        assert!(!check.update_available);
        assert_eq!(check.message, "You are on the latest version.");
    }

    #[test]
    fn newer_tag_without_installer_is_not_offered_for_download() {
        let release = GitHubRelease {
            tag_name: "v0.9.0".to_string(),
            published_at: Some("2026-09-09T00:00:00Z".to_string()),
            body: Some("notes".to_string()),
            assets: vec![GitHubAsset {
                name: "checksums.txt".to_string(),
                size: Some(128),
                browser_download_url: "https://example.test/checksums.txt".to_string(),
            }],
        };
        let check = build_update_check("0.1.1", &release).expect("check");

        assert!(!check.update_available);
        assert!(check.download_url.is_none());
        assert!(check.message.contains("no installer has been published"));
    }

    #[test]
    fn patch_versions_compare_numerically_not_lexically() {
        assert_eq!(compare_versions("0.0.9", "0.0.10"), Ordering::Less);
        assert_eq!(compare_versions("0.0.10", "0.0.10"), Ordering::Equal);
        assert_eq!(compare_versions("0.0.11", "0.0.10"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.0", "0.1.1"), Ordering::Less);
    }

    #[test]
    fn older_tag_backports_do_not_mask_newer_release() {
        assert_eq!(compare_versions("0.2.0", "0.1.1"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.1", "0.2.0"), Ordering::Less);
    }

    #[test]
    fn leading_v_and_prerelease_tags_are_ignored() {
        assert_eq!(compare_versions("v0.1.0", "v0.2.0"), Ordering::Less);
        assert_eq!(compare_versions("v0.2.0", "0.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.2.0", "0.2.0-beta.1"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.0", "0.2.0-beta.1"), Ordering::Less);
    }

    #[test]
    fn msi_asset_is_preferred_over_exe() {
        let assets = vec![
            GitHubAsset {
                name: "ClipForge_0.2.0_x64-setup.exe".to_string(),
                size: Some(1),
                browser_download_url: "https://example.test/setup.exe".to_string(),
            },
            GitHubAsset {
                name: "ClipForge_0.2.0_x64_en-US.msi".to_string(),
                size: Some(2),
                browser_download_url: "https://example.test/setup.msi".to_string(),
            },
        ];
        let selected = select_installer_asset(&assets).expect("asset");
        assert_eq!(selected.name, "ClipForge_0.2.0_x64_en-US.msi");
    }

    #[test]
    fn exe_asset_is_selected_when_no_msi_exists() {
        let assets = vec![GitHubAsset {
            name: "ClipForge_0.2.0_x64-setup.exe".to_string(),
            size: Some(1),
            browser_download_url: "https://example.test/setup.exe".to_string(),
        }];
        let selected = select_installer_asset(&assets).expect("asset");
        assert_eq!(selected.name, "ClipForge_0.2.0_x64-setup.exe");
    }

    #[test]
    fn uninstaller_and_unknown_assets_are_rejected() {
        let assets = vec![
            GitHubAsset {
                name: "ClipForge_0.2.0_x64_uninstall.exe".to_string(),
                size: Some(1),
                browser_download_url: "https://example.test/uninstall.exe".to_string(),
            },
            GitHubAsset {
                name: "linux-port.AppImage".to_string(),
                size: Some(1),
                browser_download_url: "https://example.test/linux.AppImage".to_string(),
            },
            GitHubAsset {
                name: "checksums.txt".to_string(),
                size: Some(1),
                browser_download_url: "https://example.test/checksums.txt".to_string(),
            },
        ];
        assert!(select_installer_asset(&assets).is_none());
        assert_eq!(normalize_version("v0.2.0"), "0.2.0");
    }

    #[test]
    fn update_check_marks_latest_github_release_as_available() {
        let release = GitHubRelease {
            tag_name: "v0.2.0".to_string(),
            published_at: Some("2026-09-01T00:00:00Z".to_string()),
            body: Some("Update notes".to_string()),
            assets: vec![GitHubAsset {
                name: "ClipForge_0.2.0_x64-setup.exe".to_string(),
                size: Some(1_000_000),
                browser_download_url: "https://example.test/ClipForge_0.2.0.exe".to_string(),
            }],
        };
        let check = build_update_check("0.1.0", &release).expect("check");
        assert!(check.update_available);
        assert_eq!(check.latest_version, "0.2.0");
        assert_eq!(
            check.installer_file.as_deref(),
            Some("ClipForge_0.2.0_x64-setup.exe")
        );
        assert_eq!(check.published_at.as_deref(), Some("2026-09-01T00:00:00Z"));
    }

    #[test]
    #[ignore = "requires live network access to the GitHub API"]
    fn live_check_against_github_repo() {
        let check = check_for_updates(DEFAULT_UPDATE_REPO, "0.0.1").expect("check");
        assert!(
            check.download_url.as_deref().filter(|value| !value.is_empty()).is_some()
                || !check.update_available
        );
        assert!(!check.latest_version.is_empty());
    }
}