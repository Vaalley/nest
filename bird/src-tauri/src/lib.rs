//! The Bird — Tauri desktop client for the Nest save-backup platform.
//!
//! This crate implements the frontend-facing commands and background services
//! that turn a local gaming device into a Bird. Phase 6 scaffolds the Tauri
//! app, config store, and API client; Phase 7 adds the Foraging Engine that
//! discovers save locations via the Ludusavi manifest; Phases 8 and 9 add the
//! Feather Agent (process monitoring) and the Flight Home sync engine.

pub mod agent;
pub mod api;
pub mod commands;
pub mod config;
pub mod egg;
pub mod error;
pub mod forage;
pub mod process;
pub mod state;
pub mod storage;
pub mod sync;

use tauri::tray::TrayIconBuilder;
use tauri::{async_runtime, Manager, RunEvent, WindowEvent};

use crate::state::AppState;

/// Initialise the Bird application and run the Tauri event loop.
pub fn run() {
    let app_state = async_runtime::block_on(AppState::load());
    let app_state = match app_state {
        Ok(s) => s,
        Err(err) => {
            eprintln!("failed to load Bird state: {err}");
            std::process::exit(1);
        }
    };
    let bg_state = app_state.clone();

    let app = tauri::Builder::default()
        .manage(app_state)
        .setup(|app| {
            setup_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_config,
            commands::set_config,
            commands::register_flock,
            commands::login,
            commands::register_bird,
            commands::logout,
            commands::list_birds,
            commands::list_clutches,
            commands::compare_game,
            commands::resolve_game,
            commands::refresh_manifest,
            commands::discover_games,
            commands::discover_game,
            commands::watch_game,
            commands::unwatch_game,
            commands::watched_games,
            commands::sync_status,
            commands::sync_now,
            commands::resolve_and_sync,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application");

    let app_handle = app.handle().clone();
    async_runtime::spawn(async move {
        if let Err(err) = bg_state.start_background(app_handle).await {
            tracing::error!(%err, "failed to start background sync engine");
        }
    });

    app.run(move |app_handle, event| {
        if let RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } = event
        {
            if label == "main" {
                // Hide to tray instead of quitting.
                api.prevent_close();
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
        }
    });
}

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let open = tauri::menu::MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&open, &quit])?;

    let app_handle = app.handle().clone();
    TrayIconBuilder::new()
        .tooltip("Nest Bird")
        .icon_as_template(false)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(&app_handle)?;

    Ok(())
}
