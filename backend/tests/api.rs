//! End-to-end API tests against the real router with a temporary database and models
//! directory.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use findamodel::app::AppState;
use findamodel::config::{RuntimeMode, Settings};
use findamodel::geometry::{Triangle, Vec3};
use findamodel::{LogBuffer, routes};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const STL: &str = "solid t
  facet normal 0 0 1
    outer loop
      vertex 0 0 0
      vertex 10 0 0
      vertex 0 10 0
    endloop
  endfacet
  facet normal 0 0 1
    outer loop
      vertex 0 0 0
      vertex 0 10 0
      vertex 0 0 10
    endloop
  endfacet
endsolid t
";

struct TestApp {
    router: Router,
    state: AppState,
    _data: tempfile::TempDir,
    models: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        Self::with(|_| {}).await
    }

    async fn with(configure: impl FnOnce(&mut Settings)) -> Self {
        let data = tempfile::tempdir().unwrap();
        let models = tempfile::tempdir().unwrap();
        let mut settings = Settings::local(data.path().to_path_buf(), Some(models.path().display().to_string()));
        configure(&mut settings);
        let state = AppState::initialize(settings, Arc::new(LogBuffer::new(500))).await.unwrap();
        TestApp { router: routes::router(state.clone()), state, _data: data, models }
    }

    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
        let path = self.models.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
        (status, headers, body)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let (status, _, body) = self.send(Request::get(uri).body(Body::empty()).unwrap()).await;
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    async fn json(&self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _, bytes) = self.send(request).await;
        let parsed = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, parsed)
    }

    async fn index_and_wait(&self, flags: i32) {
        let (status, request) = self.json("POST", "/api/indexer", json!({ "flags": flags })).await;
        assert_eq!(status, StatusCode::OK);
        let run_id = request["runId"].as_str().unwrap().to_string();
        for _ in 0..400 {
            let (_, status) = self.get("/api/indexer").await;
            let finished = status["recent"].as_array().unwrap().iter().any(|r| r["runId"] == run_id.as_str());
            if finished {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("index run did not finish");
    }
}

#[tokio::test]
async fn health_and_current_user() {
    let app = TestApp::new().await;
    assert_eq!(app.get("/health").await, (StatusCode::OK, json!({ "status": "ok" })));
    let (status, me) = app.get("/api/users/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["username"], "admin");
    assert_eq!(me["isAdmin"], true);
}

#[tokio::test]
async fn settings_config_round_trip_and_validation() {
    let app = TestApp::new().await;
    let (status, config) = app.get("/api/settings/config").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["defaultRaftHeightMm"], 2.0);
    assert_eq!(config["setupCompleted"], true);

    let mut update = json!({
        "defaultRaftHeightMm": 3.5,
        "generatePreviewsEnabled": false,
        "minimumPreviewGenerationVersion": 1,
        "minimumSupportExportVersion": 0,
        "repairedMeshCacheSizeMb": 2048,
        "repairThreads": 8
    });
    let (status, updated) = app.json("PUT", "/api/settings/config", update.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(updated.get("theme").is_none());
    assert_eq!(updated["generatePreviewsEnabled"], false);
    assert_eq!(updated["repairedMeshCacheSizeMb"], 2048);
    assert_eq!(updated["repairThreads"], 8);
    assert_eq!(updated["minimumSupportExportVersion"], 0);
    let (_, stats) = app.get("/api/settings/stats").await;
    let current = findamodel::geometry::support_reader::CURRENT_SUPPORT_READER_VERSION;
    assert_eq!(stats["supportExportVersion"], current);

    let mut too_new = update.clone();
    too_new["minimumSupportExportVersion"] = json!(current + 1);
    let (status, _) = app.json("PUT", "/api/settings/config", too_new).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut no_cache = update.clone();
    no_cache["repairedMeshCacheSizeMb"] = json!(0);
    let (status, _) = app.json("PUT", "/api/settings/config", no_cache).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    for threads in [0, 257] {
        let mut bad_threads = update.clone();
        bad_threads["repairThreads"] = json!(threads);
        let (status, _) = app.json("PUT", "/api/settings/config", bad_threads).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{threads} threads");
    }

    update["defaultRaftHeightMm"] = json!(-1);
    let (status, _) = app.json("PUT", "/api/settings/config", update).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, setup) = app.get("/api/settings/setup-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(setup, json!({ "setupCompleted": true, "requiresWizard": false }));
}

#[tokio::test]
async fn initial_setup_sets_models_directory() {
    let app = TestApp::with(|s| s.models_path = None).await;
    let (_, setup) = app.get("/api/settings/setup-status").await;
    assert_eq!(setup["requiresWizard"], true);

    let (status, message) = app
        .json("POST", "/api/settings/setup", json!({ "modelsDirectoryPath": "/definitely/missing", "defaultRaftHeightMm": 2, "generatePreviewsEnabled": true }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message.as_str().unwrap().contains("does not exist"));

    let models_path = app.models.path().display().to_string();
    let (status, config) = app
        .json(
            "POST",
            "/api/settings/setup",
            json!({ "modelsDirectoryPath": models_path, "defaultRaftHeightMm": 2, "generatePreviewsEnabled": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["setupCompleted"], true);
    assert_eq!(app.state.models_root().unwrap(), Path::new(&models_path));
    let (status, _) = app.get("/api/explorer").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn printers_and_printing_lists() {
    let app = TestApp::new().await;
    let (_, printers) = app.get("/api/printers").await;
    assert_eq!(printers.as_array().unwrap().len(), 2);

    let (status, printer) =
        app.json("POST", "/api/printers", json!({ "name": "Saturn", "bedWidthMm": 218, "bedDepthMm": 123 })).await;
    assert_eq!(status, StatusCode::OK);
    let printer_id = printer["id"].as_str().unwrap().to_string();
    let (status, _) = app.json("POST", &format!("/api/printers/{printer_id}/set-default"), json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, message) = app.json("DELETE", &format!("/api/printers/{printer_id}"), json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(message.as_str().unwrap().contains("default printer"));

    let (_, active) = app.get("/api/printing-lists/active").await;
    assert_eq!(active["name"], "Default");
    assert_eq!(active["isDefault"], true);

    let request = Request::post("/api/printing-lists")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "name": "  Batch  " }).to_string()))
        .unwrap();
    let (status, headers, body) = app.send(request).await;
    assert_eq!(status, StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(created["name"], "Batch");
    let list_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(headers[header::LOCATION], format!("/api/printing-lists/{list_id}"));

    let model_id = uuid::Uuid::new_v4();
    let (status, detail) =
        app.json("PUT", &format!("/api/printing-lists/active/items/{model_id}"), json!({ "quantity": 3 })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["items"][0]["quantity"], 3);
    let (status, detail) = app
        .json(
            "PUT",
            &format!("/api/printing-lists/{list_id}/settings"),
            json!({ "spawnType": "RANDOM", "hullMode": "sansraft" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((detail["spawnType"].as_str(), detail["hullMode"].as_str()), (Some("random"), Some("sansRaft")));

    let (status, _) = app.get("/api/printing-lists/not-a-guid").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app
        .json("PUT", &format!("/api/printing-lists/{}", active["id"].as_str().unwrap()), json!({ "name": "x" }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn index_query_preview_geometry_and_plate_export() {
    let app = TestApp::new().await;
    app.write("findamodel.yaml", "creator: Alice\ntags:\n  - fantasy\n");
    app.write("Fantasy/dragon.stl", STL);
    app.write("Fantasy/notes.md", "hello");
    app.write("Fantasy/print.ctb", [1u8, 2, 3]);

    app.index_and_wait(3).await;

    let (status, result) = app.get("/api/query?tags=FANTASY&fileType=stl&limit=10").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["totalCount"], 1);
    let model = &result["models"][0];
    assert_eq!(model["name"], "dragon");
    assert_eq!(model["creator"], "Alice");
    assert_eq!(model["relativePath"], "Fantasy/dragon.stl");
    assert!(model["convexHull"].is_null(), "search results leave hulls out");
    let id = model["id"].as_str().unwrap().to_string();

    let (status, detail) = app.get(&format!("/api/models/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(detail["convexHull"].as_str().unwrap().starts_with("[["));
    let (status, listed) = app.json("POST", "/api/models/by-ids", json!({ "ids": [id] })).await;
    assert_eq!(status, StatusCode::OK);
    assert!(listed[0]["convexHull"].as_str().unwrap().starts_with("[["));

    let (_, options) = app.get("/api/query/options?creator=Alice").await;
    assert_eq!(options["fileTypes"], json!(["ctb", "stl"]));
    assert_eq!(options["creators"], json!(["Alice"]));

    let preview_url = model["previewUrl"].as_str().unwrap();
    let (status, headers, png) = app.send(Request::get(preview_url).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    assert_eq!(&png[1..4], b"PNG");

    let request = Request::get(format!("/api/models/{id}/geometry"))
        .header(header::ACCEPT, "application/vnd.findamodel.mesh")
        .body(Body::empty())
        .unwrap();
    let (status, headers, mesh) = app.send(request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/vnd.findamodel.mesh");
    assert_eq!(u32::from_le_bytes(mesh[12..16].try_into().unwrap()), 2);

    let (status, legacy) = app.get(&format!("/api/models/{id}/geometry?format=json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(legacy["triangleCount"], 2);
    assert_eq!(legacy["positions"].as_array().unwrap().len(), 18);

    let (status, _, _) =
        app.send(Request::get(format!("/api/models/{id}/geometry/split")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, headers, file) =
        app.send(Request::get(format!("/api/models/{id}/file")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "model/stl");
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().contains("dragon.stl"));
    assert_eq!(file, STL.as_bytes());

    let (_, explorer) = app.get("/api/explorer?path=Fantasy").await;
    assert_eq!(explorer["models"].as_array().unwrap().len(), 2);
    assert_eq!(explorer["files"][0]["fileName"], "notes.md");
    let (status, _, text) =
        app.send(Request::get("/api/explorer/file?path=Fantasy/notes.md").body(Body::empty()).unwrap()).await;
    assert_eq!((status, text.as_slice()), (StatusCode::OK, b"hello".as_slice()));
    let (status, _) = app.get("/api/explorer/file?path=../secret.md").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let ctb_id = explorer["models"].as_array().unwrap().iter().find(|m| m["fileName"] == "print.ctb").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let plate = json!({ "placements": [
        { "modelId": id, "instanceIndex": 0, "xMm": 0, "yMm": 0, "angleRad": 0 },
        { "modelId": id, "instanceIndex": 1, "xMm": 30, "yMm": 10, "angleRad": 1.57 },
        { "modelId": ctb_id, "instanceIndex": 0, "xMm": 0, "yMm": 0, "angleRad": 0 }
    ], "format": "stl" });
    let request = Request::post("/api/plate/generate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(plate.to_string()))
        .unwrap();
    let (status, headers, stl) = app.send(request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-plate-skipped-models"], "print.ctb");
    assert_eq!(u32::from_le_bytes(stl[80..84].try_into().unwrap()), 4);

    let mut voxl_plate = plate.clone();
    voxl_plate["format"] = json!("voxl");
    voxl_plate["bedWidthMm"] = json!(200);
    voxl_plate["bedDepthMm"] = json!(120);
    voxl_plate["plateName"] = json!("Dragons");
    let request = Request::post("/api/plate/generate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(voxl_plate.to_string()))
        .unwrap();
    let (status, headers, voxl) = app.send(request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/vnd.dragonfruit.voxl");
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().contains("plate.voxl"));
    assert_eq!(&voxl[0..4], b"VOXL");
    // Two placements of one model share a MESH chunk, which raises the container version.
    assert_eq!(u16::from_le_bytes(voxl[4..6].try_into().unwrap()), 3);

    let (status, message) = app
        .json("POST", "/api/plate/generate", json!({ "placements": [{ "modelId": "nope", "xMm": 0, "yMm": 0 }] }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(message, "Invalid model ID: nope");

    let (_, runs) = app.get("/api/indexer/runs").await;
    let run_id = runs[0]["id"].as_str().unwrap();
    let (status, detail) = app.get(&format!("/api/indexer/runs/{run_id}?filesView=processed")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["files"]["totalCount"], 2);
}

#[tokio::test]
async fn directory_config_update_and_validation() {
    let app = TestApp::new().await;
    app.write("A/model.stl", STL);
    app.index_and_wait(1).await;

    let (status, detail) =
        app.json("PUT", "/api/explorer/config?path=A", json!({ "creator": "Bob", "tags": ["x"] })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["localValues"]["creator"], "Bob");
    assert_eq!(detail["parentPath"], "");

    let (status, errors) =
        app.json("PUT", "/api/explorer/config?path=A", json!({ "fieldRules": { "creator": "rule: magic" } })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(errors["fieldErrors"]["creator"].as_str().unwrap().contains("magic"));

    let (status, _) = app.json("PUT", "/api/explorer/config?path=Missing", json!({ "creator": "x" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn desktop_mode_requires_session_token() {
    let app = TestApp::with(|s| {
        s.mode = RuntimeMode::Desktop;
        s.desktop_token = Some("secret".into());
    })
    .await;
    assert_eq!(app.get("/health").await.0, StatusCode::OK);
    assert_eq!(app.get("/api/users/me").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.get("/api/users/me?desktopToken=secret").await.0, StatusCode::OK);
    let request =
        Request::get("/api/users/me").header("X-Findamodel-Desktop-Token", "secret").body(Body::empty()).unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::OK);
}

#[tokio::test]
async fn desktop_mode_allows_the_webview_origin() {
    let app = TestApp::with(|s| {
        s.mode = RuntimeMode::Desktop;
        s.desktop_token = Some("secret".into());
    })
    .await;

    // The webview's preflight for a token-carrying request is answered without the token.
    for origin in ["http://tauri.localhost", "tauri://localhost"] {
        let preflight = Request::builder()
            .method("OPTIONS")
            .uri("/health")
            .header("Origin", origin)
            .header("Access-Control-Request-Method", "GET")
            .header("Access-Control-Request-Headers", "x-findamodel-desktop-token")
            .body(Body::empty())
            .unwrap();
        let (status, headers, _) = app.send(preflight).await;
        assert!(status.is_success(), "{origin}: {status}");
        assert_eq!(headers.get("access-control-allow-origin").unwrap(), origin);
    }

    let request = Request::get("/api/users/me")
        .header("Origin", "http://tauri.localhost")
        .header("X-Findamodel-Desktop-Token", "secret")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = app.send(request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("access-control-allow-origin").unwrap(), "http://tauri.localhost");

    // Other sites on the machine still can't read the desktop backend's responses.
    let request = Request::get("/health").header("Origin", "http://evil.example").body(Body::empty()).unwrap();
    let (_, headers, _) = app.send(request).await;
    assert!(headers.get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn logs_and_stats() {
    let app = TestApp::new().await;
    let (status, logs) = app.get("/api/settings/logs?severity=Warning&limit=10").await;
    assert_eq!(status, StatusCode::OK);
    assert!(logs["availableSeverities"].as_array().unwrap().contains(&json!("Information")));
    let (status, _) = app.get("/api/settings/logs?severity=loud").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, stats) = app.get("/api/settings/stats").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["previewGpuEnabled"], false);
    assert_eq!(stats["printingListCount"], 1);
}

#[tokio::test]
async fn serves_frontend_with_spa_fallback() {
    let web = tempfile::tempdir().unwrap();
    std::fs::write(web.path().join("index.html"), "<html>app</html>").unwrap();
    std::fs::write(web.path().join("app.js"), "console.log(1)").unwrap();
    let web_root = web.path().to_path_buf();
    let app = TestApp::with(move |s| s.web_root = Some(web_root)).await;

    let (status, _, body) = app.send(Request::get("/").body(Body::empty()).unwrap()).await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"<html>app</html>".as_slice()));
    let (status, _, body) = app.send(Request::get("/models/some/deep/link").body(Body::empty()).unwrap()).await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"<html>app</html>".as_slice()));
    let (status, _, body) = app.send(Request::get("/app.js").body(Body::empty()).unwrap()).await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"console.log(1)".as_slice()));
}

/// Closed box split into an n x n grid per face, wound outward. Z-up.
fn cuboid(min: [f32; 3], max: [f32; 3], n: usize) -> Vec<Triangle> {
    let faces = [
        (2, min[2], 1, 0),
        (2, max[2], 0, 1),
        (0, min[0], 2, 1),
        (0, max[0], 1, 2),
        (1, min[1], 0, 2),
        (1, max[1], 2, 0),
    ];
    let mut out = Vec::new();
    for (axis, value, u, v) in faces {
        let point = |i: usize, j: usize| {
            let mut p = [0.0; 3];
            p[axis] = value;
            p[u] = min[u] + (max[u] - min[u]) * i as f32 / n as f32;
            p[v] = min[v] + (max[v] - min[v]) * j as f32 / n as f32;
            Vec3::new(p[0], p[1], p[2])
        };
        for i in 0..n {
            for j in 0..n {
                out.push(Triangle::new(point(i, j), point(i + 1, j), point(i + 1, j + 1), Vec3::new(0.0, 0.0, 1.0)));
                out.push(Triangle::new(point(i, j), point(i + 1, j + 1), point(i, j + 1), Vec3::new(0.0, 0.0, 1.0)));
            }
        }
    }
    out
}

/// Closed vertical frustum from `bottom` (radius `rb`) up to `top` (radius `rt`).
fn post(x: f32, y: f32, bottom: f32, top: f32, rb: f32, rt: f32) -> Vec<Triangle> {
    let ring = |z: f32, r: f32, k: usize| {
        let t = std::f32::consts::TAU * k as f32 / 12.0;
        Vec3::new(x + r * t.cos(), y + r * t.sin(), z)
    };
    let (b, t) = (Vec3::new(x, y, bottom), Vec3::new(x, y, top));
    let up = Vec3::new(0.0, 0.0, 1.0);
    (0..12)
        .flat_map(|k| {
            let (p0, p1, q0, q1) =
                (ring(bottom, rb, k), ring(bottom, rb, k + 1), ring(top, rt, k), ring(top, rt, k + 1));
            [
                Triangle::new(p0, p1, q1, up),
                Triangle::new(p0, q1, q0, up),
                Triangle::new(b, p1, p0, up),
                Triangle::new(t, q0, q1, up),
            ]
        })
        .collect()
}

/// A block standing on four posts with tapered tips.
fn presupported_knight() -> Vec<Triangle> {
    let mut mesh = cuboid([-5.0, -5.0, 10.0], [5.0, 5.0, 15.0], 12);
    for (x, y) in [(-3.0, -3.0), (3.0, -3.0), (-3.0, 3.0), (3.0, 3.0)] {
        mesh.extend(post(x, y, 0.0, 8.0, 0.4, 0.4));
        mesh.extend(post(x, y, 8.0, 10.2, 0.4, 0.12));
    }
    mesh
}

#[tokio::test]
async fn presupported_model_downloads_as_voxl_with_its_supports() {
    let app = TestApp::new().await;
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&presupported_knight(), None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.write("Mini/plain.stl", STL);
    app.index_and_wait(3).await;

    let (_, result) = app.get("/api/query?limit=10").await;
    let id_of = |name: &str| {
        result["models"].as_array().unwrap().iter().find(|m| m["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (knight, plain) = (id_of("knight"), id_of("plain"));
    let (_, detail) = app.get(&format!("/api/models/{knight}")).await;
    assert_eq!(detail["supportSplit"], true);

    let (status, headers, bytes) =
        app.send(Request::get(format!("/api/models/{knight}/voxl")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/vnd.dragonfruit.voxl");
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().contains("knight.voxl"));
    assert_eq!(&bytes[..4], b"VOXL");
    let supports = voxl_chunk(&bytes, b"SUPP");
    assert_eq!(supports["roots"].as_array().unwrap().len(), 4);
    assert_eq!(supports["trunks"].as_array().unwrap().len(), 4);
    let models = voxl_chunk(&bytes, b"MODL");
    assert_eq!(models[0]["polygonCount"], 12 * 12 * 12, "the model is stored without its supports");
    for trunk in supports["trunks"].as_array().unwrap() {
        assert_eq!(trunk["modelId"], models[0]["id"]);
        let contact = &trunk["contactCone"]["pos"];
        assert!((contact["z"].as_f64().unwrap() - 10.0).abs() < 0.1, "{contact}");
    }

    let (status, _, _) = app.send(Request::get(format!("/api/models/{plain}/voxl")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, detail) = app.get(&format!("/api/models/{plain}")).await;
    assert_eq!(detail["supportSplit"], false);

    // A detection from before the current scan config is checked again when split.
    forget_support_detection(&app).await;
    let (status, _, _) =
        app.send(Request::get(format!("/api/models/{knight}/geometry/split")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, detail) = app.get(&format!("/api/models/{knight}")).await;
    assert_eq!(detail["supportSplit"], true);
}

#[tokio::test]
async fn a_plate_voxl_rebuilds_the_supports_of_presupported_models() {
    let app = TestApp::new().await;
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&presupported_knight(), None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.write("Mini/plain.stl", STL);
    app.index_and_wait(3).await;
    let (_, result) = app.get("/api/query?limit=10").await;
    let id_of = |name: &str| {
        result["models"].as_array().unwrap().iter().find(|m| m["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let plate = json!({ "placements": [
        { "modelId": id_of("knight"), "instanceIndex": 0, "xMm": 0, "yMm": 0, "angleRad": 0 },
        { "modelId": id_of("knight"), "instanceIndex": 1, "xMm": 40, "yMm": 0, "angleRad": 0 },
        { "modelId": id_of("plain"), "instanceIndex": 0, "xMm": -40, "yMm": 0, "angleRad": 0 }
    ], "format": "voxl", "bedWidthMm": 200, "bedDepthMm": 120 });
    let request = Request::post("/api/plate/generate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(plate.to_string()))
        .unwrap();
    let (status, _, voxl) = app.send(request).await;
    assert_eq!(status, StatusCode::OK);
    let models = voxl_chunk(&voxl, b"MODL");
    let knights: Vec<&Value> =
        models.as_array().unwrap().iter().filter(|m| m["polygonCount"] == 12 * 12 * 12).collect();
    assert_eq!(knights.len(), 2, "both knights are stored without their supports: {models}");
    let supports = voxl_chunk(&voxl, b"SUPP");
    let trunks = supports["trunks"].as_array().unwrap();
    assert_eq!(trunks.len(), 8, "four rebuilt supports under each knight");
    for knight in knights {
        assert_eq!(trunks.iter().filter(|t| t["modelId"] == knight["id"]).count(), 4);
    }
}

/// An indexed pre-supported knight: the app and the knight's id.
async fn indexed_knight(app: &TestApp) -> String {
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&presupported_knight(), None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.index_and_wait(3).await;
    let (_, result) = app.get("/api/query?limit=10").await;
    result["models"][0]["id"].as_str().unwrap().to_string()
}

async fn plate_voxl(app: &TestApp, id: &str) -> Vec<u8> {
    let plate = json!({ "placements": [
        { "modelId": id, "instanceIndex": 0, "xMm": 0, "yMm": 0, "angleRad": 0 }
    ], "format": "voxl", "bedWidthMm": 200, "bedDepthMm": 120 });
    let request = Request::post("/api/plate/generate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(plate.to_string()))
        .unwrap();
    let (status, _, voxl) = app.send(request).await;
    assert_eq!(status, StatusCode::OK);
    voxl
}

/// The export entry files in the store.
fn entry_files(app: &TestApp) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(app.state.settings.support_exports_path()) else { return out };
    for dir in dirs.flatten() {
        out.extend(std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.path()));
    }
    out
}

/// Replaces every stored entry's supports with none, so an export that uses an entry shows it.
fn empty_entries(app: &TestApp) {
    use findamodel::services::support_exports::codec;
    for path in entry_files(app) {
        let (header, export) = codec::decode(&std::fs::read(&path).unwrap()).unwrap();
        let (mask, _) = export.expect("an entry with supports");
        let empty = findamodel::geometry::support_layout::SupportLayout::default();
        std::fs::write(&path, codec::encode(&header, Some((&mask, &empty)))).unwrap();
    }
}

fn trunk_count(voxl: &[u8]) -> usize {
    voxl_chunk(voxl, b"SUPP")["trunks"].as_array().unwrap().len()
}

#[tokio::test]
async fn indexing_stores_an_export_entry_that_plate_and_model_voxls_use() {
    let app = TestApp::new().await;
    let id = indexed_knight(&app).await;
    assert_eq!(entry_files(&app).len(), 1, "indexing wrote the knight's entry");
    let (path, version): (Option<String>, Option<i32>) =
        sqlx::query_as("SELECT support_export_path, support_export_version FROM models")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(path.is_some());
    assert_eq!(version, Some(findamodel::geometry::support_reader::CURRENT_SUPPORT_READER_VERSION));
    assert_eq!(trunk_count(&plate_voxl(&app, &id).await), 4);

    // Both exports read the entry rather than the mesh.
    empty_entries(&app);
    assert_eq!(trunk_count(&plate_voxl(&app, &id).await), 0);
    assert_eq!(trunk_count(&download_voxl(&app, &id).await), 0);
}

#[tokio::test]
async fn model_and_plate_voxls_write_the_entry_the_other_uses() {
    let app = TestApp::new().await;
    let id = indexed_knight(&app).await;

    // A model VOXL writes the entry a plate then uses.
    for path in entry_files(&app) {
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(trunk_count(&download_voxl(&app, &id).await), 4);
    assert_eq!(entry_files(&app).len(), 1);
    empty_entries(&app);
    assert_eq!(trunk_count(&plate_voxl(&app, &id).await), 0);

    // A plate writes the entry a model VOXL (not yet in the VOXL cache) then uses.
    for path in entry_files(&app) {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir_all(app.state.settings.voxl_cache_path()).unwrap();
    assert_eq!(trunk_count(&plate_voxl(&app, &id).await), 4);
    assert_eq!(entry_files(&app).len(), 1);
    empty_entries(&app);
    assert_eq!(trunk_count(&download_voxl(&app, &id).await), 0);
}

#[tokio::test]
async fn a_model_voxl_from_an_entry_still_records_that_supports_were_found() {
    let app = TestApp::new().await;
    let id = indexed_knight(&app).await;
    sqlx::query("UPDATE models SET support_parts_found = NULL").execute(&app.state.db).await.unwrap();
    assert_eq!(trunk_count(&download_voxl(&app, &id).await), 4);
    let (_, detail) = app.get(&format!("/api/models/{id}")).await;
    assert_eq!(detail["supportSplit"], true);

    // With a detection from before the current scan config, the split is checked again and its
    // entry (for an unchecked detection) written.
    forget_support_detection(&app).await;
    std::fs::remove_dir_all(app.state.settings.voxl_cache_path()).unwrap();
    assert_eq!(trunk_count(&download_voxl(&app, &id).await), 4);
    let (_, detail) = app.get(&format!("/api/models/{id}")).await;
    assert_eq!(detail["supportSplit"], true);
    // The unchecked detection has its own entry, which replaces the checked one the row named.
    assert_eq!(entry_files(&app).len(), 1);
}

#[tokio::test]
async fn voxl_downloads_cannot_evict_export_entries() {
    // A VOXL cache too small for any VOXL evicts everything it holds.
    let app = TestApp::with(|settings| settings.voxl_cache_max_bytes = 1).await;
    let id = indexed_knight(&app).await;
    for _ in 0..2 {
        download_voxl(&app, &id).await;
    }
    assert_eq!(entry_files(&app).len(), 1);
    assert!(!app.state.settings.support_exports_path().starts_with(app.state.settings.voxl_cache_path()));
}

#[tokio::test]
async fn model_voxls_are_cached_until_the_raft_height_changes() {
    let app = TestApp::new().await;
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&presupported_knight(), None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.index_and_wait(3).await;
    let (_, result) = app.get("/api/query?limit=10").await;
    let id = result["models"][0]["id"].as_str().unwrap().to_string();
    let cache_dir = app.state.settings.voxl_cache_path();
    let entries = || std::fs::read_dir(&cache_dir).map(|d| d.count()).unwrap_or(0);

    // Every VOXL written gets new scene ids and timestamps, so identical bytes were cached.
    let first = download_voxl(&app, &id).await;
    assert_eq!(entries(), 1);
    assert_eq!(download_voxl(&app, &id).await, first);

    sqlx::query("UPDATE models SET hull_raft_height_mm = 1.5").execute(&app.state.db).await.unwrap();
    let rebuilt = download_voxl(&app, &id).await;
    assert_ne!(rebuilt, first);
    assert_eq!(entries(), 2);
    assert_eq!(download_voxl(&app, &id).await, rebuilt);
}

#[tokio::test]
async fn model_voxls_are_not_cached_when_the_cache_size_is_zero() {
    let app = TestApp::with(|settings| settings.voxl_cache_max_bytes = 0).await;
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&presupported_knight(), None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.index_and_wait(3).await;
    let (_, result) = app.get("/api/query?limit=10").await;
    let id = result["models"][0]["id"].as_str().unwrap().to_string();

    assert_ne!(download_voxl(&app, &id).await, download_voxl(&app, &id).await);
    assert!(!app.state.settings.voxl_cache_path().exists());
}

async fn download_voxl(app: &TestApp, id: &str) -> Vec<u8> {
    let (status, _, bytes) =
        app.send(Request::get(format!("/api/models/{id}/voxl")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&bytes[..4], b"VOXL");
    bytes
}

#[tokio::test]
async fn model_parts_classed_as_supports_are_not_split_off() {
    let app = TestApp::new().await;
    // A kit part with thin straps along its side, in a folder marked pre-supported.
    let mut mesh = cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 10.0], 30);
    for z in [2.0, 5.0, 7.5] {
        mesh.extend(cuboid([5.0, -4.0, z], [6.0, 4.0, z + 1.0], 2));
    }
    app.write("Kit/hull.stl", findamodel::geometry::saver::save_stl(&mesh, None));
    app.write("Kit/findamodel.yaml", "supported: true\n");
    app.index_and_wait(3).await;

    let (_, result) = app.get("/api/query?limit=10").await;
    let id = result["models"][0]["id"].as_str().unwrap().to_string();
    let (_, detail) = app.get(&format!("/api/models/{id}")).await;
    assert_eq!(detail["supported"], true);
    assert_eq!(detail["supportSplit"], false);
    let (status, _, _) = app.send(Request::get(format!("/api/models/{id}/voxl")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) =
        app.send(Request::get(format!("/api/models/{id}/geometry/split")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    forget_support_detection(&app).await;
    let (_, detail) = app.get(&format!("/api/models/{id}")).await;
    assert_eq!(detail["supportSplit"], Value::Null);
    let (status, _, _) =
        app.send(Request::get(format!("/api/models/{id}/geometry/split")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, detail) = app.get(&format!("/api/models/{id}")).await;
    assert_eq!(detail["supportSplit"], false);
}

/// Makes every model's support detection look like it predates the current scan config.
async fn forget_support_detection(app: &TestApp) {
    sqlx::query("UPDATE models SET scan_config_checksum = NULL, support_parts_found = NULL")
        .execute(&app.state.db)
        .await
        .unwrap();
}

/// Decompresses a VOXL JSON chunk.
fn voxl_chunk(bytes: &[u8], tag: &[u8; 4]) -> Value {
    use std::io::Read;
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let entry = (0..u32_at(8)).map(|i| 16 + i * 20).find(|&base| &bytes[base..base + 4] == tag).unwrap();
    let (offset, size) = (u32_at(entry + 8), u32_at(entry + 12));
    let mut data = Vec::new();
    flate2::read::ZlibDecoder::new(&bytes[offset..offset + size]).read_to_end(&mut data).unwrap();
    serde_json::from_slice(&data).unwrap()
}

/// The knight with a second block crossing its body: a model mesh that is not watertight.
/// Finely tessellated (19,200 body faces), so the mesh chunk dominates the file size.
fn crossed_knight() -> Vec<Triangle> {
    let mut mesh = cuboid([-5.0, -5.0, 10.0], [5.0, 5.0, 15.0], 40);
    for (x, y) in [(-3.0, -3.0), (3.0, -3.0), (-3.0, 3.0), (3.0, 3.0)] {
        mesh.extend(post(x, y, 0.0, 8.0, 0.4, 0.4));
        mesh.extend(post(x, y, 8.0, 10.2, 0.4, 0.12));
    }
    // Off the body's grid, so the faces cross in general position.
    mesh.extend(cuboid([2.05, 2.05, 13.05], [7.05, 7.05, 17.05], 4));
    mesh
}

/// The triangles of a VOXL's mesh chunks (binary STL each).
fn voxl_mesh_triangles(bytes: &[u8]) -> Vec<Triangle> {
    use std::io::Read;
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    for i in 0..u32_at(8) {
        let base = 16 + i * 20;
        if &bytes[base..base + 4] != b"MESH" {
            continue;
        }
        let (offset, size) = (u32_at(base + 8), u32_at(base + 12));
        let mut stl = Vec::new();
        flate2::read::ZlibDecoder::new(&bytes[offset..offset + size]).read_to_end(&mut stl).unwrap();
        let n = u32::from_le_bytes(stl[80..84].try_into().unwrap()) as usize;
        let f = |at: usize| f32::from_le_bytes(stl[at..at + 4].try_into().unwrap());
        out.extend((0..n).map(|t| {
            let b = 84 + t * 50;
            let p = |k: usize| Vec3::new(f(b + k * 12), f(b + k * 12 + 4), f(b + k * 12 + 8));
            Triangle::new(p(1), p(2), p(3), p(0))
        }));
    }
    out
}

/// A VOXL's support entities without their ids and links by id (new on every write).
fn support_entities(bytes: &[u8]) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.retain(|k, _| k != "id" && !k.ends_with("Id") && !k.ends_with("Ids"));
                m.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut supports = voxl_chunk(bytes, b"SUPP");
    strip(&mut supports);
    supports
}

fn watertight_check(triangles: &[Triangle]) -> findamodel::geometry::watertight::mesh::Check {
    use findamodel::geometry::watertight::mesh;
    let points = findamodel::geometry::support_reader::math::points_of(triangles);
    mesh::check(&mesh::Indexed::weld(&points), true)
}

#[tokio::test]
async fn the_remeshed_voxl_holds_the_watertight_model_and_the_same_supports() {
    let app = TestApp::new().await;
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&crossed_knight(), None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.index_and_wait(3).await;
    let (_, result) = app.get("/api/query?limit=10").await;
    let id = result["models"][0]["id"].as_str().unwrap().to_string();

    let normal = download_voxl(&app, &id).await;
    let (status, headers, remeshed) =
        app.send(Request::get(format!("/api/models/{id}/voxl?remeshed=true")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().contains("knight-remeshed.voxl"));

    let before = watertight_check(&voxl_mesh_triangles(&normal));
    assert!(before.self_intersecting_pairs > 0, "the stored model crosses itself: {before:?}");
    let model = voxl_mesh_triangles(&remeshed);
    let after = watertight_check(&model);
    assert!(after.watertight() && after.shells == 1, "{after:?}");
    // The union of the body and the crossing block, and nothing of the supports (below z 10).
    let expected = 10.0 * 10.0 * 5.0 + 5.0 * 5.0 * 4.0 - 2.95 * 2.95 * 1.95;
    assert!((after.volume - expected).abs() < 1e-3, "{} vs {expected}", after.volume);
    // Nothing of the supports: the repaired mesh spans the same box as the stored model.
    let (lo, hi) = bounds(&voxl_mesh_triangles(&normal));
    let (rlo, rhi) = bounds(&model);
    assert!(
        (0..3).all(|k| (lo[k] - rlo[k]).abs() < 1e-3 && (hi[k] - rhi[k]).abs() < 1e-3),
        "{lo:?} {hi:?} vs {rlo:?} {rhi:?}"
    );
    assert_close(&support_entities(&remeshed), &support_entities(&normal), "the same DragonFruit supports");

    // About as compact as the normal VOXL: the repaired mesh stores no per-face normals.
    assert!(remeshed.len() * 4 < normal.len() * 5, "{} bytes against {}", remeshed.len(), normal.len());

    // Cached under its own key, apart from the normal VOXL.
    let entries = std::fs::read_dir(app.state.settings.voxl_cache_path()).unwrap().count();
    assert_eq!(entries, 2);
}

#[tokio::test]
async fn a_remeshed_plate_voxl_repairs_each_model_and_keeps_the_supports() {
    let app = TestApp::new().await;
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&crossed_knight(), None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.index_and_wait(3).await;
    let (_, result) = app.get("/api/query?limit=10").await;
    let id = result["models"][0]["id"].as_str().unwrap().to_string();
    let plate = |format: &str| {
        json!({ "placements": [
            { "modelId": id, "instanceIndex": 0, "xMm": 0, "yMm": 0, "angleRad": 0 }
        ], "format": format, "bedWidthMm": 200, "bedDepthMm": 120 })
    };
    let mut voxls = Vec::new();
    for format in ["voxl", "voxl-remeshed"] {
        let request = Request::post("/api/plate/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(plate(format).to_string()))
            .unwrap();
        let (status, _, voxl) = app.send(request).await;
        assert_eq!(status, StatusCode::OK);
        voxls.push(voxl);
    }
    let after = watertight_check(&voxl_mesh_triangles(&voxls[1]));
    assert!(after.watertight() && after.shells == 1, "{after:?}");
    assert!(!watertight_check(&voxl_mesh_triangles(&voxls[0])).watertight());
    assert_close(&support_entities(&voxls[1]), &support_entities(&voxls[0]), "the same DragonFruit supports");
}

#[tokio::test]
async fn a_download_and_a_plate_asking_for_one_model_at_once_share_its_repair() {
    let app = TestApp::new().await;
    let id = indexed_knight_of_its_own(&app, 1.01).await;
    let plate = remeshed_plate(&id);
    let shared_before = findamodel::geometry::watertight::repairs_shared();
    let download = app.send(Request::get(format!("/api/models/{id}/voxl?remeshed=true")).body(Body::empty()).unwrap());
    let plate = app.send(
        Request::post("/api/plate/generate")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(plate.to_string()))
            .unwrap(),
    );
    let ((s1, _, single), (s2, _, plated)) = tokio::join!(download, plate);
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK));
    // One repair, waited for by the other request: the same model mesh in both.
    assert!(findamodel::geometry::watertight::repairs_shared() > shared_before, "the second request repaired again");
    let (a, b) = (voxl_mesh_triangles(&single), voxl_mesh_triangles(&plated));
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(&b).all(|(x, y)| [x.v0, x.v1, x.v2] == [y.v0, y.v1, y.v2]), "the same repaired faces");
}

/// Indexes a crossed knight of its own (stretched along x by `stretch`: a load centres the model,
/// so only a change of shape keeps other tests from repairing this mesh) as a pre-supported
/// model. Returns its id.
async fn indexed_knight_of_its_own(app: &TestApp, stretch: f32) -> String {
    let knight: Vec<Triangle> = crossed_knight()
        .into_iter()
        .map(|t| {
            let m = |v: Vec3| Vec3::new(v.x * stretch, v.y, v.z);
            Triangle::new(m(t.v0), m(t.v1), m(t.v2), t.normal)
        })
        .collect();
    app.write("Mini/knight.stl", findamodel::geometry::saver::save_stl(&knight, None));
    app.write("Mini/findamodel.yaml", "supported: true\n");
    app.index_and_wait(3).await;
    let (_, result) = app.get("/api/query?limit=10").await;
    result["models"][0]["id"].as_str().unwrap().to_string()
}

fn remeshed_plate(id: &str) -> Value {
    json!({ "placements": [
        { "modelId": id, "instanceIndex": 0, "xMm": 0, "yMm": 0, "angleRad": 0 }
    ], "format": "voxl-remeshed", "bedWidthMm": 200, "bedDepthMm": 120 })
}

fn corner_bits(triangles: &[Triangle]) -> Vec<u32> {
    triangles.iter().flat_map(|t| [t.v0, t.v1, t.v2]).flat_map(|v| [v.x, v.y, v.z]).map(f32::to_bits).collect()
}

#[tokio::test]
async fn two_remeshed_downloads_of_one_model_at_once_build_it_once() {
    let app = TestApp::new().await;
    let id = indexed_knight_of_its_own(&app, 1.02).await;
    let uri = format!("/api/models/{id}/voxl?remeshed=true");
    let ((s1, _, a), (s2, _, b)) = tokio::join!(
        app.send(Request::get(&uri).body(Body::empty()).unwrap()),
        app.send(Request::get(&uri).body(Body::empty()).unwrap())
    );
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK));
    assert_eq!(a, b, "the same bytes");
    // One build, waited for by the other download: one split, support reading, repair and write.
    assert_eq!((app.state.voxl_builds.runs(), app.state.voxl_builds.shared()), (1, 1));
    let repaired = app.state.models.repaired_meshes().unwrap();
    assert_eq!((repaired.writes(), repaired.hits()), (1, 0));

    // A later download (in a flight of its own) is served from the VOXL cache.
    let (status, _, c) = app.send(Request::get(&uri).body(Body::empty()).unwrap()).await;
    assert_eq!((status, c == a), (StatusCode::OK, true));
    assert_eq!((repaired.writes(), repaired.hits()), (1, 0), "not repaired again");
}

#[tokio::test]
async fn a_remeshed_plate_after_a_download_uses_the_kept_repair() {
    let app = TestApp::new().await;
    let id = indexed_knight_of_its_own(&app, 1.03).await;
    let (status, _, single) =
        app.send(Request::get(format!("/api/models/{id}/voxl?remeshed=true")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let repaired = app.state.models.repaired_meshes().unwrap().clone();
    assert_eq!((repaired.writes(), repaired.hits()), (1, 0), "the download repaired and kept it");
    let files: Vec<_> = walk(repaired.dir());
    assert_eq!(files.len(), 1, "{files:?}");

    let request = Request::post("/api/plate/generate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(remeshed_plate(&id).to_string()))
        .unwrap();
    let (status, _, plated) = app.send(request).await;
    assert_eq!(status, StatusCode::OK);
    // Read from the store: no new repair.
    assert_eq!((repaired.writes(), repaired.hits()), (1, 1));
    // Bit for bit the faces the fresh repair wrote.
    let (a, b) = (voxl_mesh_triangles(&single), voxl_mesh_triangles(&plated));
    assert!(!a.is_empty());
    assert_eq!(corner_bits(&a), corner_bits(&b));

    // A download after the VOXL cache is cleared reads it too, and writes the same mesh.
    std::fs::remove_dir_all(app.state.settings.voxl_cache_path()).unwrap();
    let (status, _, again) =
        app.send(Request::get(format!("/api/models/{id}/voxl?remeshed=true")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((repaired.writes(), repaired.hits()), (1, 2));
    assert_eq!(corner_bits(&voxl_mesh_triangles(&again)), corner_bits(&a));
}

/// Every file under `dir`, relative.
fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.path().is_dir() {
            out.extend(walk(&entry.path()).into_iter().map(|f| format!("{}/{f}", entry.file_name().to_string_lossy())));
        } else {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out
}

fn bounds(triangles: &[Triangle]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in triangles.iter().flat_map(|t| [t.v0, t.v1, t.v2]) {
        for (k, x) in [v.x, v.y, v.z].into_iter().enumerate() {
            lo[k] = lo[k].min(x);
            hi[k] = hi[k].max(x);
        }
    }
    (lo, hi)
}

/// Equal JSON, numbers within a micron (single-precision placement of a re-written mesh).
fn assert_close(a: &Value, b: &Value, what: &str) {
    fn diff(a: &Value, b: &Value, path: &str) -> Option<String> {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                ((x.as_f64().unwrap() - y.as_f64().unwrap()).abs() >= 1e-3).then(|| format!("{path}: {x} vs {y}"))
            }
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                x.iter().zip(y).enumerate().find_map(|(i, (p, q))| diff(p, q, &format!("{path}[{i}]")))
            }
            (Value::Object(x), Value::Object(y)) if x.len() == y.len() => x.iter().find_map(|(k, v)| {
                y.get(k).map_or(Some(format!("{path}.{k} missing")), |w| diff(v, w, &format!("{path}.{k}")))
            }),
            _ => (a != b).then(|| format!("{path}: {a} vs {b}")),
        }
    }
    if let Some(d) = diff(a, b, "") {
        panic!("{what}: {d}");
    }
}
