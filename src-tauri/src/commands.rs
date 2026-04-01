use crate::HttpClient;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_store::{Store, StoreExt};

// ─── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub install_path: String,
    pub build_server_url: String,
    pub sso_url: String,
    pub signing_identity: String,
    pub apple_team_id: String,
    pub windows_cert_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            install_path: default_install_path(),
            build_server_url: "https://raw.githubusercontent.com/LightningWorksGames/SiegeWorldsBuild/main"
                .to_string(),
            sso_url: "https://sso.lightningworks.io".to_string(),
            signing_identity: String::new(),
            apple_team_id: String::new(),
            windows_cert_path: String::new(),
        }
    }
}

fn default_install_path() -> String {
    if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map(|h| {
                h.join("Games")
                    .join("Siege Worlds")
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_else(|| "/Applications/Siege Worlds".to_string())
    } else if cfg!(target_os = "linux") {
        dirs::home_dir()
            .map(|h| {
                h.join("Games")
                    .join("Siege Worlds")
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_else(|| "/opt/siege-worlds".to_string())
    } else {
        "C:\\Games\\Siege Worlds".to_string()
    }
}

// ─── Store Helpers ──────────────────────────────────────────────────────────

fn get_store(app: &AppHandle) -> Arc<Store<tauri::Wry>> {
    app.store("settings.json").expect("failed to access store")
}

fn store_get_string(store: &Store<tauri::Wry>, key: &str, default: String) -> String {
    store
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or(default)
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    let store = get_store(&app);
    let defaults = AppSettings::default();
    AppSettings {
        install_path: store_get_string(&store, "install_path", defaults.install_path),
        build_server_url: store_get_string(&store, "build_server_url", defaults.build_server_url),
        sso_url: store_get_string(&store, "sso_url", defaults.sso_url),
        signing_identity: store_get_string(&store, "signing_identity", defaults.signing_identity),
        apple_team_id: store_get_string(&store, "apple_team_id", defaults.apple_team_id),
        windows_cert_path: store_get_string(&store, "windows_cert_path", defaults.windows_cert_path),
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let store = get_store(&app);
    store.set("install_path", serde_json::json!(settings.install_path));
    store.set("build_server_url", serde_json::json!(settings.build_server_url));
    store.set("sso_url", serde_json::json!(settings.sso_url));
    store.set("signing_identity", serde_json::json!(settings.signing_identity));
    store.set("apple_team_id", serde_json::json!(settings.apple_team_id));
    store.set("windows_cert_path", serde_json::json!(settings.windows_cert_path));
    store.save().map_err(|e| format!("Failed to save: {}", e))
}

#[tauri::command]
pub async fn select_install_path(app: AppHandle) -> Result<String, String> {
    let app_clone = app.clone();
    let path = tokio::task::spawn_blocking(move || {
        app_clone
            .dialog()
            .file()
            .blocking_pick_folder()
            .map(|p| p.to_string())
            .ok_or_else(|| "No folder selected".to_string())
    })
    .await
    .map_err(|e| format!("Dialog task failed: {}", e))??;

    let store = get_store(&app);
    store.set("install_path", serde_json::json!(&path));
    let _ = store.save();
    Ok(path)
}

// ─── Path Safety ────────────────────────────────────────────────────────────

fn validate_manifest_path(path: &str) -> Result<(), String> {
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(format!("Rejected absolute path in manifest: {}", path));
    }
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(format!("Rejected absolute path in manifest: {}", path));
    }
    for component in path.split(['/', '\\']) {
        if component == ".." {
            return Err(format!(
                "Rejected path traversal in manifest: {}",
                path
            ));
        }
    }
    Ok(())
}

// ─── File Hashing ───────────────────────────────────────────────────────────

fn hash_file(path: &PathBuf) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open for hashing: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read for hashing: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Case-insensitive hash comparison (handles uppercase/lowercase hex from different tools).
fn hashes_match(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

// ─── Game Download & Launch ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    path: String,
    hash: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct DownloadProgress {
    current: usize,
    total: usize,
    file: String,
}

/// Get the shared HTTP client from Tauri managed state.
fn http_client(app: &AppHandle) -> reqwest::Client {
    app.state::<HttpClient>().0.clone()
}

/// Get the user's stored access token (their Supabase JWT from SSO).
fn get_user_token(app: &AppHandle) -> Option<String> {
    let store = get_store(app);
    store
        .get("access_token")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|t| !t.is_empty())
}


async fn fetch_manifest(
    app: &AppHandle,
    client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<ManifestEntry>, String> {
    if base_url.starts_with("http://") {
        app.emit(
            "log",
            "WARNING: Build server is using HTTP (not HTTPS). Downloads are not encrypted."
                .to_string(),
        )
        .map_err(|e| e.to_string())?;
    }

    let manifest_url = format!("{}/file_manifest.json", base_url);
    let res = client
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch manifest: {}", e))?;

    if !res.status().is_success() {
        return Err(format!(
            "Manifest server returned {}",
            res.status()
        ));
    }

    let text = res
        .text()
        .await
        .map_err(|e| format!("Failed to read manifest: {}", e))?;

    let entries: Vec<ManifestEntry> =
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse manifest: {}", e))?;

    for entry in &entries {
        validate_manifest_path(&entry.path)?;
    }

    Ok(entries)
}

/// Collect all files under a directory recursively, returning paths relative to the base.
fn collect_local_files(base: &PathBuf) -> HashSet<String> {
    let mut files = HashSet::new();
    if let Ok(entries) = walkdir(base, base) {
        files = entries;
    }
    files
}

fn walkdir(base: &PathBuf, current: &PathBuf) -> Result<HashSet<String>, std::io::Error> {
    let mut result = HashSet::new();
    if !current.is_dir() {
        return Ok(result);
    }
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            result.extend(walkdir(base, &path)?);
        } else if let Ok(relative) = path.strip_prefix(base) {
            // Normalize to forward slashes for cross-platform comparison
            let rel_str = relative.to_string_lossy().replace('\\', "/");
            result.insert(rel_str);
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn check_updates(app: AppHandle) -> Result<String, String> {
    let store = get_store(&app);
    let defaults = AppSettings::default();
    let base_url = store_get_string(&store, "build_server_url", defaults.build_server_url);
    let install_path = store_get_string(&store, "install_path", defaults.install_path);

    let client = http_client(&app);
    let entries = fetch_manifest(&app, &client, &base_url).await?;
    let install_dir = PathBuf::from(&install_path);

    let mut needs_download = 0;
    let mut up_to_date = 0;

    for entry in &entries {
        let file_path = install_dir.join(&entry.path);
        if file_path.exists() {
            if let Some(expected_hash) = &entry.hash {
                match hash_file(&file_path) {
                    Ok(local_hash) if hashes_match(&local_hash, expected_hash) => {
                        up_to_date += 1;
                    }
                    _ => {
                        needs_download += 1;
                    }
                }
            } else {
                up_to_date += 1;
            }
        } else {
            needs_download += 1;
        }
    }

    // Check for orphaned files
    let manifest_paths: HashSet<String> = entries
        .iter()
        .map(|e| e.path.replace('\\', "/"))
        .collect();
    let local_files = collect_local_files(&install_dir);
    let orphans = local_files.difference(&manifest_paths).count();

    let mut msg = if needs_download == 0 {
        format!("All {} files are up to date!", entries.len())
    } else {
        format!(
            "{} files need updating ({} already up to date)",
            needs_download, up_to_date
        )
    };

    if orphans > 0 {
        msg.push_str(&format!(". {} old files will be removed", orphans));
    }

    Ok(msg)
}

#[tauri::command]
pub async fn download_game(app: AppHandle) -> Result<(), String> {
    let store = get_store(&app);
    let defaults = AppSettings::default();
    let base_url = store_get_string(&store, "build_server_url", defaults.build_server_url);
    let install_path = store_get_string(&store, "install_path", defaults.install_path);

    app.emit("log", "Fetching file manifest...".to_string())
        .map_err(|e| e.to_string())?;

    let client = http_client(&app);
    let entries = fetch_manifest(&app, &client, &base_url).await?;
    let install_dir = PathBuf::from(&install_path);
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Failed to create install directory: {}", e))?;

    // ── Phase 1: Determine which files need downloading ──
    let mut to_download: Vec<&ManifestEntry> = Vec::new();
    let mut skipped = 0;

    for entry in &entries {
        let file_path = install_dir.join(&entry.path);
        if file_path.exists() {
            if let Some(expected_hash) = &entry.hash {
                match hash_file(&file_path) {
                    Ok(local_hash) if hashes_match(&local_hash, expected_hash) => {
                        skipped += 1;
                        continue;
                    }
                    _ => {}
                }
            } else {
                skipped += 1;
                continue;
            }
        }
        to_download.push(entry);
    }

    // ── Phase 2: Remove orphaned files not in manifest ──
    let manifest_paths: HashSet<String> = entries
        .iter()
        .map(|e| e.path.replace('\\', "/"))
        .collect();
    let local_files = collect_local_files(&install_dir);
    let mut orphans_removed = 0;
    for orphan in local_files.difference(&manifest_paths) {
        let orphan_path = install_dir.join(orphan);
        if std::fs::remove_file(&orphan_path).is_ok() {
            orphans_removed += 1;
        }
    }
    if orphans_removed > 0 {
        app.emit(
            "log",
            format!("Removed {} old files", orphans_removed),
        )
        .map_err(|e| e.to_string())?;
    }

    if to_download.is_empty() {
        app.emit("log", "All files are up to date!".to_string())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    app.emit(
        "log",
        format!(
            "Downloading {} files ({} already up to date)",
            to_download.len(),
            skipped
        ),
    )
    .map_err(|e| e.to_string())?;

    // ── Phase 3: Download files, continue on individual failures ──
    let total = to_download.len();
    let mut failed: Vec<String> = Vec::new();

    for (i, entry) in to_download.iter().enumerate() {
        let file_url = format!("{}/{}", base_url, entry.path);
        let file_path = install_dir.join(&entry.path);

        if let Some(parent) = file_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                let msg = format!("FAILED {}: can't create directory: {}", entry.path, e);
                app.emit("log", msg.clone()).ok();
                failed.push(msg);
                continue;
            }
        }

        app.emit(
            "download-progress",
            DownloadProgress {
                current: i + 1,
                total,
                file: entry.path.clone(),
            },
        )
        .ok();

        app.emit("log", format!("Downloading: {}", entry.path)).ok();

        // Download the file
        let response = match client.get(&file_url).send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("FAILED {}: {}", entry.path, e);
                app.emit("log", msg.clone()).ok();
                failed.push(msg);
                continue;
            }
        };

        if !response.status().is_success() {
            let msg = format!(
                "FAILED {}: server returned {}",
                entry.path,
                response.status()
            );
            app.emit("log", msg.clone()).ok();
            failed.push(msg);
            continue;
        }

        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("FAILED {}: read error: {}", entry.path, e);
                app.emit("log", msg.clone()).ok();
                failed.push(msg);
                continue;
            }
        };

        // Verify hash before writing
        if let Some(expected_hash) = &entry.hash {
            let actual_hash = hash_bytes(&bytes);
            if !hashes_match(&actual_hash, expected_hash) {
                let msg = format!(
                    "FAILED {}: hash mismatch (expected {}, got {})",
                    entry.path, expected_hash, actual_hash
                );
                app.emit("log", msg.clone()).ok();
                failed.push(msg);
                continue;
            }
        }

        // Write to temp file then rename (atomic-ish) to avoid partial writes
        let temp_path = file_path.with_extension("sw_tmp");
        if let Err(e) = std::fs::write(&temp_path, &bytes) {
            let msg = format!("FAILED {}: write error: {}", entry.path, e);
            app.emit("log", msg.clone()).ok();
            failed.push(msg);
            // Clean up temp file
            let _ = std::fs::remove_file(&temp_path);
            continue;
        }
        if let Err(e) = std::fs::rename(&temp_path, &file_path) {
            let msg = format!("FAILED {}: rename error: {}", entry.path, e);
            app.emit("log", msg.clone()).ok();
            failed.push(msg);
            let _ = std::fs::remove_file(&temp_path);
            continue;
        }
    }

    // ── Report results ──
    if failed.is_empty() {
        app.emit("log", "Download complete!".to_string()).ok();
    } else {
        app.emit(
            "log",
            format!(
                "Download finished with {} failures out of {}",
                failed.len(),
                total
            ),
        )
        .ok();
    }

    app.emit(
        "download-progress",
        DownloadProgress {
            current: total,
            total,
            file: "Complete".to_string(),
        },
    )
    .ok();

    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("{} files failed to download", failed.len()))
    }
}

