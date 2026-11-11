# max-explorer

A small, independent web browser with tabs — built for KDE (Linux) and planned
for Windows, then macOS and Android.

**Status:** pre-alpha. Runs on Linux: one window, tab strip, address bar, live
Chromium pages via CEF. No history, bookmarks, downloads or settings yet.

- Engine: Chromium via [CEF](https://bitbucket.org/chromiumembedded/cef) (Rust bindings: `cef`, Tauri team)
- Language: Rust — `crates/core` (product logic) + `crates/app` (CEF shell, CEF Views)
- Not Electron — see "نقش و استک" in `AGENTS.md` for why

## Run it

```bash
cargo build -p max-explorer-app         # first build downloads the CEF runtime (~1.5 GB)
./scripts/dev.sh                        # browser window
MAX_EXPLORER_SMOKE=3 ./scripts/dev.sh   # 3 colour-coded tabs, switching every 2s
MAX_EXPLORER_NO_GPU=1 ./scripts/dev.sh  # software rendering (Xvfb / no GPU)
```

`cargo run` alone does not work: CEF has to be on `LD_LIBRARY_PATH`, which
`scripts/dev.sh` handles.

Product features: [`FEATURES.md`](FEATURES.md) · Rules for AI agents: [`AGENTS.md`](AGENTS.md)
