# Vendored crates

Haul uses [librqbit](https://github.com/ikatson/rqbit) 9.1.0 and librqbit-dualstack-sockets 0.7.0 with a few patches. They're wired in with `[patch.crates-io]` in `src-tauri/Cargo.toml`. Every change is marked with a `Haul:` comment.

## librqbit

- **Protocol encryption (MSE/PE)**: `src/mse.rs`, hooked into outgoing connections (`peer_connection.rs`) and incoming ones (`session.rs`). Global mode: Prefer (try encrypted, fall back to plain), Require, or Disabled. Tested with a real handshake in `mse::tests`.
- **Per-torrent live settings** on `TorrentStateLive`: `set_ratelimits`, `set_file_priorities` (High/Normal/Low order) and `set_sequential`.
- **Piece order**: sequential mode walks each file in order (first and last piece early); otherwise each peer starts at its own offset in the file, which spreads the swarm like rarest-first. `chunk_tracker.rs`, `piece_tracker.rs`.
- **`SessionOptions::bind_local_addr`**: binds HTTP tracker requests to the chosen adapter's address on Windows.
- The web UI and examples were removed.

## librqbit-dualstack-sockets

- **Windows adapter binding**: `BindDevice` takes the adapter's interface index and sets `IP_UNICAST_IF` / `IPV6_UNICAST_IF`, so peer connections, DHT and UDP trackers leave through that adapter (upstream supports this on Linux and macOS only).