#[tauri::command]
pub async fn launch_game(app: AppHandle) -> Result<(), String> {
    let store = get_store(&app);
    let install_path = store_get_string(&store, "install_path", AppSettings::default().install_path);
    let install_dir = PathBuf::from(&install_path);

    // Check for stored auth token to pass to the game
    let auth_token = store
        .get("access_token")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|t| !t.is_empty());

    let exe_path = if cfg!(target_os = "macos") {
        let app_path = install_dir.join("Siege Worlds.app");
        if app_path.exists() {
            app_path
        } else {
            install_dir.join("Siege Worlds")
        }
    } else if cfg!(target_os = "linux") {
        let path = install_dir.join("Siege Worlds.x86_64");
        if path.exists() {
            path
        } else {
            install_dir.join("Siege Worlds")
        }
    } else {
        install_dir.join("Siege Worlds.exe")
    };

    if !exe_path.exists() {
        return Err(format!(
            "Game not found at {}. Please download it first.",
            exe_path.display()
        ));
    }

    if auth_token.is_some() {
        app.emit("log", "Launching with SSO token (auto-login)...".to_string())
            .map_err(|e| e.to_string())?;
    } else {
        app.emit("log", format!("Launching: {}", exe_path.display()))
            .map_err(|e| e.to_string())?;
    }

    // Build launch arguments: pass JWT as --auth-token if user is signed in
    let mut args: Vec<String> = Vec::new();
    if let Some(token) = &auth_token {
        args.push("--auth-token".to_string());
        args.push(token.clone());
    }

    if cfg!(target_os = "macos") {
        let mut cmd = Command::new("open");
        cmd.arg(&exe_path);
        if !args.is_empty() {
            cmd.arg("--args");
            cmd.args(&args);
        }
        cmd.spawn()
            .map_err(|e| format!("Failed to launch game: {}", e))?;
    } else if cfg!(target_os = "linux") {
        let _ = Command::new("chmod").arg("+x").arg(&exe_path).output();
        Command::new(&exe_path)
            .args(&args)
            .spawn()
            .map_err(|e| format!("Failed to launch game: {}", e))?;
    } else {
        Command::new(&exe_path)
            .args(&args)
            .spawn()
            .map_err(|e| format!("Failed to launch game: {}", e))?;
    }

    Ok(())
}

