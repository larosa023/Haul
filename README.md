<p align="center">
  <img src="app-icon.svg" width="96" alt="Haul logo" />
</p>

<h1 align="center">Haul</h1>

<p align="center">A clean download manager and torrent client for Windows, in one light app.</p>

Haul does what IDM and qBittorrent do, in a single small app built with [Tauri](https://tauri.app) and [Svelte](https://svelte.dev): fast multi-connection direct downloads, and a full torrent client powered by [librqbit](https://github.com/ikatson/rqbit). It lives in the tray, has no accounts, ads or telemetry, needs no admin rights, and runs no background services.

## Features

- **Fast direct downloads.** Paste a link (or copy one: Haul notices it and offers to grab it). Big files are split into parallel connections, and when a connection finishes its part it takes over half of the biggest part left, so every connection stays busy to the end.
  - **Pause and resume**, even after closing Haul or restarting the PC. Progress is saved per part.
  - **Retries** with a growing pause between tries, and a different error for what won't fix itself (a 404, a full disk).
  - **SHA-256 check**: paste the expected hash and Haul verifies the finished file.
- **Torrents.** Magnet links and .torrent files (Haul registers as the handler for both, per user).
  - **Pick files** before starting and any time after, with **Skip / Low / Normal / High** priority per file.
  - **Download in order** (sequential), to preview videos while they download.
  - **Seeding limits** by ratio and time, globally or per torrent, and a limit on how many torrents seed at once.
  - **DHT, PEX, encryption** (Prefer / Require / Off, compatible with qBittorrent, Transmission and uTorrent), **UPnP and NAT-PMP** port mapping, and **IP blocklists** (eMule .dat, PeerGuardian .p2p, plain or .gz, from a link or a file).
  - **Bind torrents to a network adapter** (your VPN). If that adapter goes down, torrents stop until it's back.
- **Real-Debrid.** Connect your account in **Settings → Debrid** with your [API key](https://real-debrid.com/apitoken) (stored encrypted for your Windows account).
  - **File hoster links** (1fichier, Rapidgator, Mega and the other hosters Real-Debrid supports) download at premium speed with Haul's multiple connections. Expired links are renewed automatically.
  - **Torrents through Real-Debrid**: Real-Debrid downloads the torrent on its servers, then Haul fetches the files over HTTPS (no seeding, no torrent traffic from your PC). Pick the files, follow Real-Debrid's progress in the list, and the files land in the torrent's folders. Off, Ask each time, or Always.
- **One list for everything**: name, size, progress, speed, time left and status, with **Downloading, Seeding, Completed, Paused and Errors** filters and search. The list only draws what's on screen, so hundreds of torrents stay smooth.
- **Sorting.** Downloads go into folders you choose (Video, Music, Games, Software, Archives, Documents, Images, Other) by rules you can edit: file type, size and site.
- **Auto-extract** finished zip, 7z and rar archives (multi-part too), and optionally delete the archive after.
- **Speed limits**: global and per download, a **limited speed** you can switch on with the turtle button or by **schedule** (e.g. limited during the day, full speed at night), and a **queue** with a maximum number of downloads at once and High / Normal / Low priority.
- **Browser extension** for Chromium browsers (Chrome, Edge, Drift…) that sends downloads to Haul, with **Download with Haul** in the right-click menu. Alt+click lets the browser keep one.
- **Notifications** when a download finishes, with **Open** and **Show in folder**.
- **Built-in updates.** Haul checks GitHub Releases on launch and every few hours (**Stable** or **Beta** channel), downloads in the background, verifies the SHA-256, and restarts into the new version when nothing is downloading (or when you say so). Downloads pick up where they left off, and if the new version doesn't start properly Haul rolls back by itself.
- **The suite's 14 themes** (Dark with Mica by default, Light, Windows, Midnight, Glass, Frost, Twilight, Forest, Sunset, Ocean, Rose, Graphite, Paper, Mist), the suite's accent colors, and **custom theme files** in the Hangar format, all switchable live.
- **Keyboard first.**

  | Keys | Does |
  |---|---|
  | `Ctrl+N` | New download |
  | `Ctrl+O` | Open a .torrent file |
  | `Ctrl+V` | Paste a link or magnet to download it |
  | `Space` | Pause / resume |
  | `Del` | Remove (with an option to delete the files) |
  | `Enter` / `Shift+Enter` | Open / show in folder |
  | `↑ ↓` (`Shift` to extend), `Ctrl+A` | Select |
  | `Alt+↑ / Alt+↓` (`Ctrl` for top / bottom) | Move in the queue |
  | `Ctrl+F` | Search |
  | `F2` | Details panel |
  | `Ctrl+,` | Settings |

## Install

Grab the latest installer from [Releases](../../releases) (the `Haul_x.y.z_x64-setup.exe`), or install it from [Hangar](https://github.com/larosa023/Hangar). It installs for your user only, with no admin prompt.

**Portable:** download `Haul.exe` from the release, put it in a folder, and create an empty file named `portable` next to it. Settings and downloads data then stay in a `data` folder beside the exe.

### Browser extension

1. In Haul, open **Settings → Browser** and click **Open extension folder** (or unzip `Haul-extension.zip` from the release).
2. Open `chrome://extensions` (Edge: `edge://extensions`), turn on **Developer mode**, click **Load unpacked** and choose that folder.

When Haul isn't running, the browser downloads as usual.

### Themes

Haul reads theme files from `%LOCALAPPDATA%\Haul\themes\` and from Hangar's `%LOCALAPPDATA%\Hangar\themes\`, in the [shared format](https://github.com/larosa023/Hangar/blob/main/docs/THEMES.md). **Settings → Appearance → Save as file** starts one from the current theme.

## Where things are

Everything is in `%LOCALAPPDATA%\Haul\` (or `.\data\` in portable mode): `settings.json`, `items.json` (the list), `session\` (torrent state and fast-resume), `torrents\` (a copy of every .torrent), `themes\`, `updates\` and `haul.log`.

## Develop

Requirements: Node 20+, Rust (stable), and the WebView2 runtime (preinstalled on Windows 10/11).

```bash
npm install
npm run tauri dev     # run the app with hot reload
npm run tauri build   # produce the installer in src-tauri/target/release/bundle
npm run check         # type-check the UI
npm test              # UI unit tests
cd src-tauri && cargo test --lib   # backend tests (includes a real multi-connection download)
```

### Project layout

```
src/
  lib/themecore.js   the suite's 14 themes, accents and the Hangar theme format
  lib/state.svelte.ts list, selection and events from the backend
  lib/components/    sidebar, list, details, add / torrent / remove dialogs, banners
  routes/Settings.svelte
src-tauri/src/
  engine.rs          the queue, limits, schedule, seeding rules and finished-download steps
  http.rs            multi-connection, resumable direct downloads
  debrid.rs secret.rs  Real-Debrid, and the encrypted API key
  torrent.rs         the librqbit session (DHT, PEX, encryption, UPnP, NAT-PMP, blocklists, adapter)
  sorter.rs extract.rs  sorting rules, and zip / 7z / rar extraction
  bridge.rs          the local endpoint the browser extension talks to
  clipboard.rs notify.rs tray.rs assoc.rs  Windows integration
  updater.rs guard.rs  updates with checksum checks and automatic rollback
src-tauri/vendor/    librqbit with Haul's patches (see vendor/README.md)
extension/           the Chromium extension
```

Releases: every push builds and tests Haul on Windows. A push to `main` whose version (in `src-tauri/tauri.conf.json`) doesn't have a release yet publishes one, with the installer, a portable `Haul.exe`, the extension and `SHA256SUMS.txt`. A version with a `-` (like `0.2.0-beta.1`) is published as a pre-release for the Beta channel. Bump the version (also in `package.json` and `src-tauri/Cargo.toml`) to release; installed copies then update themselves.
