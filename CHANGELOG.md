# Changelog

## 0.3.0

- **Video and music downloads** from YouTube and the 1,800+ other sites yt-dlp supports. Paste a link (or use the browser button) and pick the quality, up to 8K and HDR as MP4, or audio only as MP3, M4A or Opus. Options for subtitles, chapters, embedded thumbnails and SponsorBlock (skip or mark sponsor segments). Playlists and channels list their videos so you can pick which to queue. Videos go to your Video folder and audio to Music, by the sorting rules.
  - Haul downloads yt-dlp and ffmpeg the first time you need them (they aren't bundled), checks their SHA-256, and keeps yt-dlp up to date by itself, right away when a site stops working. Settings → Video & music shows their versions, with Update now and Remove.
- **HLS (.m3u8) and DASH (.mpd) streams**: the pieces download in parallel and are joined into one file.
- **FTP and FTPS** links (ftp://, ftps://, ftpes://), with multiple connections and resume.
- **Smarter resume**: before resuming, Haul checks the file hasn't changed on the server (ETag / Last-Modified). If it has, the download starts over cleanly instead of making a corrupted file.
- **Connections per download** can go up to 32, and the new default is **Auto**: 1 connection for small files, up to 24 for huge ones.
- **Batch downloads**: paste a list of links, or a pattern like `file[001-100].zip`, `part[a-f].rar` or `img[0-40:2].jpg`.
- **Site grabber**: give it a page and it lists every file the page links to, with sizes, filterable by type, size and name. Pick what to queue.
- **Hugging Face**: paste a model or dataset link (or `org/name`), pick the files (or "only .safetensors" / one GGUF), and they download over multiple connections, each checked against its SHA-256 (or git hash) when done. An optional token for gated and private models.
- **Firefox**: the browser extension now works in Firefox too, with the same features.
- **"Download video with Haul"** in the extension: a small button on video pages, a page right-click item, and a button in the popup.
- **Mini window**: a small always-on-top window with active downloads, total speed and Pause all. Toggle it from the tray or with `Ctrl+Alt+H` (change it in Settings → General).

## 0.2.0

- Real-Debrid support. Connect your account in Settings → Debrid with your API key (stored encrypted for your Windows account).
  - Links from file hosters Real-Debrid supports download at premium speed, with Haul's multiple connections. Expired links are renewed automatically.
  - Torrents can be downloaded by Real-Debrid instead of your PC: pick the files, watch Real-Debrid's progress in the list, then Haul fetches the files at full speed, keeping the torrent's folders. Choose Off, Ask each time (a switch in the New download window) or Always.
- Magnet links and .torrent files you open from your browser or Explorer show the New download window when Real-Debrid is set to Ask each time.

## 0.1.0

First release of Haul: direct downloads and torrents in one light app.

- Direct downloads split into parallel connections, resume after pauses and restarts, retry on failure, and can verify a SHA-256.
- Torrents (magnet links and .torrent files) with file picking, file priorities, sequential download, seeding ratio/time limits, DHT, PEX, encryption, UPnP and NAT-PMP, IP blocklists and binding to a network adapter (with a kill switch).
- One list for everything, with filters and search.
- Finished files sorted into folders by your rules; optional auto-extract of zip, 7z and rar.
- Global and per-download speed limits, a speed schedule and a queue.
- Browser extension for Chrome, Edge, Drift and other Chromium browsers.
- Notifications with Open and Show in folder.
- 14 suite themes, accent colors and custom theme files.
- Built-in updates with Stable and Beta channels, checksum checks and automatic rollback.