// ─── SSO Authentication ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SSOUser {
    pub id: String,
    pub email: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub avatar_url: Option<String>,
    pub avatar_outer_color: String,
    pub avatar_inner_color: String,
    pub avatar_pan_x: f64,
    pub avatar_pan_y: f64,
    pub avatar_zoom: f64,
    pub created_at: String,
    pub last_sign_in: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerifyResponse {
    valid: bool,
    user: Option<SSOUser>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuthState {
    pub logged_in: bool,
    pub user: Option<SSOUser>,
}

const SSO_TIMEOUT: Duration = Duration::from_secs(120);

/// Generate a pseudo-random u64 using system time + thread ID + pointer entropy.
/// Not cryptographic, but sufficient for a local-only anti-injection nonce.
fn rand_u64() -> u64 {
    use std::time::SystemTime;
    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let thread_id = format!("{:?}", std::thread::current().id());
    let mut hasher = Sha256::new();
    hasher.update(time.to_le_bytes());
    hasher.update(thread_id.as_bytes());
    let hash = hasher.finalize();
    u64::from_le_bytes(hash[..8].try_into().unwrap())
}

#[tauri::command]
pub async fn start_sso_login(app: AppHandle) -> Result<AuthState, String> {
    let store = get_store(&app);
    let sso_url = store_get_string(&store, "sso_url", AppSettings::default().sso_url);

    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Failed to bind: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get port: {}", e))?
        .port();

    // Generate a random nonce to prevent other local processes from injecting tokens
    let nonce = format!("{:016x}", rand_u64());

    let login_url = format!(
        "{}/login?app=siegeworlds&redirect=http://localhost:{}/callback",
        sso_url, port
    );

    app.emit("log", "Opening browser for sign in...".to_string())
        .map_err(|e| e.to_string())?;

    open::that(&login_url).map_err(|e| format!("Failed to open browser: {}", e))?;

    let token_result: Result<(String, String), String> =
        tokio::task::spawn_blocking(move || {
            listener
                .set_nonblocking(true)
                .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

            // Embed the nonce into the callback page so only our browser tab can submit it
            let callback_html = format!(r#"<!DOCTYPE html>
<html>
<head><title>Signing in...</title></head>
<body style="background:#1a112e;color:#fff;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0">
<div style="text-align:center"><h2>Signing in...</h2></div>
<script>
  var hash = window.location.hash.substring(1);
  var params = new URLSearchParams(hash);
  var token = params.get('access_token');
  if (token) {{
    fetch('/receive-token?state={}&token=' + encodeURIComponent(token) + '&refresh=' + encodeURIComponent(params.get('refresh_token') || ''))
      .then(function() {{
        document.querySelector('div').innerHTML = '<h2>Signed in!</h2><p>You can close this tab and return to the launcher.</p>';
      }});
  }} else {{
    document.querySelector('div').innerHTML = '<h2>Login failed</h2><p>No token received. Please try again.</p>';
  }}
</script>
</body>
</html>"#, nonce);

            let mut access_token = String::new();
            let mut refresh_token = String::new();
            let deadline = std::time::Instant::now() + SSO_TIMEOUT;

            loop {
                if std::time::Instant::now() > deadline {
                    return Err("Sign in timed out (2 minutes). Please try again.".to_string());
                }

                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(e) => return Err(format!("Failed to accept: {}", e)),
                };

                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                let mut stream = stream;
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let request = String::from_utf8_lossy(&buf[..n]).to_string();

                if request.contains("/receive-token") {
                    // Parse query params
                    let mut received_state = String::new();
                    if let Some(query_start) = request.find("/receive-token?") {
                        let query = &request[query_start + 15..];
                        let query = query.split_whitespace().next().unwrap_or("");
                        for param in query.split('&') {
                            let mut kv = param.splitn(2, '=');
                            let key = kv.next().unwrap_or("");
                            let val = kv.next().unwrap_or("");
                            match key {
                                "state" => {
                                    received_state =
                                        urlencoding::decode(val).unwrap_or_default().to_string()
                                }
                                "token" => {
                                    access_token =
                                        urlencoding::decode(val).unwrap_or_default().to_string()
                                }
                                "refresh" => {
                                    refresh_token =
                                        urlencoding::decode(val).unwrap_or_default().to_string()
                                }
                                _ => {}
                            }
                        }
                    }

                    // Reject if nonce doesn't match (prevents token injection from other processes)
                    if received_state != nonce {
                        let response = "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\n\r\nInvalid state";
                        let _ = stream.write_all(response.as_bytes());
                        access_token.clear();
                        refresh_token.clear();
                        continue;
                    }

                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nOK";
                    let _ = stream.write_all(response.as_bytes());
                    break;
                } else {
                    // Serve the callback page (initial browser request)
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                        callback_html.len(),
                        callback_html
                    );
                    let _ = stream.write_all(response.as_bytes());
                    // Don't break — keep listening for the /receive-token follow-up
                }
            }

            if access_token.is_empty() {
                return Err("No token received from SSO".to_string());
            }

            Ok((access_token, refresh_token))
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))?;

    let (access_token, refresh_token) = token_result?;

    let client = http_client(&app);
    let user = verify_token_internal(&client, &sso_url, &access_token).await?;

    let store = get_store(&app);
    store.set("access_token", serde_json::json!(&access_token));
    store.set("refresh_token", serde_json::json!(&refresh_token));
    let _ = store.save();

    app.emit(
        "log",
        format!("Signed in as {}", user.display_name),
    )
    .map_err(|e| e.to_string())?;

    Ok(AuthState {
        logged_in: true,
        user: Some(user),
    })
}

