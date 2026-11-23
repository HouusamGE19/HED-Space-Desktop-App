use crate::process::hidden_command;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::db;
use crate::models::{PlayerSnapshot, TrackInfo};

const HEDSPACE_OSC_LUA: &str = include_str!("../assets/hedspace-osc.lua");

pub struct PlayerSession {
    pub token: String,
    pub child: Child,
    pub pipe: String,
    pub media_id: i64,
}

pub struct AtmosphereSession {
    pub video: Child,
    pub audio: Option<Child>,
}

#[derive(Clone, Default)]
pub struct PlayerManager {
    pub session: Arc<Mutex<Option<PlayerSession>>>,
    pub atmosphere: Arc<Mutex<Option<AtmosphereSession>>>,
}

#[cfg(windows)]
#[derive(Clone, Debug)]
struct DisplayRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[cfg(windows)]
#[derive(Clone, Debug, Default)]
struct DisplayTarget {
    rect: Option<DisplayRect>,
}

#[cfg(not(windows))]
#[derive(Clone, Debug, Default)]
struct DisplayTarget {
    name: Option<String>,
    index: Option<usize>,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: WinRect,
    rc_work: WinRect,
    flags: u32,
}

#[cfg(windows)]
#[repr(C)]
struct EnumWindowData {
    pid: u32,
    hwnd: isize,
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn EnumWindows(
        callback: Option<unsafe extern "system" fn(isize, isize) -> i32>,
        lparam: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(hwnd: isize, process_id: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: isize) -> i32;
    fn MonitorFromWindow(hwnd: isize, flags: u32) -> isize;
    fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
    fn ShowWindow(hwnd: isize, command: i32) -> i32;
    fn SetWindowPos(
        hwnd: isize,
        insert_after: isize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
}

#[cfg(windows)]
fn monitor_rect_for_hwnd(hwnd: isize) -> Option<DisplayRect> {
    const MONITOR_DEFAULTTONEAREST: u32 = 0x0000_0002;

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor == 0 {
        return None;
    }

    let mut info = MonitorInfo {
        cb_size: std::mem::size_of::<MonitorInfo>() as u32,
        ..MonitorInfo::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info as *mut MonitorInfo) } == 0 {
        return None;
    }

    Some(DisplayRect {
        x: info.rc_monitor.left,
        y: info.rc_monitor.top,
        width: info.rc_monitor.right - info.rc_monitor.left,
        height: info.rc_monitor.bottom - info.rc_monitor.top,
    })
}

#[cfg(windows)]
fn current_display_target(app: &AppHandle) -> DisplayTarget {
    let Some(window) = app.get_webview_window("main") else {
        return DisplayTarget::default();
    };

    // Read the target monitor from HED Space's real native Windows HWND.
    // MonitorFromWindow chooses the monitor with the largest intersection.
    let Ok(hwnd) = window.hwnd() else {
        return DisplayTarget::default();
    };

    DisplayTarget {
        rect: monitor_rect_for_hwnd(hwnd.0 as isize),
    }
}

#[cfg(not(windows))]
fn current_display_target(app: &AppHandle) -> DisplayTarget {
    let Some(window) = app.get_webview_window("main") else {
        return DisplayTarget::default();
    };

    let monitors = window.available_monitors().unwrap_or_default();
    let selected = window.current_monitor().ok().flatten().and_then(|current| {
        monitors
            .iter()
            .enumerate()
            .find(|(_, monitor)| {
                monitor.position().x == current.position().x
                    && monitor.position().y == current.position().y
                    && monitor.size().width == current.size().width
                    && monitor.size().height == current.size().height
            })
    });

    let Some((index, monitor)) = selected else {
        return DisplayTarget::default();
    };

    DisplayTarget {
        name: monitor.name().cloned().filter(|value| !value.trim().is_empty()),
        index: Some(index),
    }
}

#[cfg(windows)]
fn apply_display_target(command: &mut std::process::Command, _target: &DisplayTarget) {
    command
        .arg("--fs-screen=current")
        .arg("--d3d11-exclusive-fs=no");
}

#[cfg(not(windows))]
fn apply_display_target(command: &mut std::process::Command, target: &DisplayTarget) {
    if let Some(name) = target.name.as_deref() {
        command
            .arg(format!("--screen-name={name}"))
            .arg(format!("--fs-screen-name={name}"));
    } else if let Some(index) = target.index {
        command
            .arg(format!("--screen={index}"))
            .arg(format!("--fs-screen={index}"));
    }
}

#[cfg(windows)]
unsafe extern "system" fn find_process_window(hwnd: isize, lparam: isize) -> i32 {
    let data = &mut *(lparam as *mut EnumWindowData);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid as *mut u32);

