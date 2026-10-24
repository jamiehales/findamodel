use rand::RngCore;
use serde::Serialize;
use std::io::Error as IoError;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
    backend: Arc<Mutex<Option<findamodel::ServerHandle>>>,
}

#[tauri::command]
fn desktop_runtime_config(state: State<DesktopRuntimeState>) -> DesktopRuntimeConfig {
    state.config.clone()
}

fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
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

/// Starts the backend inside this process on a free localhost port.
fn start_embedded_backend(
    app_data_dir: std::path::PathBuf,
) -> Result<(findamodel::ServerHandle, String, String), String> {
    let log_buffer = Arc::new(findamodel::LogBuffer::new(2000));
    findamodel::logging::init(log_buffer.clone());

    let token = generate_session_token();
    let mut settings = findamodel::Settings::local(app_data_dir, std::env::var("FINDAMODEL_MODELS_PATH").ok());
    settings.mode = findamodel::config::RuntimeMode::Desktop;
    settings.listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    settings.desktop_token = Some(token.clone());
    settings.preview_use_gpu = true;

    let handle =
        tauri::async_runtime::block_on(findamodel::start(settings, log_buffer)).map_err(|e| format!("{e:#}"))?;
    let url = format!("http://{}", handle.address);
    Ok((handle, url, token))
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // When FINDAMODEL_EXTERNAL_BACKEND_URL is set (e.g. from a VS Code debug launch),
            // the shell attaches to an already-running backend instead of starting its own.
            let external_url = std::env::var("FINDAMODEL_EXTERNAL_BACKEND_URL").ok();
            let external_token = std::env::var("FINDAMODEL_EXTERNAL_BACKEND_TOKEN").ok();

            let (api_base_url, session_token, backend) =
                if let (Some(url), Some(token)) = (external_url, external_token) {
                    if let Err(error) = wait_for_health(&url, &token) {
                        return Err(IoError::other(format!("External backend not healthy: {error}")).into());
                    }
                    (url, token, None)
                } else {
                    let app_data_dir = app.path().app_data_dir().map_err(IoError::other)?;
                    std::fs::create_dir_all(&app_data_dir).map_err(IoError::other)?;
                    let (handle, url, token) = start_embedded_backend(app_data_dir)
                        .map_err(|e| IoError::other(format!("Backend failed to start: {e}")))?;
                    (url, token, Some(handle))
                };

            app.manage(DesktopRuntimeState {
                config: DesktopRuntimeConfig {
                    api_base_url,
                    mode: "desktop".to_string(),
                    desktop_session_token: session_token,
                },
                backend: Arc::new(Mutex::new(backend)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![desktop_runtime_config])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app.try_state::<DesktopRuntimeState>()
                && let Ok(mut backend) = state.backend.lock()
                && let Some(handle) = backend.take()
            {
                handle.shutdown.cancel();
                let _ = tauri::async_runtime::block_on(handle.task);
            }
        });
}