async fn verify_token_internal(
    client: &reqwest::Client,
    sso_url: &str,
    token: &str,
) -> Result<SSOUser, String> {
    let res = client
        .post(format!("{}/api/verify", sso_url))
        .json(&serde_json::json!({ "token": token }))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Invalid or expired token".to_string());
    }

    let body: VerifyResponse = res
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    if body.valid {
        body.user.ok_or_else(|| "No user data".to_string())
    } else {
        Err(body
            .error
            .unwrap_or_else(|| "Verification failed".to_string()))
    }
}

#[tauri::command]
pub async fn verify_token(app: AppHandle) -> Result<AuthState, String> {
    let store = get_store(&app);
    let sso_url = store_get_string(&store, "sso_url", AppSettings::default().sso_url);

    let token = store
        .get("access_token")
        .and_then(|v| v.as_str().map(|s| s.to_string()));

    let client = http_client(&app);
    match token {
        Some(t) if !t.is_empty() => match verify_token_internal(&client, &sso_url, &t).await {
            Ok(user) => Ok(AuthState {
                logged_in: true,
                user: Some(user),
            }),
            Err(_) => {
                store.delete("access_token");
                store.delete("refresh_token");
                let _ = store.save();
                Ok(AuthState {
                    logged_in: false,
                    user: None,
                })
            }
        },
        _ => Ok(AuthState {
            logged_in: false,
            user: None,
        }),
    }
}

