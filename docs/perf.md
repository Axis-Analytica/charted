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
