//! `charted open --screenshot FILE`: renders the window, saves a PNG of the webview's
//! contents (independent of the compositor) and exits. Used for README images and
//! milestone checkpoints.

use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub fn schedule(handle: AppHandle, file: PathBuf) {
    std::thread::spawn(move || {
        // Give the UI time to load the project and render.
        let delay = std::env::var("CHARTED_SCREENSHOT_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4000);
        std::thread::sleep(Duration::from_millis(delay));
        // CHARTED_SCREENSHOT_SCRIPT: JavaScript to run first (e.g. to open a view), then
        // wait CHARTED_SCREENSHOT_AFTER_MS before the snapshot.
        if let Ok(script) = std::env::var("CHARTED_SCREENSHOT_SCRIPT") {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.eval(&script);
            }
            let after = std::env::var("CHARTED_SCREENSHOT_AFTER_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000);
            std::thread::sleep(Duration::from_millis(after));
        }
        let h = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            let Some(window) = h.get_webview_window("main") else {
                return;
            };
            let done = h.clone();
            let file = file.clone();
            let _ = window.with_webview(move |webview| {
                use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
                let view = webview.inner();
                view.snapshot(
                    SnapshotRegion::Visible,
                    SnapshotOptions::NONE,
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        let code = match result.and_then(|surface| {
                            let mut out = std::fs::File::create(&file).map_err(|e| {
                                glib::Error::new(glib::FileError::Failed, &e.to_string())
                            })?;
                            surface.write_to_png(&mut out).map_err(|e| {
                                glib::Error::new(glib::FileError::Failed, &e.to_string())
                            })
                        }) {
                            Ok(()) => {
                                println!("{}", file.display());
                                0
                            }
                            Err(e) => {
                                eprintln!("charted: screenshot failed: {e}");
                                1
                            }
                        };
                        done.exit(code);
                    },
                );
            });
        });
    });
}
