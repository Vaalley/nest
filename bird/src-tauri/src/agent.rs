//! Feather Agent — background process watcher for tracked games.
//!
//! Phase 8 launches a low-CPU worker that detects when a watched game starts,
//! monitors its PID while it runs, and emits an `Exited` event a few seconds
//! after the process disappears.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, Mutex, RwLock};

use crate::error::BirdResult;
use crate::process::{ProcessBackend, SysinfoBackend};

const POST_EXIT_DELAY: Duration = Duration::from_secs(5);
const SCAN_INTERVAL: Duration = Duration::from_secs(2);
const MONITOR_INTERVAL: Duration = Duration::from_millis(500);

/// Lifecycle events emitted by the Feather Agent.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A tracked game process has been detected.
    Launched { game_id: String, pid: u32 },
    /// A tracked game process is still running.
    Running { game_id: String, pid: u32 },
    /// A tracked game process has exited and the post-exit delay elapsed.
    Exited { game_id: String, pid: u32 },
}

#[derive(Debug, Clone)]
struct ActiveSession {
    pid: u32,
}

#[derive(Debug, Clone)]
struct ExitingSession {
    game_id: String,
    pid: u32,
    deadline: Instant,
}

/// Background process monitor.
#[derive(Clone)]
pub struct FeatherAgent {
    watched: Arc<RwLock<HashMap<String, Vec<String>>>>,
    active: Arc<Mutex<HashMap<String, ActiveSession>>>,
    events: broadcast::Sender<AgentEvent>,
    shutdown: Arc<AtomicBool>,
    handle: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
}

impl FeatherAgent {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            watched: Arc::new(RwLock::new(HashMap::new())),
            active: Arc::new(Mutex::new(HashMap::new())),
            events,
            shutdown: Arc::new(AtomicBool::new(false)),
            handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Subscribe to lifecycle events.
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.events.subscribe()
    }

    /// Start watching a game and map it to one or more process names.
    pub async fn track(&self, game_id: String, process_names: Vec<String>) {
        self.watched.write().await.insert(game_id, process_names);
    }

    /// Stop watching a game.
    pub async fn untrack(&self, game_id: &str) {
        let mut active = self.active.lock().await;
        active.remove(game_id);
        drop(active);
        self.watched.write().await.remove(game_id);
    }

    /// Return the list of watched games and their process names.
    pub async fn watched(&self) -> Vec<(String, Vec<String>)> {
        self.watched
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Start scanning with the default Windows `sysinfo` backend.
    pub async fn start(&self) -> BirdResult<()> {
        let backend = SysinfoBackend::new()?;
        self.start_with(Box::new(backend)).await
    }

    /// Start scanning with a custom backend (useful for tests or future platforms).
    pub async fn start_with(
        &self,
        mut backend: Box<dyn ProcessBackend + Send + Sync>,
    ) -> BirdResult<()> {
        let watched = self.watched.clone();
        let active = self.active.clone();
        let events = self.events.clone();
        let shutdown = self.shutdown.clone();

        let handle = tauri::async_runtime::spawn(async move {
            let mut scan_interval = tokio::time::interval(SCAN_INTERVAL);
            let mut monitor_interval = tokio::time::interval(MONITOR_INTERVAL);
            let mut exiting: Vec<ExitingSession> = Vec::new();

            loop {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                tokio::select! {
                    _ = scan_interval.tick() => {
                        backend.refresh();

                        let mut active_guard = active.lock().await;

                        // Promote finished exits after the post-exit delay.
                        let now = Instant::now();
                        let (ready, pending): (Vec<_>, Vec<_>) =
                            exiting.into_iter().partition(|s| s.deadline <= now);
                        exiting = pending;
                        for s in ready {
                            let _ = events.send(AgentEvent::Exited {
                                game_id: s.game_id,
                                pid: s.pid,
                            });
                        }

                        // Check active sessions.
                        let mut still_active: HashMap<String, ActiveSession> = HashMap::new();
                        for (game_id, session) in active_guard.drain() {
                            if backend.is_running(session.pid) {
                                let _ = events.send(AgentEvent::Running {
                                    game_id: game_id.clone(),
                                    pid: session.pid,
                                });
                                still_active.insert(game_id, session);
                            } else {
                                exiting.push(ExitingSession {
                                    game_id,
                                    pid: session.pid,
                                    deadline: Instant::now() + POST_EXIT_DELAY,
                                });
                            }
                        }

                        // Detect newly launched tracked games.
                        let watched_guard = watched.read().await;
                        for (game_id, names) in watched_guard.iter() {
                            if still_active.contains_key(game_id)
                                || exiting.iter().any(|e| e.game_id == *game_id)
                            {
                                continue;
                            }
                            if let Some(info) = backend.find_by_name(names) {
                                let _ = events.send(AgentEvent::Launched {
                                    game_id: game_id.clone(),
                                    pid: info.pid,
                                });
                                still_active.insert(
                                    game_id.clone(),
                                    ActiveSession { pid: info.pid },
                                );
                            }
                        }
                        *active_guard = still_active;
                    }
                    _ = monitor_interval.tick() => {
                        // The scan loop already refreshes the process list;
                        // the monitor tick keeps the async runtime responsive.
                    }
                }
            }
        });

        *self.handle.lock().await = Some(handle);
        Ok(())
    }
}

impl Default for FeatherAgent {
    fn default() -> Self {
        Self::new()
    }
}
