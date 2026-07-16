const tauri = window.__TAURI__;

const invoke = (() => {
  if (!tauri) return () => Promise.reject(new Error("Tauri API not available"));
  const fn = tauri.core?.invoke || tauri.invoke;
  return fn ? fn.bind(tauri.core || tauri) : () => Promise.reject(new Error("Tauri invoke not available"));
})();

const listen = (() => {
  if (!tauri || !tauri.event) return null;
  return tauri.event.listen.bind(tauri.event);
})();

function toast(message, error = false) {
  const el = document.getElementById("toast");
  el.textContent = message;
  el.classList.toggle("error", error);
  el.classList.remove("hidden");
  setTimeout(() => el.classList.add("hidden"), 4000);
}

async function invokeSafe(name, args = {}) {
  try {
    return { ok: true, data: await invoke(name, args) };
  } catch (err) {
    console.error(err);
    const msg =
      typeof err === "object" && err !== null
        ? err.message || JSON.stringify(err)
        : String(err);
    toast(msg, true);
    return { ok: false, error: err };
  }
}

function escapeHtml(str) {
  if (str == null) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function formatSize(bytes) {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

function statusInfo(state) {
  if (state == null) return { label: "Unknown", class: "status-unknown" };
  switch (String(state).toLowerCase()) {
    case "safe_in_nest":
    case "idle":
      return { label: "Safe in Nest", class: "status-safe" };
    case "flying":
      return { label: "Flying", class: "status-flying" };
    case "chilly_egg":
      return { label: "Chilly Egg", class: "status-chilly" };
    case "error":
      return { label: "Error", class: "status-error" };
    default:
      return { label: state.replace(/_/g, " "), class: "status-unknown" };
  }
}

// ---------------------------------------------------------------------------
// Status & config
// ---------------------------------------------------------------------------

async function loadStatus() {
  const res = await invokeSafe("get_status");
  if (!res.ok) return;
  const s = res.data;
  document.getElementById("status-nest-url").textContent = s.nest_url;
  document.getElementById("status-bird-name").textContent = s.bird_name;
  document.getElementById("status-platform").textContent = s.platform;
  document.getElementById("status-auth").textContent = s.authenticated ? "Yes" : "No";
  document.getElementById("status-flock").textContent = s.flock_username || "—";
  document.getElementById("status-bird-id").textContent = s.bird_id || "—";
  document.getElementById("btn-register-bird").disabled = !s.authenticated;
}

async function loadConfig() {
  const res = await invokeSafe("get_config");
  if (!res.ok) return;
  document.getElementById("config-nest-url").value = res.data.nest_url;
  document.getElementById("config-bird-name").value = res.data.bird_name;
}

async function saveConfig() {
  const config = await (await invokeSafe("get_config")).data;
  config.nest_url = document.getElementById("config-nest-url").value.trim();
  config.bird_name = document.getElementById("config-bird-name").value.trim();
  const res = await invokeSafe("set_config", { config });
  if (res.ok) {
    toast("Configuration saved");
    await loadStatus();
  }
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

async function registerFlock() {
  const username = document.getElementById("auth-username").value.trim();
  const password = document.getElementById("auth-password").value;
  const res = await invokeSafe("register_flock", { username, password });
  if (res.ok) {
    toast("Account created and signed in");
    await loadStatus();
  }
}

async function login() {
  const username = document.getElementById("auth-username").value.trim();
  const password = document.getElementById("auth-password").value;
  const res = await invokeSafe("login", { username, password });
  if (res.ok) {
    toast("Signed in");
    await loadStatus();
  }
}

async function registerBird() {
  const res = await invokeSafe("register_bird", { name: null });
  if (res.ok) {
    toast(`Device registered as ${res.data.bird.name}`);
    await loadStatus();
  }
}

async function logout() {
  const res = await invokeSafe("logout");
  if (res.ok) {
    toast("Signed out");
    await loadStatus();
    renderGames([], {});
  }
}

// ---------------------------------------------------------------------------
// Games
// ---------------------------------------------------------------------------

let discoveredGames = [];
let watchedGameMap = {};

async function loadWatched() {
  const res = await invokeSafe("watched_games");
  if (!res.ok) return {};
  const map = {};
  for (const g of res.data) {
    map[g.game_id] = g;
  }
  watchedGameMap = map;
  return map;
}

async function discoverGames() {
  const loading = document.getElementById("games-loading");
  const empty = document.getElementById("games-empty");
  const list = document.getElementById("games-list");

  loading.classList.remove("hidden");
  empty.classList.add("hidden");
  list.innerHTML = "";

  const [discoverRes, watchedMap] = await Promise.all([
    invokeSafe("discover_games"),
    loadWatched(),
  ]);

  loading.classList.add("hidden");

  if (!discoverRes.ok) {
    renderGames([], watchedMap);
    return;
  }

  discoveredGames = discoverRes.data.sort((a, b) => a.title.localeCompare(b.title));
  renderGames(discoveredGames, watchedMap);
}

function renderGames(games, watchedMap) {
  const list = document.getElementById("games-list");
  const empty = document.getElementById("games-empty");

  list.innerHTML = "";
  if (games.length === 0) {
    empty.classList.remove("hidden");
    return;
  }
  empty.classList.add("hidden");

  for (const g of games) {
    const watched = watchedMap[g.game_id];
    const isProtected = !!watched;
    const state = watched ? watched.state : g.status;
    const message = watched ? watched.message : "";
    const info = statusInfo(state);

    const card = document.createElement("div");
    card.className = "game-card";
    card.dataset.gameId = g.game_id;

    const statusHtml = `<span class="status-badge ${info.class}">${escapeHtml(
      info.label
    )}</span>`;

    card.innerHTML = `
      <div class="game-header">
        <div>
          <div class="game-title">${escapeHtml(g.title)}</div>
          <code class="game-id">${escapeHtml(g.game_id)}</code>
        </div>
        ${statusHtml}
      </div>
      <div class="game-meta">
        <span>Save:</span>
        <code>${escapeHtml(g.save_path || "—")}</code>
        <span class="exists ${g.exists ? "yes" : "no"}">${g.exists ? "Found" : "Missing"}</span>
      </div>
      <div class="game-message ${message ? "" : "hidden"}">${escapeHtml(message)}</div>
      <div class="game-actions">
        <label class="toggle">
          <input type="checkbox" class="watch-toggle" data-game-id="${escapeHtml(
            g.game_id
          )}" ${isProtected ? "checked" : ""} />
          Keep safe in the Nest
        </label>
        <div class="game-buttons">
          <button class="btn-sync-now" data-game-id="${escapeHtml(
            g.game_id
          )}">Sync now</button>
          <button class="btn-history secondary" data-game-id="${escapeHtml(
            g.game_id
          )}" data-title="${escapeHtml(g.title)}">History</button>
        </div>
      </div>
    `;

    list.appendChild(card);
  }

  for (const toggle of list.querySelectorAll(".watch-toggle")) {
    toggle.addEventListener("change", (e) => toggleWatch(e.target.dataset.gameId, e.target.checked));
  }
  for (const btn of list.querySelectorAll(".btn-sync-now")) {
    btn.addEventListener("click", () => syncNow(btn.dataset.gameId));
  }
  for (const btn of list.querySelectorAll(".btn-history")) {
    btn.addEventListener("click", () => showHistory(btn.dataset.gameId, btn.dataset.title));
  }
}

function updateGameCard(gameId, status, message) {
  const card = document.querySelector(`.game-card[data-game-id="${CSS.escape(gameId)}"]`);
  if (!card) return;
  const info = statusInfo(status);
  const badge = card.querySelector(".status-badge");
  badge.className = `status-badge ${info.class}`;
  badge.textContent = info.label;
  const msgEl = card.querySelector(".game-message");
  msgEl.textContent = message || "";
  msgEl.classList.toggle("hidden", !message);
}

async function toggleWatch(gameId, protect) {
  if (protect) {
    const res = await invokeSafe("watch_game", { game_id: gameId, process_names: [] });
    if (res.ok) toast(`Now keeping ${gameId} safe`);
  } else {
    const res = await invokeSafe("unwatch_game", { game_id: gameId });
    if (res.ok) toast(`Stopped watching ${gameId}`);
  }
  await loadWatched();
  renderGames(discoveredGames, watchedGameMap);
}

async function syncNow(gameId) {
  const res = await invokeSafe("sync_now", { game_id: gameId });
  if (res.ok) {
    toast(`${gameId}: ${res.data.message}`);
    updateGameCard(gameId, res.data.state, res.data.message);
  }
  await loadWatched();
  renderGames(discoveredGames, watchedGameMap);
}

async function refreshManifest() {
  const res = await invokeSafe("refresh_manifest");
  if (res.ok) toast("Ludusavi manifest refreshed");
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

const historyDialog = document.getElementById("history-dialog");
let currentHistoryGameId = null;

async function showHistory(gameId, title) {
  currentHistoryGameId = gameId;
  document.getElementById("history-title").textContent = escapeHtml(title);
  document.getElementById("history-subtitle").textContent = escapeHtml(gameId);
  document.getElementById("history-loading").classList.remove("hidden");
  document.getElementById("history-empty").classList.add("hidden");
  document.getElementById("history-list").innerHTML = "";
  historyDialog.showModal();

  const res = await invokeSafe("list_eggs", { game_id: gameId });
  document.getElementById("history-loading").classList.add("hidden");

  if (!res.ok) return;

  const eggs = res.data;
  const list = document.getElementById("history-list");

  if (eggs.length === 0) {
    document.getElementById("history-empty").classList.remove("hidden");
    return;
  }

  for (const egg of eggs) {
    const li = document.createElement("li");
    li.className = "history-item";
    li.innerHTML = `
      <div class="history-info">
        <span>${new Date(egg.created_at).toLocaleString()}</span>
        <span class="muted">${formatSize(egg.size_bytes)}</span>
        <code>${escapeHtml(egg.file_hash.slice(0, 16))}…</code>
      </div>
      <div class="history-actions">
        <button class="btn-restore" data-egg-id="${escapeHtml(
          egg.id
        )}">Restore</button>
        <button class="btn-delete-egg danger" data-egg-id="${escapeHtml(
          egg.id
        )}">Delete</button>
      </div>
    `;
    list.appendChild(li);
  }

  for (const btn of list.querySelectorAll(".btn-restore")) {
    btn.addEventListener("click", () => restoreEgg(currentHistoryGameId, btn.dataset.eggId));
  }
  for (const btn of list.querySelectorAll(".btn-delete-egg")) {
    btn.addEventListener("click", () => deleteEgg(currentHistoryGameId, btn.dataset.eggId));
  }
}

async function restoreEgg(gameId, eggId) {
  const res = await invokeSafe("restore_egg", { game_id: gameId, egg_id: eggId });
  if (res.ok) {
    toast(res.data.message);
    historyDialog.close();
    await loadWatched();
    renderGames(discoveredGames, watchedGameMap);
  }
}

async function deleteEgg(gameId, eggId) {
  if (!confirm("Delete this Egg? It cannot be undone.")) return;
  const res = await invokeSafe("delete_egg", { game_id: gameId, egg_id: eggId });
  if (res.ok) {
    toast("Egg deleted");
    await showHistory(gameId, document.getElementById("history-title").textContent);
    await loadWatched();
    renderGames(discoveredGames, watchedGameMap);
  }
}

// ---------------------------------------------------------------------------
// Conflict dialog
// ---------------------------------------------------------------------------

const conflictDialog = document.getElementById("conflict-dialog");
let currentConflictGameId = null;

function showConflict(gameId) {
  currentConflictGameId = gameId;
  conflictDialog.showModal();
}

async function resolveConflict(resolution) {
  if (!currentConflictGameId) return;
  conflictDialog.close();
  const res = await invokeSafe("resolve_and_sync", {
    game_id: currentConflictGameId,
    resolution,
  });
  if (res.ok) {
    toast(res.data.message);
    await loadWatched();
    renderGames(discoveredGames, watchedGameMap);
  }
  currentConflictGameId = null;
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

function setupEventListeners() {
  if (!listen) return;

  listen("sync-status", (event) => {
    const p = event.payload;
    updateGameCard(p.game_id, p.state, p.message);
  });

  listen("sync-conflict", (event) => {
    showConflict(event.payload.game_id);
  });

  listen("game-launched", (event) => {
    toast(`${event.payload.game_id} launched — syncing`);
  });

  listen("game-exited", (event) => {
    toast(`${event.payload.game_id} exited — laying a fresh Egg`);
  });
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

window.addEventListener("DOMContentLoaded", async () => {
  setupEventListeners();
  await loadConfig();
  await loadStatus();
  await discoverGames();

  document.getElementById("btn-save-config").addEventListener("click", saveConfig);
  document.getElementById("btn-register-flock").addEventListener("click", registerFlock);
  document.getElementById("btn-login").addEventListener("click", login);
  document.getElementById("btn-register-bird").addEventListener("click", registerBird);
  document.getElementById("btn-logout").addEventListener("click", logout);
  document.getElementById("btn-discover").addEventListener("click", discoverGames);
  document.getElementById("btn-refresh-manifest").addEventListener("click", refreshManifest);
  document.getElementById("history-close").addEventListener("click", () => historyDialog.close());
  document.getElementById("btn-resolve-nest").addEventListener("click", () => resolveConflict("nest"));
  document.getElementById("btn-resolve-local").addEventListener("click", () => resolveConflict("local"));
});
