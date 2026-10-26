//! Export to PDF: WebKitGTK prints the UI's print layout (every page not hidden, laid
//! out one per sheet with CSS `@page`) to a PDF file, without a dialog. The UI opens the
//! layout and waits for every visual to load before calling `print_pdf`.
//! `charted open <project> --export-pdf FILE` asks the UI to do the same, then exits.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use charted_core::ApiError;
use tauri::{AppHandle, Manager};

thread_local! {
    /// The print operation running (kept alive until it finishes).
    static RUNNING: RefCell<Option<webkit2gtk::PrintOperation>> = const { RefCell::new(None) };
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError {
        code: "pdf".into(),
        message: message.into(),
        detail: None,
    }
}

/// Prints the webview to `path` as a PDF whose sheets are `width` × `height` CSS pixels.
#[tauri::command]
pub async fn print_pdf(
    window: tauri::WebviewWindow,
    path: String,
    width: f64,
    height: f64,
) -> Result<(), ApiError> {
    let file = PathBuf::from(&path);
    let file = if file.is_absolute() {
        file
    } else {
        std::env::current_dir().unwrap_or_default().join(file)
    };
    let uri = url::Url::from_file_path(&file)
        .map_err(|_| failed(format!("{path} isn't a file path")))?
        .to_string();
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    window
        .with_webview(move |webview| {
            use webkit2gtk::{PrintOperation, PrintOperationExt};
            let op = PrintOperation::new(&webview.inner());
            let settings = gtk::PrintSettings::new();
            settings.set_printer("Print to File");
            settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
            settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&uri));
            // 1 CSS pixel is 0.75 points: the sheet matches the report page.
            let paper = gtk::PaperSize::new_custom(
                "charted-page",
                "Report page",
                width * 0.75,
                height * 0.75,
                gtk::Unit::Points,
            );
            let setup = gtk::PageSetup::new();
            setup.set_orientation(gtk::PageOrientation::Portrait);
            setup.set_paper_size(&paper);
            setup.set_top_margin(0.0, gtk::Unit::Points);
            setup.set_bottom_margin(0.0, gtk::Unit::Points);
            setup.set_left_margin(0.0, gtk::Unit::Points);
            setup.set_right_margin(0.0, gtk::Unit::Points);
            settings.set_orientation(setup.orientation());
            settings.set_paper_size(&setup.paper_size());
            op.set_print_settings(&settings);
            op.set_page_setup(&setup);
            let on_fail = tx.clone();
            op.connect_failed(move |_, e| {
                let _ = on_fail.send(Err(e.to_string()));
            });
            op.connect_finished(move |_| {
                let _ = tx.send(Ok(()));
                RUNNING.with(|r| r.borrow_mut().take());
            });
            op.print();
            RUNNING.with(|r| *r.borrow_mut() = Some(op));
        })
        .map_err(|e| failed(e.to_string()))?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(300))
            .map_err(|_| "printing took too long".to_string())?
    })
    .await
    .map_err(|e| failed(e.to_string()))?;
    result.map_err(failed)?;
    if !file.exists() {
        return Err(failed(format!("WebKitGTK wrote no file at {path}")));
    }
    Ok(())
}

/// Exits with a code (the command line's PDF export when it is done).
#[tauri::command]
pub fn exit_with(handle: AppHandle, code: i32) {
    if code == 0
        && let Some(file) = handle.try_state::<ExportTarget>()
    {
        println!("{}", file.0.display());
    }
    handle.exit(code);
}

/// The PDF the command line asked for.
pub struct ExportTarget(pub PathBuf);

/// `--export-pdf`: once the UI is up, asks it (repeatedly, until it starts) to export
/// the open project, and gives up after `CHARTED_EXPORT_TIMEOUT_S` (default 300 s).
pub fn schedule(handle: AppHandle, file: PathBuf) {
    let file = if file.is_absolute() {
        file
    } else {
        std::env::current_dir().unwrap_or_default().join(file)
    };
    handle.manage(ExportTarget(file.clone()));
    std::thread::spawn(move || {
        let timeout = std::env::var("CHARTED_EXPORT_TIMEOUT_S")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let start = Instant::now();
        let path = serde_json::to_string(&file.display().to_string()).unwrap_or_default();
        // The UI defines the entry point once the report view is up; the flag makes
        // later calls no-ops.
        let script = format!(
            "window.__chartedExportPdf && document.querySelector('.report-view') && !window.__chartedExportStarted && (window.__chartedExportStarted = true, window.__chartedExportPdf({path}))"
        );
        while start.elapsed() < Duration::from_secs(timeout) {
            std::thread::sleep(Duration::from_millis(1000));
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.eval(&script);
            }
        }
        eprintln!("charted: the PDF export didn't finish within {timeout} s");
        handle.exit(1);
    });
}