#[tauri::command]
pub async fn get_stored_auth(app: AppHandle) -> AuthState {
    match verify_token(app).await {
        Ok(state) => state,
        Err(_) => AuthState {
            logged_in: false,
            user: None,
        },
    }
}

#[tauri::command]
pub fn logout(app: AppHandle) -> Result<(), String> {
    let store = get_store(&app);
    store.delete("access_token");
    store.delete("refresh_token");
    store.save().map_err(|e| format!("Failed to save: {}", e))
}

// ─── Dynamic Slideshow from Supabase ────────────────────────────────────────

const SUPABASE_URL: &str = "https://qprwdignwccmcnninnlv.supabase.co";
const SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InFwcndkaWdud2NjbWNubmlubmx2Iiwicm9sZSI6ImFub24iLCJpYXQiOjE3NzQ0NDc4NjMsImV4cCI6MjA5MDAyMzg2M30.wQtnwkxYLON61hRGuSbdfrzqXWrodpr9GDr59SwiNZ4";
const SUPABASE_BUCKET: &str = "launcher-assets";

/// Edge Function URL for admin storage operations.
/// The function verifies the SSO token, checks admin role, and performs
/// writes using the service role key (never exposed to the client).
const ADMIN_STORAGE_URL: &str = "https://qprwdignwccmcnninnlv.supabase.co/functions/v1/admin-storage";

