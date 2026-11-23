mod db;
mod models;
mod player;
mod process;
mod scanner;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use models::{AppInfo, LibraryPayload, LibrarySource, MetadataPatch, PlayerSnapshot, ScanReport};
use player::PlayerManager;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

struct AppState {
    db_path: PathBuf,
    app_data_dir: PathBuf,
    player: PlayerManager,
}

#[tauri::command]
async fn pick_media_root(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app.dialog().file().blocking_pick_folder();
    picked
        .map(|value| {
            value
                .into_path()
                .map(|path| path.to_string_lossy().to_string())
                .map_err(|error| error.to_string())
        })
        .transpose()
}

#[tauri::command]
async fn pick_image(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Images", &["jpg", "jpeg", "png", "webp", "bmp"])
        .blocking_pick_file();
    picked
        .map(|value| {
            value
                .into_path()
                .map(|path| path.to_string_lossy().to_string())
                .map_err(|error| error.to_string())
        })
        .transpose()
}

#[tauri::command]
fn get_app_info(app: AppHandle, state: State<'_, AppState>) -> Result<AppInfo, String> {
    let conn = db::open(&state.db_path)?;
    let media_root = db::get_setting(&conn, "media_root")?;
    let sources = db::list_library_sources(&conn)?;
    for source in &sources {
        let _ = app.asset_protocol_scope().allow_directory(&source.path, true);
    }
    let artwork_root = managed_artwork_root(&conn, &state.app_data_dir);
    fs::create_dir_all(&artwork_root).map_err(|e| e.to_string())?;
    let _ = app.asset_protocol_scope().allow_directory(&artwork_root, true);
    Ok(AppInfo {
        media_root,
        library_source_count: sources.len(),
        mpv_available: find_tool(&app, "mpv").is_some(),
        ffprobe_available: find_tool(&app, "ffprobe").is_some(),
    })
}

#[tauri::command]
async fn scan_library(app: AppHandle, state: State<'_, AppState>) -> Result<ScanReport, String> {
    // A full scan can touch hundreds of files, run ffprobe/ffmpeg, and update SQLite.
    // Never do that work on Tauri's UI thread.
    let db_path = state.db_path.clone();
    let app_data_dir = state.app_data_dir.clone();

    tauri::async_runtime::spawn_blocking(move || {
        scan_internal(&app, &db_path, &app_data_dir)
    })
    .await
    .map_err(|e| format!("Library scan worker failed: {e}"))?
}

fn scan_internal(
    app: &AppHandle,
    db_path: &Path,
    app_data_dir: &Path,
) -> Result<ScanReport, String> {
    let conn = db::open(db_path)?;
    let sources = db::list_library_sources(&conn)?;
    let enabled_sources: Vec<(PathBuf, String)> = sources
        .into_iter()
        .filter(|source| source.enabled)
        .map(|source| (PathBuf::from(source.path), source.source_type))
        .collect();
    if enabled_sources.is_empty() {
        return Err("Add at least one media folder first.".into());
    }
    for (source, _) in &enabled_sources {
        app.asset_protocol_scope()
            .allow_directory(source, true)
            .map_err(|e| e.to_string())?;
    }
    let artwork_root = managed_artwork_root(&conn, app_data_dir);
    fs::create_dir_all(&artwork_root).map_err(|e| e.to_string())?;
    app.asset_protocol_scope()
        .allow_directory(&artwork_root, true)
        .map_err(|e| e.to_string())?;
    let ffprobe = find_tool(app, "ffprobe");
    let ffmpeg = find_tool(app, "ffmpeg");
    scanner::scan_sources(
        &conn,
        &artwork_root,
        &enabled_sources,
        ffprobe.as_deref(),
        ffmpeg.as_deref(),
        Utc::now().timestamp(),
    )
}

#[tauri::command]
fn list_library_sources(state: State<'_, AppState>) -> Result<Vec<LibrarySource>, String> {
    let conn = db::open(&state.db_path)?;
    db::list_library_sources(&conn)
}

#[tauri::command]
fn add_library_source(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    source_type: String,
) -> Result<(), String> {
    let source = PathBuf::from(&path);
    if !source.is_dir() {
        return Err("The selected library folder does not exist.".into());
    }
    app.asset_protocol_scope()
        .allow_directory(&source, true)
        .map_err(|e| e.to_string())?;
    let conn = db::open(&state.db_path)?;
    db::add_library_source(&conn, &path, &source_type)
}

#[tauri::command]
fn update_library_source_type(
    state: State<'_, AppState>,
    id: i64,
    source_type: String,
) -> Result<(), String> {
    let conn = db::open(&state.db_path)?;
    db::update_library_source_type(&conn, id, &source_type)
}

#[tauri::command]
fn remove_library_source(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = db::open(&state.db_path)?;
    db::remove_library_source(&conn, id)
}

#[tauri::command]
fn get_library(state: State<'_, AppState>) -> Result<LibraryPayload, String> {
    let conn = db::open(&state.db_path)?;
    db::library(&conn)
}

