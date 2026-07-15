//! Integration tests for the Nest server: migrations, /health, and the
//! repository layer against a fresh temporary SQLite database.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
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
        token_secret: "test-secret".to_string(),
        log_level: "off".to_string(),
    };
    (config, dir)
}

async fn test_state() -> (AppState, PathBuf) {
    let (config, dir) = temp_config();
    let state = build_state(config).await.expect("state builds");
    (state, dir)
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

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
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
    assert!(bird.last_seen.is_none());
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
