# Performance notes

## M0 WebKitGTK spike (2026-10-25)

Machine: aarch64 (8 cores, 7 GB RAM), Omarchy, Hyprland, WebKitGTK 2.54.1, HiDPI (scale 2),
full-height tiled window (1800×2060 device pixels). Debug build of the Rust side.

| Check | Result |
|---|---|
| Glide Data Grid, 1M rows × 8 columns, continuous programmatic scrolling | frame p50 7 ms, p95 18 ms, 1/240 frames over 33 ms |
| ECharts, 100k-point scatter + 100k-point line (LTTB), first render | 42 ms; idle frames p95 8 ms |
| Monaco editor + WebAssembly module in a module worker | works; `instantiateStreaming` succeeds (correct `application/wasm` MIME) |
| DuckDB → Arrow IPC → webview, 1M rows (41.8 MiB) | 857 ms query + transfer (debug), 52 ms decode + scan |

Conclusion: WebKitGTK on aarch64 is fast enough for the planned UI stack. Large results
should stay server-side and travel as aggregates or row blocks; a full 1M-row transfer is
an upper bound, not a pattern.

Reproduce: `VITE_SPIKE=1 VITE_SPIKE_AUTO=1 VITE_SPIKE_TAB=<grid|chart|editor|arrow> pnpm --dir ui exec vite`
then `CHARTED_SCREENSHOT_DELAY_MS=16000 target/debug/charted open --screenshot out.png`.

## v1.0 quality bars (M6, 2026-10-26)

Release build (`bin/build`), aarch64, 7 GB RAM, Hyprland.

| Bar | Target | Measured |
|---|---|---|
| Startup to a painted project window | < 1 s | ~0.35 s (`charted open <project> --screenshot` with a 1.5 s delay took 1.86 s in all) |
| Initial JS | < 1.2 MB gzipped | 473 KB gzipped (Monaco, the Data, Model and Query views load on first use: 861 KB, 168 KB, 9 KB, 7 KB) |
| Cross-filter query, 10M rows | < 250 ms | 20 ms filtered line, 49 ms highlighted column (p50; p95 21 / 50 ms) |
| Visual query p95, 10M rows | < 800 ms | 22–168 ms typical visuals; 369 ms for a 1,000-row product table |
| Idle memory | < 300 MB | 234 MB private (anonymous) in the charted process; RSS 393 MB counts 158 MB of shared libraries (100 MB is libduckdb). WebKit's web process adds ~300 MB RSS of its own. |

Visual queries: `cargo run --release -p charted-core --example visualbench -- "Harbor & Pine 10M" 5`.
Memory: `/proc/<pid>/status` (RssAnon, RssFile) 10 s after opening the demo sample.