/// Call the admin-storage Edge Function with the user's SSO token.
async fn admin_storage_call(
    app: &AppHandle,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let token = get_user_token(app).ok_or("Not signed in")?;
    let client = http_client(app);

    let res = client
        .post(ADMIN_STORAGE_URL)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = res.status();
    let response_body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    if !status.is_success() {
        let error = response_body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown error");
        return Err(format!("{}", error));
    }

    Ok(response_body)
}

// ─── Launcher Config (shared via Supabase) ──────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LauncherConfig {
    pub greeting: String,
}

/// Fetch the shared launcher config from Supabase.
#[tauri::command]
pub async fn fetch_launcher_config(app: AppHandle) -> LauncherConfig {
    let client = http_client(&app);
    let url = format!(
        "{}/storage/v1/object/public/{}/launcher-config.json",
        SUPABASE_URL, SUPABASE_BUCKET
    );

    match client.get(&url).send().await {
        Ok(res) if res.status().is_success() => {
            res.json::<LauncherConfig>().await.unwrap_or(LauncherConfig {
                greeting: "Launcher ready.".to_string(),
            })
        }
        _ => LauncherConfig {
            greeting: "Launcher ready.".to_string(),
        },
    }
}

/// Save the shared launcher config to Supabase via Edge Function (admin only).
#[tauri::command]
pub async fn save_launcher_config(app: AppHandle, config: LauncherConfig) -> Result<(), String> {
    admin_storage_call(&app, serde_json::json!({
        "action": "save-config",
        "config": config,
    })).await?;

    app.emit("log", "Greeting updated for all users.".to_string()).ok();
    Ok(())
}