    if pid == data.pid && IsWindowVisible(hwnd) != 0 {
        data.hwnd = hwnd;
        return 0;
    }

    1
}

#[cfg(windows)]
fn find_process_hwnd(pid: u32) -> Option<isize> {
    for _ in 0..100 {
        let mut data = EnumWindowData { pid, hwnd: 0 };
        unsafe {
            EnumWindows(
                Some(find_process_window),
                &mut data as *mut EnumWindowData as isize,
            );
        }

        if data.hwnd != 0 {
            return Some(data.hwnd);
        }

        thread::sleep(Duration::from_millis(25));
    }

    None
}

#[cfg(windows)]
fn place_process_window_on_display(pid: u32, target: &DisplayTarget) -> Option<isize> {
    let rect = target.rect.as_ref()?;
    let hwnd = find_process_hwnd(pid)?;

    // Important: v0.6.36 moved mpv while it was minimized. Windows maintains a
    // separate restored position for minimized windows, so restoring could send
    // mpv straight back to the primary monitor. Restore first, then move.
    const SW_RESTORE: i32 = 9;
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
    }
    thread::sleep(Duration::from_millis(35));

    let margin_x = (rect.width / 16).max(24);
    let margin_y = (rect.height / 16).max(24);
    let width = (rect.width - margin_x * 2).max(320);
    let height = (rect.height - margin_y * 2).max(180);

    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_SHOWWINDOW: u32 = 0x0040;

    let moved = unsafe {
        SetWindowPos(
            hwnd,
            0,
            rect.x + margin_x,
            rect.y + margin_y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        )
    };

    if moved == 0 {
        return None;
    }

    Some(hwnd)
}

#[cfg(windows)]
fn force_window_to_display_rect(hwnd: isize, target: &DisplayTarget) {
    let Some(rect) = target.rect.as_ref() else {
        return;
    };

    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_SHOWWINDOW: u32 = 0x0040;

    unsafe {
        SetWindowPos(
            hwnd,
            0,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOZORDER | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
    }
}

fn finish_fullscreen_launch(
    pipe: &str,
    pid: u32,
    target: &DisplayTarget,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = place_process_window_on_display(pid, target);

        command(pipe, json!(["set_property", "window-minimized", false]))?;
        command(pipe, json!(["set_property", "fullscreen", true]))?;

        // mpv can briefly reposition its native window during the transition.
        // Re-apply the exact target monitor rectangle after that transition.
        if let Some(hwnd) = hwnd {
            for delay in [70_u64, 140, 220] {
                thread::sleep(Duration::from_millis(delay));
                force_window_to_display_rect(hwnd, target);
            }
        }

        command(pipe, json!(["set_property", "pause", false]))?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let _ = pid;
        let _ = target;
        command(pipe, json!(["set_property", "fullscreen", true]))?;
        command(pipe, json!(["set_property", "window-minimized", false]))?;
        command(pipe, json!(["set_property", "pause", false]))?;
        Ok(())
    }
}

#[cfg(windows)]
fn pipe_path(token: &str) -> String {
    format!(r"\\.\pipe\hedspace-mpv-{token}")
}

#[cfg(not(windows))]
fn pipe_path(token: &str) -> String {
    format!("/tmp/hedspace-mpv-{token}.sock")
}

