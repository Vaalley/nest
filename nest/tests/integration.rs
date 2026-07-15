//! Integration tests for the Nest server: migrations, /health, auth, and
//! the repository layer against a fresh temporary SQLite database.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use nest_server::{build_state, routes, AppState, Config};
use nest_shared::domain::{Platform, DEFAULT_BROOD_LIMIT};

/// Build a Config pointing at a unique temp SQLite file.
fn temp_config() -> (Config, PathBuf) {
    let mut dir = std::env::temp_dir();
    let unique = format!("nest-test-{}-{}", std::process::id(), uuid::Uuid::new_v4());
    dir.push(unique);
    let db_path = dir.join("nest.sqlite");

    let config = Config {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        data_dir: dir.clone(),
        db_path,
        default_brood_limit: DEFAULT_BROOD_LIMIT,
        token_secret: "test-secret-which-is-long-enough-for-hmac".to_string(),
        token_expiry_seconds: 365 * 24 * 60 * 60,
        log_level: "off".to_string(),
    };
    (config, dir)
}

async fn test_state() -> (AppState, PathBuf) {
    let (config, dir) = temp_config();
    let state = build_state(config).await.expect("state builds");
    (state, dir)
}

fn json_request(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let body = match body {
        Some(value) => Body::from(serde_json::to_vec(&value).unwrap()),
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_returns_ok_after_migrations() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let json = response_json(response).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["database"], "ok");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn repositories_persist_full_aggregate_graph() {
    let (state, dir) = test_state().await;

    // Flock
    let flock = state
        .flocks()
        .create("valentin", "hash")
        .await
        .expect("create flock");
    assert!(state.flocks().username_exists("valentin").await.unwrap());
    assert!(!state.flocks().username_exists("nobody").await.unwrap());

    let creds = state
        .flocks()
        .find_credentials_by_username("valentin")
        .await
        .unwrap()
        .expect("credentials");
    assert_eq!(creds.password_hash, "hash");
    assert_eq!(creds.flock.id, flock.id);

    // Bird
    let bird = state
        .birds()
        .create(flock.id, "Steam Deck", Platform::Linux)
        .await
        .expect("create bird");
    assert!(bird.last_seen.is_some());
    state.birds().touch_last_seen(bird.id).await.unwrap();
    let birds = state.birds().list_by_flock(flock.id).await.unwrap();
    assert_eq!(birds.len(), 1);
    assert!(birds[0].last_seen.is_some());

    // Clutch
    let clutch = state
        .clutches()
        .create(flock.id, "stardew-valley", DEFAULT_BROOD_LIMIT)
        .await
        .expect("create clutch");
    let found = state
        .clutches()
        .find_by_game(flock.id, "stardew-valley")
        .await
        .unwrap()
        .expect("clutch found");
    assert_eq!(found.id, clutch.id);

    // Eggs
    let egg = state
        .eggs()
        .create(nest_server::repository::egg::NewEgg {
            clutch_id: clutch.id,
            source_bird_id: Some(bird.id),
            file_hash: "deadbeef",
            size_bytes: 1234,
            file_path: "data/flocks/x/stardew-valley/egg_1.zip",
        })
        .await
        .expect("create egg");
    assert_eq!(state.eggs().count_in_clutch(clutch.id).await.unwrap(), 1);

    let fetched = state
        .eggs()
        .find_in_clutch(clutch.id, egg.id)
        .await
        .unwrap()
        .expect("egg found");
    assert_eq!(fetched.file_hash, "deadbeef");
    assert_eq!(fetched.source_bird_id, Some(bird.id));

    let deleted = state
        .eggs()
        .delete_in_clutch(clutch.id, egg.id)
        .await
        .unwrap()
        .expect("egg deleted");
    assert_eq!(deleted.id, egg.id);
    assert_eq!(state.eggs().count_in_clutch(clutch.id).await.unwrap(), 0);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn flock_register_creates_account_and_rejects_duplicates() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    let body = serde_json::json!({
        "username": "valley",
        "password": "correct-horse-battery-staple",
    });
    let response = app
        .clone()
        .oneshot(json_request("POST", "/api/flock/register", Some(body)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert!(!json["token"].as_str().unwrap().is_empty());
    assert_eq!(json["flock"]["username"], "valley");

    // Duplicate username is rejected.
    let body = serde_json::json!({
        "username": "valley",
        "password": "different-password-1234",
    });
    let response = app
        .oneshot(json_request("POST", "/api/flock/register", Some(body)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn flock_login_issues_token_and_rejects_invalid_credentials() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    let register_body = serde_json::json!({
        "username": "valley",
        "password": "correct-horse-battery-staple",
    });
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/flock/register",
            Some(register_body),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Valid login.
    let login_body = serde_json::json!({
        "username": "valley",
        "password": "correct-horse-battery-staple",
    });
    let response = app
        .clone()
        .oneshot(json_request("POST", "/api/flock/login", Some(login_body)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    let token = json["token"].as_str().unwrap().to_string();

    // The token can access a protected route.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/flock/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["username"], "valley");

    // Invalid password returns 401 with no user-enumeration leak.
    let login_body = serde_json::json!({
        "username": "valley",
        "password": "wrong-password",
    });
    let response = app
        .clone()
        .oneshot(json_request("POST", "/api/flock/login", Some(login_body)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Non-existent user returns the same 401.
    let login_body = serde_json::json!({
        "username": "nobody",
        "password": "any-password",
    });
    let response = app
        .oneshot(json_request("POST", "/api/flock/login", Some(login_body)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn protected_routes_reject_missing_or_invalid_tokens() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/flock/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/birds")
                .header("authorization", "Bearer invalid-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn birds_register_and_list() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    // Register and log in as a Flock.
    let register_body = serde_json::json!({
        "username": "valley",
        "password": "correct-horse-battery-staple",
    });
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/flock/register",
            Some(register_body),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    let flock_token = json["token"].as_str().unwrap().to_string();

    // Register a Bird.
    let bird_body = serde_json::json!({
        "name": "Main Desktop",
        "platform": "windows",
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/birds")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {flock_token}"))
                .body(Body::from(serde_json::to_vec(&bird_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    let bird_token = json["token"].as_str().unwrap().to_string();
    assert_eq!(json["bird"]["name"], "Main Desktop");
    assert_eq!(json["bird"]["platform"], "windows");
    assert!(json["bird"]["last_seen"].is_string());

    // List birds using the Bird token and verify last_seen activity is recorded.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/birds")
                .header("authorization", format!("Bearer {bird_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    let birds = json.as_array().unwrap();
    assert_eq!(birds.len(), 1);
    assert_eq!(birds[0]["name"], "Main Desktop");
    assert_eq!(birds[0]["platform"], "windows");
    assert!(birds[0]["last_seen"].is_string());

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn birds_are_isolated_between_flocks() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    async fn flock_token(app: &mut axum::Router, username: &str) -> String {
        let body = serde_json::json!({
            "username": username,
            "password": "correct-horse-battery-staple",
        });
        let response = app
            .oneshot(json_request("POST", "/api/flock/register", Some(body)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        json["token"].as_str().unwrap().to_string()
    }

    let mut app_a = app.clone();
    let token_a = flock_token(&mut app_a, "flock-a").await;

    let mut app_b = app.clone();
    let token_b = flock_token(&mut app_b, "flock-b").await;

    // Register a bird for each Flock.
    async fn register_bird(app: &mut axum::Router, token: &str, name: &str) -> serde_json::Value {
        let body = serde_json::json!({
            "name": name,
            "platform": "linux",
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/birds")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response_json(response).await
    }

    register_bird(&mut app_a.clone(), &token_a, "Deck A").await;
    register_bird(&mut app_b.clone(), &token_b, "Deck B").await;

    // Flock A should only see Deck A.
    let response = app_a
        .oneshot(
            Request::builder()
                .uri("/api/birds")
                .header("authorization", format!("Bearer {token_a}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = response_json(response).await;
    let birds = json.as_array().unwrap();
    assert_eq!(birds.len(), 1);
    assert_eq!(birds[0]["name"], "Deck A");

    // Flock B should only see Deck B.
    let response = app_b
        .oneshot(
            Request::builder()
                .uri("/api/birds")
                .header("authorization", format!("Bearer {token_b}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = response_json(response).await;
    let birds = json.as_array().unwrap();
    assert_eq!(birds.len(), 1);
    assert_eq!(birds[0]["name"], "Deck B");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn rate_limit_blocks_repeated_auth_requests_from_same_ip() {
    let (state, dir) = test_state().await;
    let app = routes::router(state);

    let ip: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    // Exhaust the per-IP budget (5 requests in 60s) for the register endpoint.
    for i in 0..5 {
        let body = serde_json::json!({
            "username": format!("rluser{i}"),
            "password": "correct-horse-battery-staple",
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/flock/register")
                    .header("content-type", "application/json")
                    .extension(ConnectInfo(ip))
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status() == StatusCode::OK || response.status() == StatusCode::CONFLICT,
            "unique usernames should register, collisions should conflict, not rate-limit"
        );
    }

    // The 6th request from the same IP should be rate-limited.
    let body = serde_json::json!({
        "username": "rluser-blocked",
        "password": "correct-horse-battery-staple",
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/flock/register")
                .header("content-type", "application/json")
                .extension(ConnectInfo(ip))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    // A different IP is not blocked.
    let other_ip: SocketAddr = "127.0.0.2:12345".parse().unwrap();
    let body = serde_json::json!({
        "username": "rluser-other",
        "password": "correct-horse-battery-staple",
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/flock/register")
                .header("content-type", "application/json")
                .extension(ConnectInfo(other_ip))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let _ = std::fs::remove_dir_all(dir);
}
