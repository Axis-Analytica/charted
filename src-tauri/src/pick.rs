//! File and folder dialogs through xdg-desktop-portal, so they look and behave like the
//! rest of the desktop (Omarchy ships the Hyprland and GTK portal backends).

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
}

pub async fn pick(options: PickOptions) -> Result<Option<String>, String> {
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