/// Returns a list of slide image URLs, fetched from Supabase Storage.
/// Falls back to an empty list if the fetch fails (frontend uses bundled images).
#[tauri::command]
pub async fn fetch_slides(app: AppHandle) -> Vec<String> {
    match fetch_slides_internal(&app).await {
        Ok(urls) => urls,
        Err(e) => {
            app.emit("log", format!("Using bundled slides ({})", e)).ok();
            Vec::new()
        }
    }
}

async fn fetch_slides_internal(app: &AppHandle) -> Result<Vec<String>, String> {
    let client = http_client(&app);

    // List files in the launcher-assets bucket
    let list_url = format!(
        "{}/storage/v1/object/list/{}",
        SUPABASE_URL, SUPABASE_BUCKET
    );

    let res = client
        .post(&list_url)
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", SUPABASE_ANON_KEY))
        .json(&serde_json::json!({
            "prefix": "",
            "limit": 100,
            "sortBy": { "column": "name", "order": "asc" }
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("Supabase returned {}", res.status()));
    }

    let files: Vec<serde_json::Value> = res
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    // Filter for image files and build public URLs
    let image_extensions = ["jpg", "jpeg", "png", "webp"];
    let mut urls: Vec<String> = Vec::new();

    for file in &files {
        if let Some(name) = file.get("name").and_then(|n| n.as_str()) {
            let lower = name.to_lowercase();
            if image_extensions.iter().any(|ext| lower.ends_with(ext)) {
                let url = format!(
                    "{}/storage/v1/object/public/{}/{}",
                    SUPABASE_URL, SUPABASE_BUCKET, name
                );
                urls.push(url);
            }
        }
    }

    if urls.is_empty() {
        return Err("No slide images found in bucket".to_string());
    }

    // Check for ordering file
    let order_url = format!(
        "{}/storage/v1/object/public/{}/slide-order.json",
        SUPABASE_URL, SUPABASE_BUCKET
    );
    if let Ok(order_res) = client.get(&order_url).send().await {
        if order_res.status().is_success() {
            if let Ok(order) = order_res.json::<Vec<String>>().await {
                // Reorder urls based on the order list
                let mut ordered: Vec<String> = Vec::new();
                for name in &order {
                    if let Some(url) = urls.iter().find(|u| u.ends_with(name)) {
                        ordered.push(url.clone());
                    }
                }
                // Add any urls not in the order file at the end
                for url in &urls {
                    if !ordered.contains(url) {
                        ordered.push(url.clone());
                    }
                }
                urls = ordered;
            }
        }
    }

    // Cache the images locally for offline use
    let cache_dir = cache_slides_dir();
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        app.emit("log", format!("Warning: can't create cache dir: {}", e)).ok();
        return Ok(urls);
    }

    // Build set of current remote filenames for pruning
    let remote_filenames: HashSet<String> = urls
        .iter()
        .filter_map(|u| u.rsplit('/').next().map(|s| s.to_string()))
        .collect();

    for url in &urls {
        let filename = url.rsplit('/').next().unwrap_or("slide.jpg");
        let cache_path = cache_dir.join(filename);
        // Only download if not already cached
        if !cache_path.exists() {
            if let Ok(resp) = client.get(url).send().await {
                if let Ok(bytes) = resp.bytes().await {
                    let _ = std::fs::write(&cache_path, &bytes);
                }
            }
        }
    }

    // Prune cached files that are no longer in the remote slide list
    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if !remote_filenames.contains(name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    Ok(urls)
}

/// Returns cached slide file paths for offline fallback.
#[tauri::command]
pub fn get_cached_slides() -> Vec<String> {
    let cache_dir = cache_slides_dir();
    if !cache_dir.exists() {
        return Vec::new();
    }

    let image_extensions = ["jpg", "jpeg", "png", "webp"];
    let mut paths: Vec<String> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if image_extensions.contains(&ext.to_lowercase().as_str()) {
                    paths.push(path.to_string_lossy().to_string());
                }
            }
        }
    }

    paths.sort();
    paths
}

