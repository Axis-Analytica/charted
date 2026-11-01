//! File and folder dialogs. On Linux through xdg-desktop-portal, so they look and behave
//! like the rest of the desktop (Omarchy ships the Hyprland and GTK portal backends);
//! elsewhere the system's own dialogs, through Tauri's dialog plugin.

#[cfg(target_os = "linux")]
use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickOptions {
    pub title: String,
    #[serde(default)]
    pub directory: bool,
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// Choose where to save a new file, suggesting this name.
    #[serde(default)]
    pub save_as: Option<String>,
}

#[cfg(target_os = "linux")]
pub async fn pick(_app: tauri::AppHandle, options: PickOptions) -> Result<Option<String>, String> {
    if let Some(name) = &options.save_as {
        let mut request = SelectedFiles::save_file()
            .title(options.title.as_str())
            .modal(true)
            .current_name(name.as_str());
        for f in &options.filters {
            let mut filter = FileFilter::new(&f.name);
            for ext in &f.extensions {
                filter = filter.glob(&format!("*.{ext}"));
            }
            request = request.filter(filter);
        }
        let response = request
            .send()
            .await
            .map_err(|e| {
                format!("the file chooser is unavailable: {e}. Is xdg-desktop-portal running?")
            })?
            .response();
        return chosen(response);
    }
    let mut request = SelectedFiles::open_file()
        .title(options.title.as_str())
        .modal(true)
        .multiple(false)
        .directory(options.directory);
    for f in &options.filters {
        let mut filter = FileFilter::new(&f.name);
        for ext in &f.extensions {
            filter = filter.glob(&format!("*.{ext}"));
        }
        request = request.filter(filter);
    }
    let response = request
        .send()
        .await
        .map_err(|e| {
            format!("the file chooser is unavailable: {e}. Is xdg-desktop-portal running?")
        })?
        .response();
    chosen(response)
}

#[cfg(target_os = "linux")]
fn chosen(response: ashpd::Result<SelectedFiles>) -> Result<Option<String>, String> {
    match response {
        Ok(files) => Ok(files
            .uris()
            .first()
            .and_then(|u| url::Url::parse(u.as_str()).ok())
            .and_then(|u| u.to_file_path().ok())
            .map(|p| p.display().to_string())),
        // Cancelled by the user.
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => Ok(None),
        Err(e) => Err(format!("the file chooser failed: {e}")),
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn pick(app: tauri::AppHandle, options: PickOptions) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    // The blocking calls wait for the dialog; they must stay off the main thread.
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title(&options.title);
        for f in &options.filters {
            let extensions: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&f.name, &extensions);
        }
        let picked = if let Some(name) = &options.save_as {
            dialog.set_file_name(name).blocking_save_file()
        } else if options.directory {
            dialog.blocking_pick_folder()
        } else {
            dialog.blocking_pick_file()
        };
        Ok(picked
            .and_then(|p| p.into_path().ok())
            .map(|p| p.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}
