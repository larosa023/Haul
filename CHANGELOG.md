# Changelog

## 0.4.8

- Jackett is built in too, the same way as Prowlarr: Haul finds it, reads its key, starts it hidden when you search and searches every site you've added in it (its "all" feed). Settings → Torrent search shows how many sites it has, with an **Add sites…** button.
- If Jackett has no FlareSolverr address set (and no admin password), Haul fills in FlareSolverr's address for it.

## 0.4.7

- Settings → Torrent search: **Add sites…** opens Prowlarr's page again (the click did nothing because Haul only opened https links). It starts Prowlarr first if it isn't running.

## 0.4.6

- Prowlarr is built in. Haul finds Prowlarr on your PC, reads its address and API key by itself, starts it hidden when you search (no window or browser tab to keep open) and searches every site you've added to it in one go. No Torznab links or keys to copy any more; links added by hand to the same Prowlarr are skipped so nothing is searched twice.
- Sites Prowlarr has paused after errors are re-tested before each search, so they come back on their own.
- Haul also finds FlareSolverr one folder deeper in Downloads, Desktop and Documents.

## 0.4.5

- Torrent search: while a search runs, the previous results fade out and Haul says what it's searching for (and why when it's slow), so old results can't be mistaken for new ones. The result count says which search it's for.
- When Prowlarr has benched an indexer after failures (for example while FlareSolverr was still starting), Haul asks Prowlarr to re-test its indexers and searches again by itself. If it's still benched, the message says so in plain words.

## 0.4.4

- The details panel hides when nothing is selected and nothing is downloading or uploading, so the list gets the whole window. It comes back when something starts or when you select a download.
- Blocks style: a near-zero speed no longer draws a stray bar in the corner of the graph.

## 0.4.3

- The details panel follows your download tracking style too: the speed graph is a soft glow, a plain line, moving stripes, an aurora, retro bars, a hairline or neon, and the live card changes to match. The style tiles in Settings → Appearance preview the graph as well.
- With nothing selected, the details panel shows a live overview instead of an empty space: total speed with a graph, peak, what's downloading, queued and seeding, what finished today, and the active downloads (click one to open it).

## 0.4.2

- Torrent search: PC and console games are labelled **Games** (they showed as Software).
- FlareSolverr starts by itself: before a search Haul checks it's running and, if not, starts it hidden (no window). Haul finds it in your Startup folder or the usual unzip places, or you can point to it in Settings → Torrent search.

## 0.4.1

- Torrent search: when a site says it's getting too many searches (429), Haul waits and tries again by itself, and otherwise explains it in plain words.

## 0.4.0

- **Torrent search** (sidebar → Search, or `Ctrl+K`). Searches Internet Archive (built in) and any Torznab indexers you add from Prowlarr or Jackett, all at once. Results are merged, de-duplicated and sorted by seeders, with size, age and a seed-health dot; filter by category or text, and download straight away (or through your debrid service). Add sources in Settings → Torrent search.
- **More debrid services:** AllDebrid, Premiumize and TorBox, alongside Real-Debrid. Each keeps its own key; pick the one Haul uses in Settings → Debrid. File hoster links and torrents work with all four (choosing files before the download starts is Real-Debrid only).
- **Duplicate warning:** the New download and video windows tell you when you already downloaded a link (with Open / Show it), batches can skip links already in your list, and a link from the browser or clipboard that you already have opens the New download window so you see the warning.
- **Download tracking styles:** Glow, Classic, Striped, Aurora, Blocks, Minimal or Neon, with live previews in Settings → Appearance. They apply to the list, the connection map and the mini window.

## 0.3.2

- Safety score: the "digitally signed by…" check for programs now works on every PC (it could fail when PowerShell 7 is installed). Programs signed by a verified publisher score higher.

## 0.3.1

- **Safety score.** Every download gets a grade from A (looks safe) to F (likely dangerous), with the reasons listed under it.
  - When you paste a link: is it encrypted, is the site a bare IP, a look-alike name, a short link or a scam-prone domain, is it a program, and is the name disguised (like `invoice.pdf.exe` or hidden reversed text)?
  - When it finishes: does the content match its name (a program posing as a video gets an F), is a program digitally signed and by whom, what's inside archives (programs, disguised names, passwords), and a Windows Defender scan. Dangerous files aren't auto-extracted, and you get a warning.
  - Optional VirusTotal check with your own free key (only the file's SHA-256 is sent).
  - Downloads are now marked as from the internet, like browsers do, so SmartScreen and Office Protected View work on them.
  - Settings → Safety turns each part on or off.
- **Live download view.** A live speed graph with peak and average, and a map of every connection filling its part of the file. Running progress bars glow and shimmer, there's a speed graph behind the totals in the sidebar and in the mini window.
- **Speeds in Mbps** (Settings → Appearance), the unit internet plans use. 111 MB/s is about 930 Mbps.

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
