use super::*;

const RELEASE_REPO_OWNER: &str = "Nanaloveyuki";
const RELEASE_REPO_NAME: &str = "RsLiteyukiBot";
const RELEASE_PAGE_SIZE_DEFAULT: u32 = 20;
const RELEASE_PAGE_SIZE_MAX: u32 = 100;
const GITHUB_RELEASES_PER_PAGE: usize = 100;
const GITHUB_RELEASE_FETCH_PAGES: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleasePlatformInfo {
    os: String,
    arch: String,
    display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseAssetPayload {
    name: String,
    content_type: String,
    size: u64,
    download_count: u64,
    download_url: String,
    mirror_download_url: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseVersionPayload {
    tag: String,
    name: Option<String>,
    #[serde(rename = "type")]
    release_type: String,
    html_url: String,
    mirror_html_url: Option<String>,
    body: Option<String>,
    created_at: String,
    published_at: Option<String>,
    assets: Vec<ReleaseAssetPayload>,
    recommended_asset: Option<ReleaseAssetPayload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleasePaginationPayload {
    page: u32,
    page_size: u32,
    total: u32,
    total_pages: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseListPayload {
    platform: ReleasePlatformInfo,
    current_version: String,
    versions: Vec<ReleaseVersionPayload>,
    pagination: ReleasePaginationPayload,
    mirror: Option<String>,
    repo_url: String,
    releases_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LatestReleasePayload {
    platform: ReleasePlatformInfo,
    current_version: String,
    latest: ReleaseVersionPayload,
    mirror: Option<String>,
    repo_url: String,
    releases_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    #[serde(default)]
    content_type: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    download_count: u64,
    browser_download_url: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    created_at: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ProjectVersion {
    major: u32,
    minor: u32,
    patch: u32,
    commit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseFilter {
    Release,
    Prerelease,
    All,
}

pub(super) fn route_release_api(
    api_path: &str,
    raw_path: &str,
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/base/getLatestTag" {
        let body = match run_async_for_web_host(latest_release_payload(parse_query_string(raw_path))) {
            Ok(payload) => napcat_ok(&payload.latest.tag),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/GetLatestRelease" {
        let body = match run_async_for_web_host(latest_release_payload(parse_query_string(raw_path))) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/getAllReleases" {
        let body = match run_async_for_web_host(all_releases_payload(parse_query_string(raw_path))) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/base/getMirrors" {
        #[derive(Serialize)]
        struct Mirrors {
            mirrors: Vec<String>,
        }

        let config = load_mirror_config();
        let mut mirrors = vec!["https://github.com".to_string()];
        mirrors.extend(
            config
                .file_mirrors
                .into_iter()
                .filter(|mirror| !mirror.trim().is_empty()),
        );
        let body = napcat_ok(&Mirrors { mirrors });
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/UpdateApp/update" || api_path == "/UpdateNapCat/update" {
        let body = napcat_err(
            -1,
            "Liteyuki does not perform in-place binary updates. Please download the recommended asset or open the release page.",
        );
        return Some(napcat_response(body, is_head));
    }

    None
}

async fn latest_release_payload(
    query: HashMap<String, String>,
) -> Result<LatestReleasePayload, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let platform = ReleasePlatformInfo::detect();
    let mirror = resolve_requested_mirror(&query, &load_mirror_config());
    let mut releases = fetch_github_releases().await?;
    releases.retain(|release| !release.prerelease);
    releases.sort_by(release_sort_desc);

    let latest = releases
        .into_iter()
        .find_map(|release| map_release_payload(release, &platform, mirror.as_deref()))
        .ok_or_else(|| "no valid GitHub release matched the project version rule".to_string())?;

    Ok(LatestReleasePayload {
        platform,
        current_version,
        latest,
        mirror,
        repo_url: repo_url(),
        releases_url: releases_url(),
    })
}

async fn all_releases_payload(
    query: HashMap<String, String>,
) -> Result<ReleaseListPayload, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let platform = ReleasePlatformInfo::detect();
    let mirror = resolve_requested_mirror(&query, &load_mirror_config());
    let page = parse_positive_u32(query.get("page"), 1);
    let page_size = parse_positive_u32(query.get("pageSize"), RELEASE_PAGE_SIZE_DEFAULT)
        .clamp(1, RELEASE_PAGE_SIZE_MAX);
    let filter = parse_release_filter(query.get("type"));
    let search = query
        .get("search")
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    let mut versions = fetch_github_releases()
        .await?
        .into_iter()
        .filter_map(|release| map_release_payload(release, &platform, mirror.as_deref()))
        .filter(|release| release_matches_filter(release, filter))
        .filter(|release| {
            if search.is_empty() {
                return true;
            }

            release.tag.to_ascii_lowercase().contains(search.as_str())
                || release
                    .name
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(search.as_str())
        })
        .collect::<Vec<_>>();

    versions.sort_by(release_payload_sort_desc);

    let total = versions.len() as u32;
    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };
    let page = if total_pages == 0 {
        1
    } else {
        page.clamp(1, total_pages)
    };
    let start = page.saturating_sub(1) * page_size;
    let versions = versions
        .into_iter()
        .skip(start as usize)
        .take(page_size as usize)
        .collect::<Vec<_>>();

    Ok(ReleaseListPayload {
        platform,
        current_version,
        versions,
        pagination: ReleasePaginationPayload {
            page,
            page_size,
            total,
            total_pages,
        },
        mirror,
        repo_url: repo_url(),
        releases_url: releases_url(),
    })
}

async fn fetch_github_releases() -> Result<Vec<GitHubRelease>, String> {
    let mut releases = Vec::new();

    for page in 1..=GITHUB_RELEASE_FETCH_PAGES {
        let url = github_releases_api_url(page);
        let payload =
            super::upstream::fetch_upstream_json(url.as_str(), Some("application/vnd.github+json"))
                .await?;
        let batch = serde_json::from_value::<Vec<GitHubRelease>>(payload)
            .map_err(|err| format!("invalid GitHub releases payload: {err}"))?;
        let count = batch.len();
        releases.extend(batch.into_iter().filter(|release| !release.draft));
        if count < GITHUB_RELEASES_PER_PAGE {
            break;
        }
    }

    Ok(releases)
}

fn map_release_payload(
    release: GitHubRelease,
    platform: &ReleasePlatformInfo,
    mirror: Option<&str>,
) -> Option<ReleaseVersionPayload> {
    let version = parse_project_version(release.tag_name.as_str())?;
    let assets = release
        .assets
        .into_iter()
        .map(|asset| map_asset_payload(asset, mirror))
        .collect::<Vec<_>>();
    let recommended_asset = select_recommended_asset(&assets, platform).cloned();

    Some(ReleaseVersionPayload {
        tag: release.tag_name,
        name: release.name,
        release_type: if release.prerelease {
            "prerelease".to_string()
        } else {
            "release".to_string()
        },
        html_url: release.html_url.clone(),
        mirror_html_url: mirror.map(|mirror_url| {
            super::mirror_support::resolve_mirrored_url(release.html_url.as_str(), mirror_url)
        }),
        body: release.body.filter(|body| !body.trim().is_empty()),
        created_at: release.created_at,
        published_at: release.published_at,
        assets,
        recommended_asset,
    })
    .map(|payload| (version, payload))
    .map(|(_, payload)| payload)
}

fn map_asset_payload(asset: GitHubAsset, mirror: Option<&str>) -> ReleaseAssetPayload {
    ReleaseAssetPayload {
        mirror_download_url: mirror.map(|mirror_url| {
            super::mirror_support::resolve_mirrored_url(
                asset.browser_download_url.as_str(),
                mirror_url,
            )
        }),
        name: asset.name,
        content_type: asset.content_type,
        size: asset.size,
        download_count: asset.download_count,
        download_url: asset.browser_download_url,
        created_at: asset.created_at,
        updated_at: asset.updated_at,
    }
}

fn parse_project_version(tag: &str) -> Option<ProjectVersion> {
    let trimmed = tag.trim();
    let trimmed = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    let (core, suffix) = match trimmed.split_once('-') {
        Some((core, suffix)) => (core, Some(suffix)),
        None => (trimmed, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    let commit = match suffix {
        None => 0,
        Some(raw) => {
            let commit_part = raw.split('-').next().unwrap_or_default();
            let digits = commit_part.strip_prefix('c')?;
            if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            digits.parse::<u32>().ok()?
        }
    };

    Some(ProjectVersion {
        major,
        minor,
        patch,
        commit,
    })
}

fn select_recommended_asset<'a>(
    assets: &'a [ReleaseAssetPayload],
    platform: &ReleasePlatformInfo,
) -> Option<&'a ReleaseAssetPayload> {
    assets
        .iter()
        .filter_map(|asset| asset_match_score(asset.name.as_str(), platform).map(|score| (score, asset)))
        .max_by(|(left_score, left_asset), (right_score, right_asset)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_asset.size.cmp(&left_asset.size))
        })
        .map(|(_, asset)| asset)
}

fn asset_match_score(name: &str, platform: &ReleasePlatformInfo) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let arch_bonus = arch_match_bonus(lower.as_str(), platform)?;
    let os_score = match platform.os.as_str() {
        "windows" => windows_asset_score(lower.as_str())?,
        "macos" => macos_asset_score(lower.as_str())?,
        "linux" => linux_asset_score(lower.as_str())?,
        _ => return None,
    };

    Some(os_score + arch_bonus)
}

fn arch_match_bonus(name: &str, platform: &ReleasePlatformInfo) -> Option<u32> {
    let wants_arm64 = matches!(platform.arch.as_str(), "aarch64" | "arm64");
    let wants_x64 = matches!(platform.arch.as_str(), "x86_64" | "x64" | "amd64");
    let mentions_arm64 = name.contains("aarch64") || name.contains("arm64");
    let mentions_x64 = name.contains("x86_64") || name.contains("x64") || name.contains("amd64");
    let mentions_x86 = name.contains("x86") || name.contains("i686") || name.contains("i386");

    if wants_arm64 {
        if mentions_x64 || mentions_x86 {
            return None;
        }
        return Some(if mentions_arm64 { 50 } else { 5 });
    }

    if wants_x64 {
        if mentions_arm64 || mentions_x86 {
            return None;
        }
        return Some(if mentions_x64 { 50 } else { 5 });
    }

    Some(5)
}

fn windows_asset_score(name: &str) -> Option<u32> {
    if name.ends_with("-setup.exe") {
        return Some(300);
    }
    if name.ends_with(".msi") {
        return Some(260);
    }
    if name.ends_with(".exe") {
        return Some(220);
    }
    if name.ends_with(".zip") && (name.contains("windows") || name.contains("win") || name.contains("setup")) {
        return Some(180);
    }
    None
}

fn macos_asset_score(name: &str) -> Option<u32> {
    if name.ends_with(".dmg") {
        return Some(300);
    }
    if name.ends_with(".app.tar.gz") {
        return Some(240);
    }
    None
}

fn linux_asset_score(name: &str) -> Option<u32> {
    if name.ends_with(".appimage") {
        return Some(300);
    }
    if name.ends_with(".deb") {
        return Some(260);
    }
    if name.ends_with(".rpm") {
        return Some(240);
    }
    if name.ends_with(".tar.gz") && !name.contains(".app.") {
        return Some(200);
    }
    if name.ends_with(".tar.xz") {
        return Some(180);
    }
    None
}

fn release_sort_desc(left: &GitHubRelease, right: &GitHubRelease) -> std::cmp::Ordering {
    match (
        parse_project_version(left.tag_name.as_str()),
        parse_project_version(right.tag_name.as_str()),
    ) {
        (Some(left_version), Some(right_version)) => right_version
            .cmp(&left_version)
            .then_with(|| right.published_at.cmp(&left.published_at)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => right.published_at.cmp(&left.published_at),
    }
}

fn release_payload_sort_desc(
    left: &ReleaseVersionPayload,
    right: &ReleaseVersionPayload,
) -> std::cmp::Ordering {
    match (
        parse_project_version(left.tag.as_str()),
        parse_project_version(right.tag.as_str()),
    ) {
        (Some(left_version), Some(right_version)) => right_version
            .cmp(&left_version)
            .then_with(|| right.published_at.cmp(&left.published_at)),
        _ => right.published_at.cmp(&left.published_at),
    }
}

fn release_matches_filter(release: &ReleaseVersionPayload, filter: ReleaseFilter) -> bool {
    match filter {
        ReleaseFilter::Release => release.release_type == "release",
        ReleaseFilter::Prerelease => release.release_type == "prerelease",
        ReleaseFilter::All => true,
    }
}

fn parse_release_filter(raw: Option<&String>) -> ReleaseFilter {
    match raw.map(String::as_str).unwrap_or("release") {
        "all" => ReleaseFilter::All,
        "prerelease" => ReleaseFilter::Prerelease,
        _ => ReleaseFilter::Release,
    }
}

fn parse_positive_u32(raw: Option<&String>, default: u32) -> u32 {
    raw.and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn resolve_requested_mirror(
    query: &HashMap<String, String>,
    config: &MirrorConfigDoc,
) -> Option<String> {
    query
        .get("mirror")
        .map(String::as_str)
        .or(config.custom_mirror.as_deref())
        .map(str::trim)
        .filter(|mirror| !mirror.is_empty())
        .map(ToString::to_string)
}

fn repo_url() -> String {
    format!("https://github.com/{RELEASE_REPO_OWNER}/{RELEASE_REPO_NAME}")
}

fn releases_url() -> String {
    format!("{}/releases", repo_url())
}

fn github_api_base_url() -> String {
    std::env::var("LY_RELEASE_GITHUB_API_BASE")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.github.com".to_string())
}

fn github_releases_api_url(page: usize) -> String {
    format!(
        "{}/repos/{RELEASE_REPO_OWNER}/{RELEASE_REPO_NAME}/releases?per_page={GITHUB_RELEASES_PER_PAGE}&page={page}",
        github_api_base_url()
    )
}

impl ReleasePlatformInfo {
    fn detect() -> Self {
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let display_name = format!("{os} / {arch}");
        Self {
            os,
            arch,
            display_name,
        }
    }
}
