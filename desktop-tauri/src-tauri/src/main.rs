use rand::RngCore;
use serde::Serialize;
use std::io::Error as IoError;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Manager, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopRuntimeConfig {
    api_base_url: String,
    mode: String,
    desktop_session_token: String,
}

struct DesktopRuntimeState {
    config: DesktopRuntimeConfig,
    backend_process: Arc<Mutex<Option<Child>>>,
}

#[tauri::command]
fn desktop_runtime_config(state: State<DesktopRuntimeState>) -> DesktopRuntimeConfig {
    state.config.clone()
}

fn pick_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|e| e.to_string())
}

fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
}

fn find_backend_executable(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let file_name = if cfg!(target_os = "windows") {
        "findamodel-backend.exe"
    } else {
        "findamodel-backend"
    };

    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("bin")
        .join(file_name);

    if bundled.exists() {
        return Ok(bundled);
    }

    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin").join(file_name);
    if dev_path.exists() {
        return Ok(dev_path);
    }

    Err(format!("Backend sidecar not found: {}", bundled.display()))
}

fn wait_for_health(api_base_url: &str, session_token: &str) -> Result<(), String> {
    let health_url = format!("{api_base_url}/health");
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last_error = String::from("backend did not report health");

    while Instant::now() < deadline {
        match ureq::get(&health_url)
            .set("X-Findamodel-Desktop-Token", session_token)
            .timeout(Duration::from_secs(2))
            .call()
        {
            Ok(response) if response.status() == 200 => return Ok(()),
            Ok(response) => {
                last_error = format!("health endpoint returned HTTP {}", response.status());
            }
            Err(err) => {
                last_error = err.to_string();
            }
        }

        thread::sleep(Duration::from_millis(250));
    }

    Err(last_error)
}

fn terminate_backend(process: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut guard) = process.lock() {
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *guard = None;
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // When FINDAMODEL_EXTERNAL_BACKEND_URL is set (e.g. from VS Code desktop debug launch),
            // Tauri skips spawning its own sidecar and connects to the already-running backend.
            let external_url = std::env::var("FINDAMODEL_EXTERNAL_BACKEND_URL").ok();
            let external_token = std::env::var("FINDAMODEL_EXTERNAL_BACKEND_TOKEN").ok();

            let (api_base_url, session_token, backend_process) =
                if let (Some(url), Some(token)) = (external_url, external_token) {
                    // External backend — wait for it to be healthy then attach.
                    if let Err(error) = wait_for_health(&url, &token) {
                        return Err(IoError::other(format!(
                            "External backend not healthy: {error}"
                        ))
                        .into());
                    }
                    (url, token, Arc::new(Mutex::new(None)))
                } else {
                    // Managed sidecar — pick port, generate token, spawn backend.
                    let port = pick_port().map_err(IoError::other)?;
                    let token = generate_session_token();
                    let url = format!("http://127.0.0.1:{port}");

                    let app_data_dir = app
                        .path()
                        .app_data_dir()
                        .map_err(IoError::other)?;
                    std::fs::create_dir_all(&app_data_dir).map_err(IoError::other)?;

                    let backend_executable =
                        find_backend_executable(&app.handle()).map_err(IoError::other)?;

                    let mut child = Command::new(&backend_executable)
                        .env("FINDAMODEL_MODE", "desktop")
                        .env("FINDAMODEL_URL", &url)
                        .env("FINDAMODEL_DATA_PATH", app_data_dir.to_string_lossy().to_string())
                        .env("FINDAMODEL_DISABLE_CORS", "true")
                        .env("FINDAMODEL_DESKTOP_SESSION_TOKEN", &token)
                        .spawn()
                        .map_err(IoError::other)?;

                    if let Err(error) = wait_for_health(&url, &token) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(IoError::other(format!(
                            "Backend failed to become healthy: {error}"
                        ))
                        .into());
                    }

                    (url, token, Arc::new(Mutex::new(Some(child))))
                };

            let state = DesktopRuntimeState {
                config: DesktopRuntimeConfig {
                    api_base_url,
                    mode: "desktop".to_string(),
                    desktop_session_token: session_token,
                },
                backend_process,
            };

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![desktop_runtime_config])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<DesktopRuntimeState>() {
                    terminate_backend(&state.backend_process);
                }
            }
        });
}