pub fn start_player(
    app: &AppHandle,
    manager: &PlayerManager,
    db_path: &Path,
    mpv_path: &Path,
    media_id: i64,
) -> Result<(), String> {
    // v0.1.8 keeps ONE visible player window: mpv itself.
    // Previous builds tried to stack a transparent Tauri WebView2 overlay over
    // a separate mpv window. On some Windows/WebView2/GPU combinations the
    // native mpv surface always won the Z-order, so the controls could never
    // become visible or clickable. mpv's OSC is rendered inside the same video
    // surface, which removes that entire class of Windows composition bugs.
    stop_player(app, manager, db_path, true)?;

    let conn = db::open(db_path)?;
    let media = db::get_media(&conn, media_id)?;
    let resume = db::saved_position(&conn, media_id)?;
    drop(conn);

    let token = Uuid::new_v4().simple().to_string();
    let pipe = pipe_path(&token);

    // Keep keyboard controls in an explicit input.conf. The visible player UI itself is
    // rendered by our mpv Lua script *inside the same native video window*, which is
    // reliable on Windows and lets HED Space look like a streaming player without
    // stacking a WebView2 window over mpv.
    let player_dir = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let input_conf = player_dir.join("hedspace-player-input.conf");
    let input_text = concat!(
        "ESC quit\n",
        "q quit\n",
        "SPACE cycle pause\n",
        "k cycle pause\n",
        "LEFT seek -10 relative+exact\n",
        "RIGHT seek 10 relative+exact\n",
        "j seek -10 relative+exact\n",
        "l seek 10 relative+exact\n",
        "UP add volume 5\n",
        "DOWN add volume -5\n",
        "m cycle mute\n",
        "f cycle fullscreen\n",
        "a cycle audio\n",
        "s cycle sub\n",
        "v cycle sub-visibility\n"
    );
    std::fs::write(&input_conf, input_text)
        .map_err(|e| format!("Could not prepare HED Space player controls: {e}"))?;

    let osc_script = player_dir.join("hedspace-player-osc.lua");
    std::fs::write(&osc_script, HEDSPACE_OSC_LUA)
        .map_err(|e| format!("Could not prepare the HED Space player UI: {e}"))?;

    let display_title = if media.kind == "episode" {
        match (&media.series_title, media.season_number, media.episode_number) {
            (Some(series), Some(season), Some(episode)) => {
                format!("{series}  •  S{season:02}E{episode:02}  •  {}", media.title)
            }
            _ => media.title.clone(),
        }
    } else {
        media.title.clone()
    };

    let display_target = current_display_target(app);
    let mut command = hidden_command(mpv_path);
    apply_display_target(&mut command, &display_target);
    command
        .arg("--no-config")
        .arg("--terminal=no")
        .arg("--osc=no")
        .arg("--input-default-bindings=yes")
        .arg("--input-cursor=yes")
        .arg(format!("--input-conf={}", input_conf.to_string_lossy()))
        .arg(format!("--script={}", osc_script.to_string_lossy()))
        .arg(format!("--input-ipc-server={pipe}"))
        .arg("--force-window=yes")
        .arg("--fullscreen=no")
        .arg("--window-minimized=yes")
        .arg("--pause=yes")
        .arg("--border=no")
        .arg("--ontop=no")
        .arg("--keep-open=no")
        .arg("--osd-level=0")
        .arg("--osd-bar=no")
        .arg("--hwdec=auto-safe")
        .arg("--sid=no")
        // HED Space subtitle style: smaller, cleaner streaming-service look.
        // These options only affect subtitle rendering; they do not alter the video stream.
        .arg("--sub-ass-override=force")
        .arg("--sub-font=Segoe UI Semibold")
        .arg("--sub-font-size=29")
        .arg("--sub-color=#FFFFFFFF")
        .arg("--sub-outline-color=#D9000000")
        .arg("--sub-outline-size=1.15")
        .arg("--sub-shadow-offset=0.65")
        .arg("--sub-back-color=#90000000")
        .arg("--sub-margin-y=48")
        .arg("--sub-margin-x=42")
        .arg("--sub-border-style=outline-and-shadow")
        .arg("--cursor-autohide=1200")
        .arg("--title=HED Space Player")
        .arg(format!("--force-media-title={display_title}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if resume > 30.0 {
        command.arg(format!("--start={resume}"));
    }
    command.arg(&media.file_path);
    let child = command
        .spawn()
        .map_err(|e| format!("Could not start mpv: {e}"))?;
    let child_pid = child.id();

    {
        let mut guard = manager.session.lock().map_err(|_| "Player state lock poisoned")?;
        *guard = Some(PlayerSession {
            token: token.clone(),
            child,
            pipe: pipe.clone(),
            media_id,
        });
    }

    wait_for_pipe(&pipe, 50, 100)?;
    finish_fullscreen_launch(&pipe, child_pid, &display_target)?;
    spawn_progress_worker(
        app.clone(),
        manager.clone(),
        db_path.to_path_buf(),
        token,
        pipe,
        media_id,
    );
    Ok(())
}

fn spawn_progress_worker(
    app: AppHandle,
    manager: PlayerManager,
    db_path: PathBuf,
    token: String,
    pipe: String,
    media_id: i64,
) {
    thread::spawn(move || {
        let mut last_position = 0.0;
        let mut last_duration = 0.0;
        loop {
            thread::sleep(Duration::from_secs(5));
            let still_current = manager
                .session
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(|s| s.token == token))
                .unwrap_or(false);
            if !still_current {
                return;
            }

            if let Ok(position) = get_number(&pipe, "time-pos") {
                last_position = position;
            }
            if let Ok(duration) = get_number(&pipe, "duration") {
                last_duration = duration;
            }
            if last_position > 0.0 {
                if let Ok(conn) = db::open(&db_path) {
                    let _ = db::save_progress(
                        &conn,
                        media_id,
                        last_position,
                        last_duration,
                        Utc::now().timestamp(),
                    );
                }
            }

            let ended = manager
                .session
                .lock()
                .ok()
                .and_then(|mut guard| {
                    guard
                        .as_mut()
                        .filter(|s| s.token == token)
                        .and_then(|s| s.child.try_wait().ok().flatten())
                })
                .is_some();
            if ended {
                if let Ok(conn) = db::open(&db_path) {
                    let _ = db::save_progress(
                        &conn,
                        media_id,
                        last_position,
                        last_duration,
                        Utc::now().timestamp(),
                    );
                }
                if let Ok(mut guard) = manager.session.lock() {
                    if guard.as_ref().map(|s| s.token.as_str()) == Some(token.as_str()) {
                        *guard = None;
                    }
                }
                // ESC/q closes mpv and returns keyboard focus to HED Space.
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                return;
            }
        }
    });
}

