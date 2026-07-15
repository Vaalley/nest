# Nest: Product Specification

## 1. System Architecture

Nest uses a decoupled client-server model. **The Nest** (server) remains incredibly lightweight and passive, while each **Bird** (client device, like a Steam Deck or desktop PC) handles local heavy lifting (game scanning, process watching, and egg packaging).

```
┌─────────────────────────────────┐                 ┌─────────────────────────────────┐
│          The Bird (Client)      │     HTTPS       │         The Nest (Server)       │
│        (Steam Deck / PC)        │◄───────────────►│  (Pikapods / Docker Container)  │
└────────────────┬────────────────┘     REST        └────────────────┬────────────────┘
                 │                                                   │
   Scans local games via Ludusavi                      Manages user accounts (Flocks)
   Monitors active play sessions                       Stores SQLite database
   Packages saves into "Eggs"                          Holds history of "Clutches" (.zips)

```

---

## 2. Server Specifications (The Nest Backend)

The server acts as the central sanctuary, organizing user accounts, device registrations, and save archives.

* **Technology Stack:** Rust (to ensure a minuscule RAM footprint of under 30MB on Pikapods) with SQLite (default embedded database).
* **Domain Models (DDD):**
* **Bird:** A registered client device (e.g., "Valentin's Steam Deck", "Main Desktop").
* **Egg:** A single zipped save file snapshot.
* **Clutch:** The collection of Eggs (rolling save history) for a specific game.


* **Storage & Lifecycle:**
* Saves are compressed as `.zip` files (Eggs) and stored in a directory structure: `/data/flocks/{user_id}/{game_id}/egg_[timestamp].zip`.
* **The Brood Limit:** Automatically prunes the oldest Eggs when a Clutch exceeds the user-defined version history limit (default: 10 Eggs).



---

## 3. Client Specifications (The Bird Desktop/Handheld App)

The client runs locally on the user's gaming hardware (Windows, Linux/SteamOS).

* **Technology Stack:** Tauri (Rust backend with a lightweight web frontend). This ensures the background agent uses minimal system resources and battery on handhelds.
* **Core Systems:**
* **Foraging Engine:** Regularly pulls the open-source **Ludusavi** manifest to automatically identify where games write their local save files.
* **Feather Agent:** A system-tray background worker that monitors when games launch and exit.


* **Cozy UI ("The Branch"):**
* An interface showing your installed games with simple toggles to "Keep Safe in the Nest."
* Status indicators: **Safe in Nest** (Synced), **Flying** (Syncing), or **Chilly Egg** (Save Conflict).



---

## 4. The Sync Lifecycle (The Flight Home)

Every time a user plays a game, the Bird and the Nest coordinate to protect their progress.

1. **Leaving the Branch:** Pre-Launch Check.
The Bird detects a game starting. It sends a quick request to the Nest to compare the local save's hash and timestamp against the latest Egg in the Clutch.


2. **Hatching the Egg:** Resolution or Pull.
* **Nest has a newer Egg:** The Bird downloads the Egg, unpacks the save locally, and launches the game.
* **Chilly Egg Conflict:** If both local and Nest saves were modified offline, Nest pauses and asks: *"This egg got cold while you were away. Which one do you want to keep warm?"*


3. **In Flight:** Active Monitoring.
While the game is active, the Bird background process sleeps, monitoring the game's process PID with negligible resource usage.


4. **Laying a New Egg:** Post-Exit Return.
The Bird detects the game has closed. It waits 5 seconds for final disk writes, compresses the updated save folder into a new Egg, and flies it back home to the Nest via a secure upload.


---

## 5. Cozy REST API Outline

### Flock Management (Authentication)

* `POST /api/flock/register` - Register a new user account.
* `POST /api/flock/login` - Authenticate and retrieve a secure token.

### Bird Management (Devices)

* `GET /api/birds` - List all active devices connected to the Nest.
* `POST /api/birds/register` - Link a new hardware node (e.g., your AYN Odin or Steam Deck).

### Clutch & Egg Management (Saves)

* `GET /api/clutches` - Retrieve a user's tracked games and their current status.
* `GET /api/clutches/{game_id}/eggs` - Retrieve metadata for all Eggs in a game's Clutch (version history).
* `POST /api/clutches/{game_id}/lay` - Upload a new Egg (multipart payload with `.zip` file, file hash, and source Bird ID).
* `GET /api/clutches/{game_id}/hatch/{egg_id}` - Download a specific Egg to restore a save state.
* `DELETE /api/clutches/{game_id}/eggs/{egg_id}` - Manually discard an Egg from the Clutch.

---

## 6. MVP Goals (Minimum Viable Product)

1. **Single-User Nest Server:** A containerized Rust application with SQLite.
2. **Basic Bird Client:** A lightweight Tauri client that reads the local Ludusavi database, displaying a clean "Scan & Tick" UI for a subset of manually verified test games.
3. **Basic Flight Cycle:** Successful process-monitoring on game exit, automated Egg compression, and upload to the Nest.
4. **Windows Support:** The Bird client should support Windows.

## 7. Long-term Goals

1. **Fully cross-platform support:** The Bird client should support Windows, Linux/SteamOS, MacOS, and potentially mobile platforms (iOS/Android).
2. **Deploy seamlessly to Pikapods:** The Nest server should be easily deployable to Pikapods (and other container platforms if demand increases).