#[tauri::command]
fn update_metadata(
    state: State<'_, AppState>,
    entity_type: String,
    id: i64,
    patch: MetadataPatch,
) -> Result<(), String> {
    let conn = db::open(&state.db_path)?;
    match entity_type.as_str() {
        "media" => db::update_media_metadata(&conn, id, &patch),
        "series" => db::update_series_metadata(&conn, id, &patch),
        _ => Err("entity_type must be media or series".into()),
    }
}


#[tauri::command]
fn reset_title(
    state: State<'_, AppState>,
    entity_type: String,
    id: i64,
) -> Result<(), String> {
    let conn = db::open(&state.db_path)?;
    db::reset_entity_title(&conn, &entity_type, id)
}

#[tauri::command]
fn set_artwork(
    app: AppHandle,
    state: State<'_, AppState>,
    entity_type: String,
    id: i64,
    art_kind: String,
    source_path: String,
) -> Result<String, String> {
    let source = PathBuf::from(&source_path);
    if !source.is_file() {
        return Err("The selected image does not exist.".into());
    }
    let ext = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp") {
        return Err("Choose a JPG, PNG, WebP or BMP image.".into());
    }

    let conn = db::open(&state.db_path)?;
    let root = managed_artwork_root(&conn, &state.app_data_dir);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let category = if entity_type == "series" {
        "Series".to_string()
    } else if entity_type == "music_collection" {
        "Music Collections".to_string()
    } else {
        let media = db::get_media(&conn, id)?;
        match media.kind.as_str() {
            "song" => "Songs",
            "atmosphere" => "Atmospheres",
            "childhood" => "Childhood",
            "episode" => "Episodes",
            _ => "Movies",
        }
        .to_string()
    };
    let dest_dir = root.join("Posters").join(category);
    fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest = dest_dir.join(format!("{entity_type}-{id}-{art_kind}.{ext}"));
    fs::copy(&source, &dest).map_err(|e| format!("Could not copy artwork: {e}"))?;
    let dest_text = dest.to_string_lossy().to_string();

    if entity_type == "series" {
        db::set_series_artwork(&conn, id, &art_kind, &dest_text)?;
    } else if entity_type == "music_collection" {
        if art_kind != "poster" && art_kind != "cover" {
            return Err("Music collections support poster/cover artwork.".into());
        }
        db::set_music_collection_artwork(&conn, id, &dest_text)?;
    } else {
        db::set_media_artwork(&conn, id, &art_kind, &dest_text)?;
    }
    app.asset_protocol_scope()
        .allow_file(&dest)
        .map_err(|e| e.to_string())?;
    Ok(dest_text)
}


#[tauri::command]
async fn prepare_details_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    media_id: i64,
) -> Result<Option<String>, String> {
    let ffmpeg = match find_tool(&app, "ffmpeg") {
        Some(path) => path,
        None => return Ok(None),
    };

    let conn = db::open(&state.db_path)?;
    let media = db::get_media(&conn, media_id)?;
    if !matches!(media.kind.as_str(), "movie" | "episode" | "childhood") {
        return Ok(None);
    }

    let input = PathBuf::from(&media.file_path);
    if !input.is_file() {
        return Ok(None);
    }

    let cache_dir = state.app_data_dir.join("Preview Cache").join("Details");
    fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    app.asset_protocol_scope()
        .allow_directory(&cache_dir, true)
        .map_err(|e| e.to_string())?;

    // Include modified_at so replacing the source file invalidates the old preview.
    let output = cache_dir.join(format!("{}-{}.mp4", media.id, media.modified_at));
    if output.is_file() {
        return Ok(Some(output.to_string_lossy().to_string()));
    }

    let duration = media.duration.max(0.0);
    let preview_len = if duration > 14.0 { 10.0 } else { (duration - 1.0).max(4.0) };
    let safe_end = (duration - preview_len - 1.0).max(0.0);
    let seed = ((media.id.unsigned_abs() % 37) as f64) / 37.0;
    let desired = duration * (0.20 + seed * 0.34);
    let start_at = desired.min(safe_end).max(0.0);

    let ffmpeg_for_job = ffmpeg.clone();
    let input_for_job = input.clone();
    let output_for_job = output.clone();

    let status = tauri::async_runtime::spawn_blocking(move || {
        process::hidden_command(ffmpeg_for_job)
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{start_at:.3}"))
            .arg("-i")
            .arg(input_for_job)
            .arg("-t")
            .arg(format!("{preview_len:.3}"))
            .arg("-an")
            .arg("-sn")
            .arg("-dn")
            .arg("-vf")
            .arg("scale='min(960,iw)':-2")
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("ultrafast")
            .arg("-crf")
            .arg("28")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-movflags")
            .arg("+faststart")
            .arg(&output_for_job)
            .status()
            .map_err(|e| format!("Could not start ffmpeg preview generation: {e}"))
    })
    .await
    .map_err(|e| format!("Preview worker failed: {e}"))??;

    if status.success() && output.is_file() {
        Ok(Some(output.to_string_lossy().to_string()))
    } else {
        let _ = fs::remove_file(&output);
        Ok(None)
    }
}