pub fn snapshot(manager: &PlayerManager, db_path: &Path) -> Result<PlayerSnapshot, String> {
    let guard = manager.session.lock().map_err(|_| "Player state lock poisoned")?;
    let session = guard.as_ref().ok_or("No active player")?;
    let position = get_number(&session.pipe, "time-pos").unwrap_or(0.0);
    let duration = get_number(&session.pipe, "duration").unwrap_or(0.0);
    let paused = get_bool(&session.pipe, "pause").unwrap_or(false);
    let buffering = get_bool(&session.pipe, "paused-for-cache").unwrap_or(false);
    let volume = get_number(&session.pipe, "volume").unwrap_or(100.0);
    let tracks = get_property(&session.pipe, "track-list").unwrap_or(Value::Array(vec![]));
    let (audio_tracks, subtitle_tracks) = parse_tracks(&tracks);
    let conn = db::open(db_path)?;
    let media = db::get_media(&conn, session.media_id)?;
    let (previous_media_id, next_media_id) = db::adjacent_episode_ids(&conn, session.media_id)?;
    let next_media = next_media_id.and_then(|id| db::get_media(&conn, id).ok());
    Ok(PlayerSnapshot {
        media_id: session.media_id,
        title: media.title.clone(),
        media_kind: media.kind.clone(),
        series_title: media.series_title.clone(),
        season_number: media.season_number,
        episode_number: media.episode_number,
        position,
        duration,
        paused,
        buffering,
        volume,
        audio_tracks,
        subtitle_tracks,
        next_media_id,
        next_media_title: next_media.as_ref().map(|item| item.title.clone()),
        next_season_number: next_media.as_ref().and_then(|item| item.season_number),
        next_episode_number: next_media.as_ref().and_then(|item| item.episode_number),
        previous_media_id,
    })
}

pub fn action(manager: &PlayerManager, action: &str, value: Option<f64>) -> Result<(), String> {
    let guard = manager.session.lock().map_err(|_| "Player state lock poisoned")?;
    let session = guard.as_ref().ok_or("No active player")?;
    match action {
        "toggle_pause" => command(&session.pipe, json!(["cycle", "pause"])),
        "seek_relative" => command(
            &session.pipe,
            json!(["seek", value.unwrap_or(0.0), "relative"]),
        ),
        "seek_absolute" => command(
            &session.pipe,
            json!(["seek", value.unwrap_or(0.0), "absolute"]),
        ),
        "volume" => command(
            &session.pipe,
            json!(["set_property", "volume", value.unwrap_or(100.0).clamp(0.0, 130.0)]),
        ),
        "cycle_audio" => command(&session.pipe, json!(["cycle", "aid"])),
        "cycle_subtitle" => command(&session.pipe, json!(["cycle", "sid"])),
        "sub_off" => command(&session.pipe, json!(["set_property", "sid", "no"])),
        _ => Err(format!("Unknown player action: {action}")),
    }
}

