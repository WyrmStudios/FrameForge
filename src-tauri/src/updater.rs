use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use url::Url;

// Floating pointers mirroring the stable endpoints in tauri.conf.json, but
// published only for prerelease (beta/dev) builds — see
// scripts/mirror-release.ps1 and build-and-release.bat. Kept out of the
// static config so stable users never resolve against them.
const BETA_ENDPOINTS: &[&str] = &[
    "https://github.com/WyrmStudios/FrameForge/releases/download/latest-beta/latest.json",
    "https://codeberg.org/Sikewyrm/FrameForge/releases/download/latest-beta/latest.json",
    "https://gitlab.com/api/v4/projects/WyrmStudios%2FFrameForge/packages/generic/frameforge/latest-beta/latest.json",
];

fn build_updater(app: &tauri::AppHandle, beta: bool) -> Result<Updater, String> {
    let builder = app.updater_builder();
    if beta {
        let endpoints = BETA_ENDPOINTS
            .iter()
            .map(|u| Url::parse(u).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        builder
            .endpoints(endpoints)
            .map_err(|e| e.to_string())?
            .build()
            .map_err(|e| e.to_string())
    } else {
        builder.build().map_err(|e| e.to_string())
    }
}

async fn check(app: &tauri::AppHandle, beta: bool) -> Result<Option<Update>, String> {
    build_updater(app, beta)?
        .check()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn check_for_update(app: tauri::AppHandle, beta: bool) -> Result<Option<String>, String> {
    Ok(check(&app, beta).await?.map(|update| update.version))
}

#[tauri::command]
pub(crate) async fn install_update(app: tauri::AppHandle, beta: bool) -> Result<(), String> {
    if let Some(update) = check(&app, beta).await? {
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}
