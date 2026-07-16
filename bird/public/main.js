const invoke = (() => {
  const t = window.__TAURI__;
  if (!t) return () => Promise.reject(new Error("Tauri API not available"));
  return t.core?.invoke || t.invoke;
})();

function toast(message, error = false) {
  const el = document.getElementById("toast");
  el.textContent = message;
  el.classList.toggle("error", error);
  el.classList.remove("hidden");
  setTimeout(() => el.classList.add("hidden"), 3500);
}

async function invokeSafe(name, args = {}) {
  try {
    return { ok: true, data: await invoke(name, args) };
  } catch (err) {
    console.error(err);
    const msg = typeof err === "object" && err !== null ? err.message || JSON.stringify(err) : String(err);
    toast(msg, true);
    return { ok: false, error: err };
  }
}

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
  if (res.ok) toast("Configuration saved");
  await loadStatus();
}

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
  }
}

async function discoverGames() {
  const res = await invokeSafe("discover_games");
  if (!res.ok) return;
  const tbody = document.querySelector("#games-table tbody");
  tbody.innerHTML = "";
  for (const g of res.data) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td><strong>${escapeHtml(g.title)}</strong><br><code>${escapeHtml(g.game_id)}</code></td>
      <td><code>${escapeHtml(g.save_path || "—")}</code></td>
      <td>${g.exists ? "Yes" : "No"}</td>
      <td><code>${escapeHtml(g.local_hash ? g.local_hash.slice(0, 16) + "…" : "—")}</code></td>
      <td>${g.local_modified_at ? new Date(g.local_modified_at * 1000).toLocaleString() : "—"}</td>
    `;
    tbody.appendChild(tr);
  }
}

async function refreshManifest() {
  const res = await invokeSafe("refresh_manifest");
  if (res.ok) toast("Ludusavi manifest refreshed");
}

async function listClutches() {
  const res = await invokeSafe("list_clutches");
  if (!res.ok) return;
  const ul = document.getElementById("clutches-list");
  ul.innerHTML = "";
  for (const c of res.data) {
    const li = document.createElement("li");
    li.textContent = `${c.clutch.game_id} — ${c.egg_count} egg(s), status: ${c.status}`;
    ul.appendChild(li);
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

window.addEventListener("DOMContentLoaded", async () => {
  await loadConfig();
  await loadStatus();

  document.getElementById("btn-save-config").addEventListener("click", saveConfig);
  document.getElementById("btn-register-flock").addEventListener("click", registerFlock);
  document.getElementById("btn-login").addEventListener("click", login);
  document.getElementById("btn-register-bird").addEventListener("click", registerBird);
  document.getElementById("btn-logout").addEventListener("click", logout);
  document.getElementById("btn-discover").addEventListener("click", discoverGames);
  document.getElementById("btn-refresh-manifest").addEventListener("click", refreshManifest);
  document.getElementById("btn-list-clutches").addEventListener("click", listClutches);
});