pub fn select_track(manager: &PlayerManager, track_type: &str, id: i64) -> Result<(), String> {
    let guard = manager.session.lock().map_err(|_| "Player state lock poisoned")?;
    let session = guard.as_ref().ok_or("No active player")?;
    let property = match track_type {
        "audio" => "aid",
        "subtitle" => "sid",
        _ => return Err("Track type must be audio or subtitle".into()),
    };
    command(&session.pipe, json!(["set_property", property, id]))
}

pub fn stop_player(
    _app: &AppHandle,
    manager: &PlayerManager,
    db_path: &Path,
    _close_overlay: bool,
) -> Result<(), String> {
    let mut guard = manager.session.lock().map_err(|_| "Player state lock poisoned")?;
    if let Some(mut session) = guard.take() {
        let position = get_number(&session.pipe, "time-pos").unwrap_or(0.0);
        let duration = get_number(&session.pipe, "duration").unwrap_or(0.0);
        if position > 0.0 {
            if let Ok(conn) = db::open(db_path) {
                let _ = db::save_progress(
                    &conn,
                    session.media_id,
                    position,
                    duration,
                    Utc::now().timestamp(),
                );
            }
        }
        let _ = command(&session.pipe, json!(["quit"]));
        thread::sleep(Duration::from_millis(80));
        let _ = session.child.kill();
        let _ = session.child.wait();
    }
    drop(guard);
    Ok(())
}

pub fn atmosphere_fullscreen(
    app: &AppHandle,
    manager: &PlayerManager,
    app_data_dir: &Path,
    mpv_path: &Path,
    video_path: &Path,
    soundtrack_paths: &[PathBuf],
    shuffle_soundtracks: bool,
) -> Result<(), String> {
    // Fullscreen Atmospheres are intentionally pure video.  We use a second,
    // audio-only mpv process when a local song is selected so the soundtrack
    // can loop independently of a 30+ minute atmosphere video.
    if let Ok(mut guard) = manager.atmosphere.lock() {
        if let Some(mut session) = guard.take() {
            let _ = session.video.kill();
            let _ = session.video.wait();
            if let Some(mut audio) = session.audio.take() {
                let _ = audio.kill();
                let _ = audio.wait();
            }
        }
    }

    let input_conf = app_data_dir.join("atmosphere-input.conf");
    std::fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    std::fs::write(&input_conf, "ESC quit\nq quit\n").map_err(|e| e.to_string())?;

    let display_target = current_display_target(app);
    let atmosphere_token = Uuid::new_v4().simple().to_string();
    let atmosphere_pipe = pipe_path(&format!("atmos-{atmosphere_token}"));

    let mut video_cmd = hidden_command(mpv_path);
    apply_display_target(&mut video_cmd, &display_target);
    video_cmd
        .arg("--no-config")
        .arg("--osc=no")
        .arg("--fullscreen=no")
        .arg("--window-minimized=yes")
        .arg("--pause=yes")
        .arg("--border=no")
        .arg("--loop-file=inf")
        .arg("--hwdec=auto-safe")
        .arg("--cursor-autohide=always")
        .arg(format!("--input-conf={}", input_conf.to_string_lossy()))
        .arg(format!("--input-ipc-server={atmosphere_pipe}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !soundtrack_paths.is_empty() {
        video_cmd.arg("--mute=yes");
    }
    video_cmd.arg(video_path);
    let video = video_cmd
        .spawn()
        .map_err(|e| format!("Could not start atmosphere video in mpv: {e}"))?;
    let video_pid = video.id();

    wait_for_pipe(&atmosphere_pipe, 50, 100)?;
    finish_fullscreen_launch(&atmosphere_pipe, video_pid, &display_target)?;

    let audio = if !soundtrack_paths.is_empty() {
        let mut audio_cmd = hidden_command(mpv_path);
        audio_cmd
            .arg("--no-config")
            .arg("--no-video")
            .arg("--force-window=no")
            .arg("--really-quiet")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if shuffle_soundtracks {
            // Multiple command-line files form an mpv playlist. Shuffle it and
            // loop the playlist forever. Only explicitly supplied soundtrack
            // songs are included; the atmosphere's native/default audio is not.
            audio_cmd.arg("--shuffle=yes").arg("--loop-playlist=inf");
            for soundtrack in soundtrack_paths {
                audio_cmd.arg(soundtrack);
            }
        } else {
            audio_cmd
                .arg("--loop-file=inf")
                .arg(&soundtrack_paths[0]);
        }

        match audio_cmd.spawn() {
            Ok(child) => Some(child),
            Err(error) => {
                let mut video = video;
                let _ = video.kill();
                let _ = video.wait();
                return Err(format!("Could not start atmosphere soundtrack in mpv: {error}"));
            }
        }
    } else {
        None
    };

    if let Ok(mut guard) = manager.atmosphere.lock() {
        *guard = Some(AtmosphereSession { video, audio });
    }

    // When ESC/q closes the fullscreen video, also stop the independent
    // soundtrack process so no audio continues in the background.
    let manager_clone = manager.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(250));
        let ended = manager_clone
            .atmosphere
            .lock()
            .ok()
            .and_then(|mut guard| {
                guard
                    .as_mut()
                    .and_then(|session| session.video.try_wait().ok().flatten())
            })
            .is_some();
        if ended {
            if let Ok(mut guard) = manager_clone.atmosphere.lock() {
                if let Some(mut session) = guard.take() {
                    let _ = session.video.wait();
                    if let Some(mut audio) = session.audio.take() {
                        let _ = audio.kill();
                        let _ = audio.wait();
                    }
                }
            }
            return;
        }
    });
    Ok(())
}