fn cache_slides_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("siege-worlds-launcher")
        .join("slides")
}

/// Save slide ordering to Supabase via Edge Function (admin only).
#[tauri::command]
pub async fn save_slide_order(app: AppHandle, order: Vec<String>) -> Result<(), String> {
    admin_storage_call(&app, serde_json::json!({
        "action": "save-order",
        "order": order,
    })).await?;

    app.emit("log", "Slide order saved".to_string()).ok();
    Ok(())
}

/// Upload a slide image via Edge Function (admin only).
#[tauri::command]
pub async fn upload_slide(
    app: AppHandle,
    filename: String,
    data: Vec<u8>,
) -> Result<String, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);

    let result = admin_storage_call(&app, serde_json::json!({
        "action": "upload",
        "filename": filename,
        "data": b64,
    })).await?;

    let public_url = result
        .get("url")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!(
            "{}/storage/v1/object/public/{}/{}",
            SUPABASE_URL, SUPABASE_BUCKET, filename
        ));

    app.emit("log", format!("Uploaded: {}", filename)).ok();
    Ok(public_url)
}

/// Delete a slide image via Edge Function (admin only).
#[tauri::command]
pub async fn delete_slide(app: AppHandle, filename: String) -> Result<(), String> {
    admin_storage_call(&app, serde_json::json!({
        "action": "delete",
        "filename": filename,
    })).await?;

    // Remove from local cache too
    let cache_path = cache_slides_dir().join(&filename);
    let _ = std::fs::remove_file(&cache_path);

    app.emit("log", format!("Deleted: {}", filename)).ok();
    Ok(())
}