#[tauri::command]
fn open_player(
    app: AppHandle,
    state: State<'_, AppState>,
    media_id: i64,
) -> Result<(), String> {
    let mpv = find_tool(&app, "mpv").ok_or(
        "mpv was not found. Put mpv.exe in src-tauri/bin before building, or install mpv and add it to PATH.",
    )?;
    player::start_player(
        &app,
        &state.player,
        &state.db_path,
        &mpv,
        media_id,
    )
}

#[tauri::command]
fn get_player_snapshot(state: State<'_, AppState>) -> Result<PlayerSnapshot, String> {
    player::snapshot(&state.player, &state.db_path)
}

#[tauri::command]
fn player_action(
    state: State<'_, AppState>,
    action: String,
    value: Option<f64>,
) -> Result<(), String> {
    player::action(&state.player, &action, value)
}

#[tauri::command]
fn select_player_track(
    state: State<'_, AppState>,
    track_type: String,
    id: i64,
) -> Result<(), String> {
    player::select_track(&state.player, &track_type, id)
}

#[tauri::command]
fn close_player(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    player::stop_player(&app, &state.player, &state.db_path, true)
}

#[tauri::command]
fn atmosphere_fullscreen_active(state: State<'_, AppState>) -> bool {
    player::atmosphere_fullscreen_active(&state.player)
}

#[tauri::command]
fn open_atmosphere_fullscreen(
    app: AppHandle,
    state: State<'_, AppState>,
    atmosphere_id: i64,
    soundtrack_id: Option<i64>,
    soundtrack_ids: Option<Vec<i64>>,
    shuffle_soundtracks: Option<bool>,
) -> Result<(), String> {
    let mpv = find_tool(&app, "mpv").ok_or(
        "mpv was not found. Put mpv.exe in src-tauri/bin before building, or install mpv and add it to PATH.",
    )?;
    let conn = db::open(&state.db_path)?;
    let atmosphere = db::get_media(&conn, atmosphere_id)?;
    if atmosphere.kind != "atmosphere" {
        return Err("Selected media is not an atmosphere.".into());
    }
    let mut ids = soundtrack_ids.unwrap_or_default();
    if ids.is_empty() {
        if let Some(id) = soundtrack_id {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    ids.dedup();

    let mut soundtracks = Vec::new();
    for id in ids {
        let media = db::get_media(&conn, id)?;
        if media.kind != "song" {
            return Err("Selected soundtrack is not a song.".into());
        }
        soundtracks.push(PathBuf::from(media.file_path));
    }

    player::atmosphere_fullscreen(
        &app,
        &state.player,
        &state.app_data_dir,
        &mpv,
        Path::new(&atmosphere.file_path),
        &soundtracks,
        shuffle_soundtracks.unwrap_or(false),
    )
}

fn managed_artwork_root(conn: &rusqlite::Connection, app_data_dir: &Path) -> PathBuf {
    // Preserve older installs that already used <legacy root>/Posters so existing
    // artwork stays in the same place. New installs use app-local managed storage.
    if let Ok(Some(root)) = db::get_setting(conn, "media_root") {
        let path = PathBuf::from(root);
        if path.is_dir() {
            return path;
        }
    }
    app_data_dir.join("Managed Library")
}

fn find_tool(app: &AppHandle, name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };

    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("bin").join(&exe_name));
        candidates.push(resource_dir.join(&exe_name));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin").join(&exe_name));
    if let Ok(current) = env::current_exe() {
        if let Some(parent) = current.parent() {
            candidates.push(parent.join(&exe_name));
            candidates.push(parent.join("bin").join(&exe_name));
        }
    }
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            candidates.push(dir.join(&exe_name));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_local_data_dir()?;
            fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("hedspace.db");
            let conn = db::open(&db_path).map_err(std::io::Error::other)?;
            for source in db::list_library_sources(&conn).map_err(std::io::Error::other)? {
                let _ = app.asset_protocol_scope().allow_directory(source.path, true);
            }
            let artwork_root = managed_artwork_root(&conn, &app_data_dir);
            fs::create_dir_all(&artwork_root)?;
            let _ = app.asset_protocol_scope().allow_directory(&artwork_root, true);
            let preview_cache = app_data_dir.join("Preview Cache");
            fs::create_dir_all(&preview_cache)?;
            let _ = app.asset_protocol_scope().allow_directory(&preview_cache, true);
            drop(conn);
            app.manage(AppState {
                db_path,
                app_data_dir,
                player: PlayerManager::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_media_root,
            pick_image,
            get_app_info,
            scan_library,
            list_library_sources,
            add_library_source,
            update_library_source_type,
            remove_library_source,
            get_library,
            update_metadata,
            reset_title,
            set_artwork,
            prepare_details_preview,
            open_player,
            get_player_snapshot,
            player_action,
            select_player_track,
            close_player,
            atmosphere_fullscreen_active,
            open_atmosphere_fullscreen
        ])
        .run(tauri::generate_context!())
        .expect("error while running HED Space");
}