pub fn atmosphere_fullscreen_active(manager: &PlayerManager) -> bool {
    let Ok(mut guard) = manager.atmosphere.lock() else {
        return false;
    };
    let Some(session) = guard.as_mut() else {
        return false;
    };

    match session.video.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(_) => false,
    }
}

fn subtitle_track_is_forced(track: &Value) -> bool {
    if track
        .get("forced")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }

    // Some containers do not set mpv's forced boolean but still label the
    // embedded stream, for example "English-FORCED-SRT".
    track
        .get("title")
        .and_then(Value::as_str)
        .map(|title| title.to_ascii_lowercase().contains("forced"))
        .unwrap_or(false)
}

fn parse_tracks(value: &Value) -> (Vec<TrackInfo>, Vec<TrackInfo>) {
    let mut audio = Vec::new();
    let mut subtitles = Vec::new();
    for track in value.as_array().into_iter().flatten() {
        let Some(kind) = track.get("type").and_then(Value::as_str) else {
            continue;
        };
        let Some(id) = track.get("id").and_then(Value::as_i64) else {
            continue;
        };

        if kind == "sub" && subtitle_track_is_forced(track) {
            continue;
        }

        let info = TrackInfo {
            id,
            kind: kind.to_string(),
            title: track
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned),
            lang: track.get("lang").and_then(Value::as_str).map(str::to_owned),
            selected: track
                .get("selected")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            forced: track
                .get("forced")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        match kind {
            "audio" => audio.push(info),
            "sub" => subtitles.push(info),
            _ => {}
        }
    }
    (audio, subtitles)
}

fn get_number(pipe: &str, property: &str) -> Result<f64, String> {
    get_property(pipe, property).map(|v| v.as_f64().unwrap_or(0.0))
}

fn get_bool(pipe: &str, property: &str) -> Result<bool, String> {
    get_property(pipe, property).map(|v| v.as_bool().unwrap_or(false))
}

fn get_property(pipe: &str, property: &str) -> Result<Value, String> {
    request(pipe, json!(["get_property", property]))
}

fn command(pipe: &str, command: Value) -> Result<(), String> {
    request(pipe, command).map(|_| ())
}

#[cfg(windows)]
fn request(pipe: &str, command: Value) -> Result<Value, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(|e| format!("mpv IPC unavailable: {e}"))?;
    let payload = json!({"command": command, "request_id": 1});
    writeln!(file, "{}", payload).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    for _ in 0..20 {
        line.clear();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
        if value.get("request_id").and_then(Value::as_i64) == Some(1) {
            let error = value.get("error").and_then(Value::as_str).unwrap_or("success");
            if error != "success" {
                return Err(format!("mpv error: {error}"));
            }
            return Ok(value.get("data").cloned().unwrap_or(Value::Null));
        }
    }
    Err("mpv did not return a response".into())
}

#[cfg(not(windows))]
fn request(_pipe: &str, _command: Value) -> Result<Value, String> {
    Err("The HED Space mpv player bridge is currently implemented for Windows".into())
}

fn wait_for_pipe(pipe: &str, attempts: usize, delay_ms: u64) -> Result<(), String> {
    for _ in 0..attempts {
        #[cfg(windows)]
        if OpenOptions::new().read(true).write(true).open(pipe).is_ok() {
            return Ok(());
        }
        #[cfg(not(windows))]
        if Path::new(pipe).exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(delay_ms));
    }
    Err("mpv started, but its control pipe did not become available".into())
}
