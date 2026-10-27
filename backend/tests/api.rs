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
        "minimumPreviewGenerationVersion": 1
    });
    let (status, updated) = app.json("PUT", "/api/settings/config", update.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(updated.get("theme").is_none());
    assert_eq!(updated["generatePreviewsEnabled"], false);

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
