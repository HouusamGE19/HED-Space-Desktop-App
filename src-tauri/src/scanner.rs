use crate::process::hidden_command;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use regex::Regex;
use serde_json::Value;
use walkdir::WalkDir;

use crate::db;
use crate::models::ScanReport;
use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct ScannedMedia {
    pub kind: String,
    pub title: String,
    pub file_path: String,
    pub series_key: Option<String>,
    pub series_title: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub music_collection_key: Option<String>,
    pub duration: f64,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub added_at: i64,
    pub modified_at: i64,
}

#[derive(Default)]
struct ProbeData {
    duration: f64,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoCollection {
    Standard,
    Movies,
    Series,
    Anime,
    Childhood,
}

impl VideoCollection {
    fn force_series(self) -> bool {
        matches!(self, Self::Series | Self::Anime | Self::Childhood)
    }

    fn force_movies(self) -> bool {
        matches!(self, Self::Movies)
    }

    fn key_prefix(self) -> Option<&'static str> {
        match self {
            Self::Anime => Some("anime"),
            Self::Childhood => Some("childhood"),
            Self::Standard | Self::Movies | Self::Series => None,
        }
    }
}

pub fn scan_sources(
    conn: &Connection,
    artwork_root: &Path,
    sources: &[(PathBuf, String)],
    ffprobe: Option<&Path>,
    ffmpeg: Option<&Path>,
    scan_at: i64,
) -> Result<ScanReport, String> {
    if !artwork_root.is_dir() {
        return Err(format!("Managed artwork directory does not exist: {}", artwork_root.display()));
    }
    for (path, _) in sources {
        if !path.is_dir() {
            return Err(format!("Library source is unavailable: {}", path.display()));
        }
    }

    db::begin_scan(conn, scan_at)?;
    let mut report = ScanReport {
        movies: 0,
        episodes: 0,
        anime: 0,
        childhood: 0,
        songs: 0,
        atmospheres: 0,
        total: 0,
    };

    for (source, source_type) in sources {
        scan_source(
            conn,
            artwork_root,
            source,
            source_type,
            ffprobe,
            ffmpeg,
            scan_at,
            &mut report,
        )?;
    }

    db::finish_scan(conn, scan_at)?;
    report.total = report.movies + report.episodes + report.anime + report.childhood + report.songs + report.atmospheres;
    Ok(report)
}

fn scan_source(
    conn: &Connection,
    artwork_root: &Path,
    source: &Path,
    source_type: &str,
    ffprobe: Option<&Path>,
    ffmpeg: Option<&Path>,
    scan_at: i64,
    report: &mut ScanReport,
) -> Result<(), String> {
    match source_type {
        "standard" => scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Standard, report),
        "movies" => scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Movies, report),
        "series" => scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Series, report),
        "anime" => scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Anime, report),
        "childhood" => scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Childhood, report),
        "songs" => scan_songs(conn, source, ffprobe, scan_at, report),
        "atmospheres" => scan_atmospheres(conn, source, ffprobe, scan_at, report),
        "auto" | _ => scan_auto_source(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, report),
    }
}

fn scan_auto_source(
    conn: &Connection,
    artwork_root: &Path,
    source: &Path,
    ffprobe: Option<&Path>,
    ffmpeg: Option<&Path>,
    scan_at: i64,
    report: &mut ScanReport,
) -> Result<(), String> {
    // Auto mode is a generic library root. Common category folder names are
    // recognized, but the parent folder itself can be named anything.
    let base = source.file_name().and_then(|v| v.to_str()).unwrap_or_default();
    if base.eq_ignore_ascii_case("Movies & Series") || base.eq_ignore_ascii_case("Media") {
        return scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Standard, report);
    }
    if base.eq_ignore_ascii_case("Movies") || base.eq_ignore_ascii_case("Films") {
        return scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Movies, report);
    }
    if base.eq_ignore_ascii_case("Series") || base.eq_ignore_ascii_case("TV Shows") || base.eq_ignore_ascii_case("TV") {
        return scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Series, report);
    }
    if base.eq_ignore_ascii_case("Anime") {
        return scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Anime, report);
    }
    if base.eq_ignore_ascii_case("Childhood") || base.eq_ignore_ascii_case("Cartoons") {
        return scan_video_collection(conn, artwork_root, source, ffprobe, ffmpeg, scan_at, VideoCollection::Childhood, report);
    }
    if base.eq_ignore_ascii_case("Songs") || base.eq_ignore_ascii_case("Music") {
        return scan_songs(conn, source, ffprobe, scan_at, report);
    }
    if base.eq_ignore_ascii_case("Tube") || base.eq_ignore_ascii_case("Atmospheres") {
        return scan_atmospheres(conn, source, ffprobe, scan_at, report);
    }

    let mut matched = false;
    for name in ["Movies & Series", "Media"] {
        if let Some(folder) = child_dir_case_insensitive(source, name) {
            scan_video_collection(conn, artwork_root, &folder, ffprobe, ffmpeg, scan_at, VideoCollection::Standard, report)?;
            matched = true;
        }
    }
    for name in ["Movies", "Films"] {
        if let Some(folder) = child_dir_case_insensitive(source, name) {
            scan_video_collection(conn, artwork_root, &folder, ffprobe, ffmpeg, scan_at, VideoCollection::Movies, report)?;
            matched = true;
        }
    }
    for name in ["Series", "TV Shows", "TV"] {
        if let Some(folder) = child_dir_case_insensitive(source, name) {
            scan_video_collection(conn, artwork_root, &folder, ffprobe, ffmpeg, scan_at, VideoCollection::Series, report)?;
            matched = true;
        }
    }
    if let Some(folder) = child_dir_case_insensitive(source, "Childhood") {
        scan_video_collection(conn, artwork_root, &folder, ffprobe, ffmpeg, scan_at, VideoCollection::Childhood, report)?;
        matched = true;
    }
    if let Some(folder) = child_dir_case_insensitive(source, "Cartoons") {
        scan_video_collection(conn, artwork_root, &folder, ffprobe, ffmpeg, scan_at, VideoCollection::Childhood, report)?;
        matched = true;
    }
    if let Some(folder) = child_dir_case_insensitive(source, "Anime") {
        scan_video_collection(conn, artwork_root, &folder, ffprobe, ffmpeg, scan_at, VideoCollection::Anime, report)?;
        matched = true;
    }
    for name in ["Songs", "Music"] {
        if let Some(folder) = child_dir_case_insensitive(source, name) {
            scan_songs(conn, &folder, ffprobe, scan_at, report)?;
            matched = true;
        }
    }
    for name in ["Tube", "Atmospheres"] {
        if let Some(folder) = child_dir_case_insensitive(source, name) {
            scan_atmospheres(conn, &folder, ffprobe, scan_at, report)?;
            matched = true;
        }
    }
    if !matched {
        return Err(format!(
            "Auto-detect found no category folders in {}. Assign this folder a type such as Movies, Series, Anime, Childhood, Songs, or Atmospheres.",
            source.display()
        ));
    }
    Ok(())
}

fn child_dir_case_insensitive(root: &Path, wanted: &str) -> Option<PathBuf> {
    fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| {
            entry.path().is_dir()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(wanted)
        })
        .map(|entry| entry.path())
}

fn should_scan_entry(entry: &walkdir::DirEntry) -> bool {
    // Ignore helper/extras folders and their entire subtree, case-insensitively.
    // "Openings" commonly contains anime/series OP videos; "NC" is also used for
    // non-credit opening/ending extras in some collections.
    if entry.file_type().is_dir() {
        let name = entry.file_name().to_string_lossy();
        return !name.eq_ignore_ascii_case("Openings")
            && !name.eq_ignore_ascii_case("NC");
    }
    true
}

fn scan_video_collection(
    conn: &Connection,
    media_root: &Path,
    folder: &Path,
    ffprobe: Option<&Path>,
    ffmpeg: Option<&Path>,
    scan_at: i64,
    collection: VideoCollection,
    report: &mut ScanReport,
) -> Result<(), String> {
    let episode_re = Regex::new(r"(?i)(?:\bS(\d{1,2})[ ._-]*E(\d{1,4})\b|\b(\d{1,2})x(\d{1,4})\b)")
        .map_err(|e| e.to_string())?;
    let season_dir_re = Regex::new(r"(?i)^(?:season[ ._-]*|s)(\d{1,2})$").map_err(|e| e.to_string())?;

    // Sorting gives stable fallback episode numbers for files that have no numeric marker.
    let mut video_paths: Vec<PathBuf> = WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_scan_entry)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && is_video(entry.path()))
        .map(|entry| entry.path().to_path_buf())
        .collect();
    video_paths.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());

    // Childhood/anime collections are always series. If a user keeps episodes directly
    // in the collection root (instead of Show/Season folders), infer a stable show
    // grouping from repeated meaningful words in the filenames. This lets names such as
    // "... | Gumball | Cartoon Network" collapse into one Gumball series instead of
    // creating one pseudo-series per episode. Nested folders still take priority.
    let flat_series_hints = if collection.force_series() {
        build_flat_series_hints(folder, &video_paths)
    } else {
        HashMap::new()
    };

    let mut fallback_episode_counters: HashMap<(String, i64), i64> = HashMap::new();

    for path in video_paths {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled");
        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
        let modified_at = system_time_secs(metadata.modified().ok()).unwrap_or(scan_at);
        let probe = probe(ffprobe, &path);
        let explicit_episode = episode_re.captures(stem).map(|caps| {
            let (season, episode) = if let (Some(s), Some(e)) = (caps.get(1), caps.get(2)) {
                (s.as_str().parse::<i64>().unwrap_or(1), e.as_str().parse::<i64>().unwrap_or(1))
            } else {
                (
                    caps.get(3).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(1),
                    caps.get(4).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(1),
                )
            };
            let marker_start = caps.get(0).map(|m| m.start()).unwrap_or(0);
            (season, episode, marker_start)
        });

        let mut item = if (!collection.force_movies() && explicit_episode.is_some()) || collection.force_series() {
            let (base_key, series_title) = if let Some((_, _, marker_start)) = explicit_episode {
                infer_series(folder, &path, stem, marker_start, &season_dir_re)
            } else if let Some((hint_key, hint_title)) = flat_series_hints.get(&path) {
                (hint_key.clone(), hint_title.clone())
            } else {
                infer_grouped_series(folder, &path, stem, &season_dir_re)
            };
            let series_key = if let Some(prefix) = collection.key_prefix() {
                format!("{prefix}::{base_key}")
            } else {
                base_key
            };

            let season = explicit_episode
                .map(|value| value.0)
                .or_else(|| infer_season_from_path(folder, &path, &season_dir_re))
                .unwrap_or(1);
            let episode = explicit_episode
                .map(|value| value.1)
                .or_else(|| infer_loose_episode_number(stem))
                .unwrap_or_else(|| {
                    let key = (series_key.clone(), season);
                    let counter = fallback_episode_counters.entry(key).or_insert(0);
                    *counter += 1;
                    *counter
                });

            // Keep fallback counters ahead of any explicitly-numbered files in the same season.
            fallback_episode_counters
                .entry((series_key.clone(), season))
                .and_modify(|value| *value = (*value).max(episode))
                .or_insert(episode);

            let episode_title = if explicit_episode.is_some() {
                clean_episode_title(
                    probe.title.as_deref().unwrap_or(stem),
                    stem,
                    &series_title,
                    episode,
                )
            } else {
                clean_grouped_episode_title(
                    probe.title.as_deref().unwrap_or(stem),
                    stem,
                    &series_title,
                    episode,
                )
            };
            let year = extract_year(probe.title.as_deref().unwrap_or(stem)).or_else(|| extract_year(stem));
            ScannedMedia {
                kind: "episode".into(),
                title: episode_title,
                file_path: path.to_string_lossy().to_string(),
                series_key: Some(series_key.clone()),
                series_title: Some(series_title.clone()),
                season_number: Some(season),
                episode_number: Some(episode),
                artist: None,
                album: None,
                music_collection_key: None,
                duration: probe.duration,
                year,
                poster_path: None,
                backdrop_path: None,
                thumbnail_path: find_art(path.parent(), &["thumbnail", "thumb", stem])
                    .or_else(|| generate_episode_thumbnail(media_root, &path, probe.duration, &series_key, season, episode, ffmpeg)),
                added_at: scan_at,
                modified_at,
            }
        } else {
            let source_title = probe.title.as_deref().unwrap_or(stem);
            let title = clean_video_title(source_title, stem);
            let year = extract_year(source_title).or_else(|| extract_year(stem));
            ScannedMedia {
                kind: "movie".into(),
                title,
                file_path: path.to_string_lossy().to_string(),
                series_key: None,
                series_title: None,
                season_number: None,
                episode_number: None,
                artist: None,
                album: None,
                music_collection_key: None,
                duration: probe.duration,
                year,
                poster_path: find_art(path.parent(), &["poster", "folder", "cover", "front", stem]),
                backdrop_path: find_art(path.parent(), &["backdrop", "fanart", "background"])
                    .or_else(|| generate_movie_backdrop(media_root, &path, probe.duration, ffmpeg)),
                thumbnail_path: find_art(path.parent(), &["thumbnail", "thumb"]),
                added_at: scan_at,
                modified_at,
            }
        };

        if item.poster_path.is_none() && item.kind == "movie" {
            item.poster_path = find_art(path.parent(), &["poster", "folder", "cover", "front"]);
        }

        db::upsert_media(conn, &item, scan_at)?;
        if let (Some(key), Some(title)) = (&item.series_key, &item.series_title) {
            db::ensure_series(conn, key, title)?;
            if db::get_series_backdrop_by_key(conn, key)?.as_deref().unwrap_or("").trim().is_empty() {
                if let Some(backdrop) = generate_series_backdrop(
                    media_root,
                    &path,
                    probe.duration,
                    key,
                    ffmpeg,
                ) {
                    db::set_series_backdrop_if_empty(conn, key, &backdrop)?;
                }
            }
            match collection {
                VideoCollection::Anime => report.anime += 1,
                VideoCollection::Childhood => report.childhood += 1,
                VideoCollection::Standard | VideoCollection::Series => report.episodes += 1,
                VideoCollection::Movies => report.movies += 1,
            }
        } else {
            report.movies += 1;
        }
    }
    Ok(())
}

fn scan_songs(
    conn: &Connection,
    folder: &Path,
    ffprobe: Option<&Path>,
    scan_at: i64,
    report: &mut ScanReport,
) -> Result<(), String> {
    for entry in WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_scan_entry)
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !entry.file_type().is_file() || !is_audio(path) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled");
        let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
        let modified_at = system_time_secs(metadata.modified().ok()).unwrap_or(scan_at);
        let probe = probe(ffprobe, path);
        let (collection_key, collection_title, collection_dir) =
            infer_music_collection(folder, path);
        let detected_collection_poster =
            find_art(Some(collection_dir.as_path()), &["cover", "folder", "front", "album", "poster"]);

        // The physical top-level folder is the HED Space music collection.
        // Embedded album metadata can still be shown, but it does not decide grouping.
        let item = ScannedMedia {
            kind: "song".into(),
            title: probe.title.unwrap_or_else(|| clean_song_title(stem)),
            file_path: path.to_string_lossy().to_string(),
            series_key: None,
            series_title: None,
            season_number: None,
            episode_number: None,
            artist: probe.artist,
            album: probe.album.or_else(|| Some(collection_title.clone())),
            music_collection_key: Some(collection_key.clone()),
            duration: probe.duration,
            year: None,
            poster_path: None,
            backdrop_path: None,
            thumbnail_path: find_art(path.parent(), &["cover", "folder", "front", "album"]),
            added_at: scan_at,
            modified_at,
        };
        db::upsert_media(conn, &item, scan_at)?;
        db::ensure_music_collection(
            conn,
            &collection_key,
            &collection_title,
            detected_collection_poster.as_deref(),
        )?;
        report.songs += 1;
    }
    Ok(())
}


fn infer_music_collection(root: &Path, song_path: &Path) -> (String, String, PathBuf) {
    let collection_dir = song_path
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|first| root.join(first.as_os_str()))
        .filter(|candidate| candidate.is_dir())
        .unwrap_or_else(|| root.to_path_buf());

    let title = collection_dir
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Music")
        .trim()
        .to_string();

    let key = format!(
        "music::{}",
        normalize_key(&collection_dir.to_string_lossy())
    );
    (key, title, collection_dir)
}

fn scan_atmospheres(
    conn: &Connection,
    folder: &Path,
    ffprobe: Option<&Path>,
    scan_at: i64,
    report: &mut ScanReport,
) -> Result<(), String> {
    for entry in WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_scan_entry)
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !entry.file_type().is_file() || !is_video(path) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Atmosphere");
        let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
        let modified_at = system_time_secs(metadata.modified().ok()).unwrap_or(scan_at);
        let probe = probe(ffprobe, path);
        let source_title = probe.title.as_deref().unwrap_or(stem);
        let item = ScannedMedia {
            kind: "atmosphere".into(),
            title: clean_video_title(source_title, stem),
            file_path: path.to_string_lossy().to_string(),
            series_key: None,
            series_title: None,
            season_number: None,
            episode_number: None,
            artist: None,
            album: None,
            music_collection_key: None,
            duration: probe.duration,
            year: extract_year(source_title).or_else(|| extract_year(stem)),
            poster_path: None,
            backdrop_path: None,
            thumbnail_path: find_art(path.parent(), &["thumbnail", "thumb", "poster", stem]),
            added_at: scan_at,
            modified_at,
        };
        db::upsert_media(conn, &item, scan_at)?;
        report.atmospheres += 1;
    }
    Ok(())
}


fn build_flat_series_hints(
    collection_root: &Path,
    video_paths: &[PathBuf],
) -> HashMap<PathBuf, (String, String)> {
    let flat_files: Vec<&PathBuf> = video_paths
        .iter()
        .filter(|path| path.parent() == Some(collection_root))
        .collect();
    if flat_files.len() < 2 {
        return HashMap::new();
    }

    let mut tokens_by_path: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut token_frequency: HashMap<String, usize> = HashMap::new();

    for path in &flat_files {
        let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("Untitled");
        let tokens = grouping_tokens(stem);
        let unique: HashSet<String> = tokens.iter().cloned().collect();
        for token in unique {
            *token_frequency.entry(token).or_insert(0) += 1;
        }
        tokens_by_path.insert((*path).clone(), tokens);
    }

    // Pick one repeated, meaningful anchor per file. Frequency is the strongest signal:
    // show names recur across many episodes, while episode-specific words normally do not.
    let mut anchors: HashMap<PathBuf, String> = HashMap::new();
    for path in &flat_files {
        let Some(tokens) = tokens_by_path.get(*path) else { continue; };
        let mut best: Option<(usize, usize, String)> = None;
        for token in tokens {
            let frequency = *token_frequency.get(token).unwrap_or(&0);
            if frequency < 2 {
                continue;
            }
            let char_len = token.chars().count();
            let candidate = (frequency, char_len, token.clone());
            let replace = match &best {
                None => true,
                Some((best_freq, best_len, best_token)) => {
                    frequency > *best_freq
                        || (frequency == *best_freq && char_len > *best_len)
                        || (frequency == *best_freq && char_len == *best_len && token < best_token)
                }
            };
            if replace {
                best = Some(candidate);
            }
        }
        if let Some((_, _, anchor)) = best {
            anchors.insert((*path).clone(), anchor);
        }
    }

    // Build a display title for each anchor. Prefer the longest repeated filename segment
    // containing the anchor (e.g. "The Amazing World of Gumball" / "عالم غامبول المدهش")
    // rather than showing a single keyword when the fuller title is available.
    let mut files_by_anchor: HashMap<String, Vec<&PathBuf>> = HashMap::new();
    for path in &flat_files {
        if let Some(anchor) = anchors.get(*path) {
            files_by_anchor.entry(anchor.clone()).or_default().push(*path);
        }
    }

    let mut titles_by_anchor: HashMap<String, String> = HashMap::new();
    for (anchor, paths) in &files_by_anchor {
        let mut segment_counts: HashMap<String, (usize, String)> = HashMap::new();
        for path in paths {
            let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("Untitled");
            for segment in grouping_segments(stem) {
                let segment_tokens = grouping_tokens(&segment);
                if !segment_tokens.iter().any(|token| token == anchor) {
                    continue;
                }
                let normalized = normalize_grouping_text(&segment);
                if normalized.is_empty() {
                    continue;
                }
                let entry = segment_counts.entry(normalized).or_insert((0, segment.clone()));
                entry.0 += 1;
                if segment.chars().count() > entry.1.chars().count() {
                    entry.1 = segment;
                }
            }
        }

        let mut best_segment: Option<(usize, usize, String)> = None;
        for (_normalized, (count, segment)) in segment_counts {
            if count < 2 {
                continue;
            }
            let length = segment.chars().count();
            let candidate = (length, count, segment);
            let replace = match &best_segment {
                None => true,
                Some((best_len, best_count, _)) => {
                    length > *best_len || (length == *best_len && count > *best_count)
                }
            };
            if replace {
                best_segment = Some(candidate);
            }
        }

        let title = best_segment
            .map(|(_, _, value)| clean_title(&value))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| display_group_anchor(anchor));
        titles_by_anchor.insert(anchor.clone(), title);
    }

    let root_key = normalize_key(&collection_root.to_string_lossy());
    let mut result = HashMap::new();
    for path in flat_files {
        let Some(anchor) = anchors.get(path) else { continue; };
        let title = titles_by_anchor
            .get(anchor)
            .cloned()
            .unwrap_or_else(|| display_group_anchor(anchor));
        let key = format!("flat::{root_key}::{anchor}");
        result.insert(path.clone(), (key, title));
    }
    result
}

fn grouping_tokens(input: &str) -> Vec<String> {
    let cleaned = clean_video_title(input, input);
    let token_re = Regex::new(r"[\p{L}\p{N}]+").unwrap();
    token_re
        .find_iter(&cleaned)
        .map(|found| found.as_str().to_lowercase())
        .filter(|token| {
            let len = token.chars().count();
            len >= 2
                && !token.chars().all(|ch| ch.is_ascii_digit())
                && !is_grouping_stopword(token)
        })
        .collect()
}

fn grouping_segments(input: &str) -> Vec<String> {
    let split_re = Regex::new(r"\s*(?:\||｜|•|·)\s*|\s+[\-–—:]\s+").unwrap();
    split_re
        .split(input)
        .map(|value| clean_video_title(value, value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_grouping_text(input: &str) -> String {
    grouping_tokens(input).join(" ")
}

fn display_group_anchor(anchor: &str) -> String {
    let mut chars = anchor.chars();
    let Some(first) = chars.next() else { return "Unknown Series".to_string(); };
    if first.is_ascii_alphabetic() {
        let mut value = first.to_ascii_uppercase().to_string();
        value.push_str(chars.as_str());
        value
    } else {
        anchor.to_string()
    }
}

fn is_grouping_stopword(token: &str) -> bool {
    matches!(
        token,
        // English / release / broadcaster words
        "the" | "and" | "of" | "a" | "an" | "world" | "show" | "episode" | "episodes"
            | "ep" | "season" | "part" | "cartoon" | "network" | "official" | "clip"
            | "full" | "video" | "movie" | "arabic" | "english" | "eng" | "dub" | "dubbed"
            | "sub" | "subbed" | "hd" | "uhd" | "web" | "webrip" | "bluray"
        // Common Arabic episode / broadcaster words. These are intentionally ignored so
        // a network name does not accidentally merge unrelated childhood shows.
            | "حلقة" | "الحلقة" | "حلقات" | "مجموعة" | "مجموعه" | "رسوم" | "متحركة"
            | "متحركه" | "كرتون" | "نتورك" | "الموسم" | "موسم" | "الجزء" | "جزء"
            | "عربي" | "العربية" | "مدبلج" | "مدبلجة" | "مدبلجه" | "كامل" | "كاملة"
            | "الجديد" | "جديد" | "فيديو" | "عالم"
    )
}

fn infer_series(
    collection_root: &Path,
    path: &Path,
    stem: &str,
    episode_match_start: usize,
    season_dir_re: &Regex,
) -> (String, String) {
    let relative = path.strip_prefix(collection_root).unwrap_or(path);
    let components: Vec<String> = relative
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(str::to_owned))
        .collect();

    for (index, component) in components.iter().enumerate() {
        if season_dir_re.is_match(component) && index > 0 {
            let raw = &components[index - 1];
            let title = clean_series_container_title(raw);
            let key_path = collection_root.join(&title);
            return (normalize_key(&key_path.to_string_lossy()), title);
        }
    }

    if components.len() >= 2 {
        let parent = &components[components.len() - 2];
        if !season_dir_re.is_match(parent) {
            let parent_title = clean_series_container_title(parent);
            if !parent_title.is_empty() && !parent_title.eq_ignore_ascii_case("movies & series") {
                // A folder named "One Piece S2" or "One Piece Season 2" is a season
                // container, not the show title. Build the key from the cleaned show name
                // so S1/S2 sibling folders merge into one series.
                let key_path = collection_root.join(&parent_title);
                return (normalize_key(&key_path.to_string_lossy()), parent_title);
            }
        }
    }

    let prefix = stem.get(..episode_match_start).unwrap_or(stem);
    let title = clean_title(prefix);
    let title = if title.is_empty() { "Unknown Series".to_string() } else { title };
    (normalize_key(&title), title)
}

fn infer_grouped_series(
    collection_root: &Path,
    path: &Path,
    stem: &str,
    season_dir_re: &Regex,
) -> (String, String) {
    let relative = path.strip_prefix(collection_root).unwrap_or(path);
    let components: Vec<String> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();

    for (index, component) in components.iter().enumerate() {
        if season_dir_re.is_match(component) && index > 0 {
            let raw = &components[index - 1];
            let title = clean_series_container_title(raw);
            let key_path = collection_root.join(&title);
            return (normalize_key(&key_path.to_string_lossy()), title);
        }
    }

    if let Some(parent) = path.parent() {
        if parent != collection_root {
            if let Some(parent_name) = parent.file_name().and_then(|name| name.to_str()) {
                let title = clean_series_container_title(parent_name);
                if !title.is_empty() {
                    // Normalize season-suffixed folders ("Show S2", "Show Season 2")
                    // to a shared show key, so the UI presents one series with seasons.
                    let key_path = collection_root.join(&title);
                    return (normalize_key(&key_path.to_string_lossy()), title);
                }
            }
        }
    }

    let title = derive_series_title_from_filename(stem);
    (normalize_key(&title), title)
}

fn season_suffix_number(value: &str) -> Option<i64> {
    // Accept common season-container names, including release-style wrappers:
    // "Show S2", "Show Season 02", "Show (Season 01)", "Show [Season 4 Part 2]".
    // Parts still belong to the same numbered season in the HED Space UI.
    let re = Regex::new(
        r"(?ix)
        (?:^|[\s._\-\(\[\{])
        (?:season[\s._\-]*|s)
        (\d{1,2})
        (?:[\s._\-]*(?:part|pt)[\s._\-]*\d{1,2})?
        [\s\)\]\}._\-]*$
        ",
    )
    .ok()?;
    re.captures(value)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

fn clean_series_container_title(value: &str) -> String {
    // Anime folders are often named like:
    //   [Anime Time] Attack on titan (Season 01)
    //   [Anime Time] Attack On Titan [Season 4 Part 2]
    // Strip source/release tags and the season wrapper so all sibling season folders
    // resolve to one series key: "Attack on Titan".
    let leading_tags = Regex::new(r"^\s*(?:\[[^\]]+\]\s*)+").unwrap();
    let without_leading_tags = leading_tags.replace(value.trim(), "");

    let season_suffix = Regex::new(
        r"(?ix)
        \s*
        [\(\[\{]?\s*
        (?:season[\s._\-]*|s)\d{1,2}
        (?:[\s._\-]*(?:part|pt)[\s._\-]*\d{1,2})?
        \s*[\)\]\}]?
        \s*$
        ",
    )
    .unwrap();
    let without_season = season_suffix.replace(&without_leading_tags, "");
    let title = clean_title(without_season.trim());
    if title.is_empty() {
        clean_title(without_leading_tags.trim())
    } else {
        title
    }
}

fn infer_season_from_path(
    collection_root: &Path,
    path: &Path,
    season_dir_re: &Regex,
) -> Option<i64> {
    let relative = path.strip_prefix(collection_root).ok()?;
    for component in relative.components() {
        let value = component.as_os_str().to_str()?;
        if let Some(caps) = season_dir_re.captures(value) {
            if let Some(number) = caps.get(1).and_then(|m| m.as_str().parse::<i64>().ok()) {
                return Some(number);
            }
        }
        if let Some(number) = season_suffix_number(value) {
            return Some(number);
        }
    }
    None
}

fn infer_loose_episode_number(stem: &str) -> Option<i64> {
    let named = Regex::new(r"(?i)\b(?:episode|ep|e)[ ._-]*0*(\d{1,4})\b").unwrap();
    if let Some(number) = named
        .captures(stem)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
    {
        return Some(number);
    }

    // Handles names such as "Frieren - 01", "001 - Pilot", and "Show [12]".
    let token = Regex::new(r"(?:^|[ ._\-\[\(])0*(\d{1,4})(?:v\d+)?(?:$|[ ._\-\]\)])").unwrap();
    for caps in token.captures_iter(stem) {
        let Some(number) = caps.get(1).and_then(|m| m.as_str().parse::<i64>().ok()) else { continue; };
        if !(1900..=2099).contains(&number) {
            return Some(number);
        }
    }
    None
}

fn derive_series_title_from_filename(stem: &str) -> String {
    let leading_group = Regex::new(r"^\s*(?:\[[^\]]+\]\s*)+").unwrap();
    let stripped = leading_group.replace(stem, "");
    let markers = [
        Regex::new(r"(?i)\bS\d{1,2}[ ._-]*E\d{1,4}\b").unwrap(),
        Regex::new(r"(?i)\b\d{1,2}x\d{1,4}\b").unwrap(),
        Regex::new(r"(?i)\b(?:episode|ep|e)[ ._-]*0*\d{1,4}\b").unwrap(),
        Regex::new(r"(?:^|[ ._\-\[\(])0*\d{1,4}(?:v\d+)?(?:$|[ ._\-\]\)])").unwrap(),
    ];
    let mut cut = stripped.len();
    for marker in markers {
        if let Some(found) = marker.find(&stripped) {
            cut = cut.min(found.start());
        }
    }
    let candidate = stripped.get(..cut).unwrap_or(&stripped);
    let title = clean_title(candidate);
    if title.is_empty() { "Unknown Series".to_string() } else { title }
}

fn clean_grouped_episode_title(source: &str, stem: &str, series_title: &str, episode: i64) -> String {
    let marker = Regex::new(
        r"(?i)(?:\bS\d{1,2}[ ._-]*E\d{1,4}\b|\b\d{1,2}x\d{1,4}\b|\b(?:episode|ep|e)[ ._-]*0*\d{1,4}\b|(?:^|[ ._\-\[\(])0*\d{1,4}(?:v\d+)?(?:$|[ ._\-\]\)]))",
    ).unwrap();

    for candidate in [source, stem] {
        if let Some(found) = marker.find(candidate) {
            let suffix = candidate.get(found.end()..).unwrap_or("");
            let suffix = clean_video_title(suffix, suffix);
            if episode_title_is_useful(&suffix, series_title) {
                return suffix;
            }
        }
    }

    let leading_group = Regex::new(r"^\s*(?:\[[^\]]+\]\s*)+").unwrap();
    let cleaned = clean_video_title(source, stem);
    let cleaned = leading_group.replace(&cleaned, "").to_string();
    let without_series = cleaned
        .strip_prefix(series_title)
        .map(|value| value.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | ':' | '.')))
        .unwrap_or(cleaned.as_str());
    let without_marker = marker.replace(without_series, " ");
    let candidate = clean_video_title(&without_marker, &without_marker);
    if episode_title_is_useful(&candidate, series_title) {
        candidate
    } else {
        format!("Episode {episode}")
    }
}

fn episode_title_is_useful(value: &str, series_title: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(series_title) {
        return false;
    }
    let technical_only = Regex::new(r"(?i)^[\s\(\[\{]*(?:2160p|1080p|720p|480p|4k|uhd|x26[45]|h\.?26[45]|hevc|av1|10[ ._-]*bit|8[ ._-]*bit|web[ ._-]*dl|webrip|bluray|aac|ac3|eac3|dts|multi)[\s\)\]\}]*$").unwrap();
    !technical_only.is_match(trimmed)
}

fn probe(ffprobe: Option<&Path>, path: &Path) -> ProbeData {
    let Some(ffprobe) = ffprobe else {
        return ProbeData::default();
    };
    let output = hidden_command(ffprobe)
        .args([
            "-v",
            "quiet",
            "-show_entries",
            "format=duration:format_tags=title,artist,album",
            "-of",
            "json",
        ])
        .arg(path)
        .output();
    let Ok(output) = output else {
        return ProbeData::default();
    };
    if !output.status.success() {
        return ProbeData::default();
    }
    let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) else {
        return ProbeData::default();
    };
    let format = value.get("format").unwrap_or(&Value::Null);
    let duration = format
        .get("duration")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let tags = format.get("tags").unwrap_or(&Value::Null);
    ProbeData {
        duration,
        title: tag(tags, "title"),
        artist: tag(tags, "artist"),
        album: tag(tags, "album"),
    }
}

fn tag(tags: &Value, key: &str) -> Option<String> {
    tags.as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, value)| value.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn find_art(dir: Option<&Path>, names: &[&str]) -> Option<String> {
    let dir = dir?;
    let entries = fs::read_dir(dir).ok()?;
    let names: Vec<String> = names.iter().map(|s| s.to_ascii_lowercase()).collect();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() || !is_image(&path) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if names.iter().any(|wanted| stem == *wanted || stem.starts_with(wanted)) {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_video(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "mkv" | "mp4" | "m4v" | "avi" | "mov" | "webm" | "wmv" | "ts" | "m2ts" | "mpg" | "mpeg"
    )
}

fn is_audio(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "opus" | "wma" | "alac"
    )
}

fn is_image(path: &Path) -> bool {
    matches!(extension(path).as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp")
}

fn clean_song_title(stem: &str) -> String {
    let re = Regex::new(r"^\s*\d{1,3}\s*[-_. ]+\s*").unwrap();
    clean_basic_title(&re.replace(stem, ""))
}

fn clean_episode_title(source: &str, stem: &str, series_title: &str, episode: i64) -> String {
    let marker = Regex::new(r"(?i)(?:\bS\d{1,2}[ ._-]*E\d{1,3}\b|\b\d{1,2}x\d{1,3}\b)").unwrap();

    // Prefer text after SxxExx / 1x01. This avoids displaying the series name as the episode title.
    for candidate in [source, stem] {
        if let Some(m) = marker.find(candidate) {
            let suffix = candidate.get(m.end()..).unwrap_or("");
            let suffix = clean_video_title(suffix, suffix);
            if !suffix.is_empty() && !suffix.eq_ignore_ascii_case(series_title) {
                return suffix;
            }
        }
    }

    let cleaned = clean_video_title(source, stem);
    let without_series = cleaned
        .strip_prefix(series_title)
        .map(|v| v.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | ':' | '.')))
        .unwrap_or(cleaned.as_str());
    if !without_series.is_empty() && !without_series.eq_ignore_ascii_case(series_title) {
        without_series.to_string()
    } else {
        format!("Episode {episode}")
    }
}

fn clean_video_title(source: &str, fallback: &str) -> String {
    let cleaned = clean_release_title(source);
    if cleaned.is_empty() {
        clean_release_title(fallback)
    } else {
        cleaned
    }
}

fn clean_release_title(input: &str) -> String {
    let mut title = input.replace(['.', '_'], " ");

    // Release groups are very commonly appended in square brackets.
    let trailing_brackets = Regex::new(r"(?i)(?:\s*\[[^\]]+\])+\s*$").unwrap();
    title = trailing_brackets.replace(&title, "").to_string();

    // Pull the release year out of the display title. It is stored separately in SQLite.
    let bracket_year = Regex::new(r"(?i)[\(\[\{]\s*(?:19\d{2}|20\d{2})\s*[\)\]\}]").unwrap();
    title = bracket_year.replace_all(&title, " ").to_string();

    // Once a technical release token begins, everything following it is almost always encoding metadata.
    let release_tail = Regex::new(
        r"(?ix)(?:\s*[-–—]?\s*)(?:2160p|1080p|720p|480p|4k|uhd|bluray|blu-ray|bdrip|brrip|webrip|web-rip|web[ ._-]*dl|hdtv|remux|hdr10\+?|hdr|dv|dolby[ ._-]*vision|x26[45]|h\.?26[45]|hevc|av1|10[ ._-]*bit|8[ ._-]*bit|proper|repack|extended|multi|dual[ ._-]*audio|aac|eac3|ac3|dts(?:-hd)?|truehd|atmos)\b.*$",
    ).unwrap();
    title = release_tail.replace(&title, "").to_string();

    // Remove a standalone year token when it follows a normal title.
    let year_token = Regex::new(r"(?i)(?:\s*[-–—]?\s*)\b(?:19\d{2}|20\d{2})\b\s*$").unwrap();
    title = year_token.replace(&title, "").to_string();

    clean_basic_title(&title)
}

fn clean_basic_title(input: &str) -> String {
    let mut title = input.replace(['.', '_'], " ");
    let episode = Regex::new(r"(?i)\bS\d{1,2}[ ._-]*E\d{1,3}\b|\b\d{1,2}x\d{1,3}\b").unwrap();
    title = episode.replace(&title, " ").to_string();
    let edge_separators = Regex::new(r"^\s*[-–—:]+\s*|\s*[-–—:]+\s*$").unwrap();
    title = edge_separators.replace_all(&title, "").to_string();
    let spaces = Regex::new(r"\s{2,}").unwrap();
    spaces.replace_all(title.trim(), " ").to_string()
}

pub fn clean_title(input: &str) -> String {
    clean_release_title(input)
}

fn extract_year(input: &str) -> Option<i64> {
    let bracketed = Regex::new(r"[\(\[\{]\s*(19\d{2}|20\d{2})\s*[\)\]\}]").unwrap();
    if let Some(caps) = bracketed.captures_iter(input).last() {
        return caps.get(1)?.as_str().parse().ok();
    }
    let token = Regex::new(r"(?:^|[ ._\-])(19\d{2}|20\d{2})(?:$|[ ._\-])").unwrap();
    token.captures_iter(input)
        .last()
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn normalize_key(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('\\', "/")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}


fn generate_movie_backdrop(
    media_root: &Path,
    video_path: &Path,
    duration: f64,
    ffmpeg: Option<&Path>,
) -> Option<String> {
    let ffmpeg = ffmpeg?;
    let output_dir = media_root.join("Posters").join("Backdrops").join("Movies");
    if fs::create_dir_all(&output_dir).is_err() {
        return None;
    }

    let hash = stable_path_hash(video_path);
    let output = output_dir.join(format!("movie-{hash:016x}.jpg"));
    if output.is_file() {
        return Some(output.to_string_lossy().to_string());
    }

    extract_backdrop_frame(ffmpeg, video_path, duration, &output)?;
    Some(output.to_string_lossy().to_string())
}

fn generate_series_backdrop(
    media_root: &Path,
    video_path: &Path,
    duration: f64,
    series_key: &str,
    ffmpeg: Option<&Path>,
) -> Option<String> {
    let ffmpeg = ffmpeg?;
    let output_dir = media_root.join("Posters").join("Backdrops").join("Series");
    if fs::create_dir_all(&output_dir).is_err() {
        return None;
    }

    let mut safe_key = slugify(series_key);
    if safe_key.is_empty() {
        safe_key = format!("series-{:016x}", stable_text_hash(series_key));
    }
    let output = output_dir.join(format!("{safe_key}.jpg"));
    if output.is_file() {
        return Some(output.to_string_lossy().to_string());
    }

    extract_backdrop_frame(ffmpeg, video_path, duration, &output)?;
    Some(output.to_string_lossy().to_string())
}

fn extract_backdrop_frame(
    ffmpeg: &Path,
    video_path: &Path,
    duration: f64,
    output: &Path,
) -> Option<()> {
    let pick = stable_backdrop_capture_time(video_path, duration);
    let status = hidden_command(ffmpeg)
        .arg("-hide_banner")
        .arg("-loglevel").arg("error")
        .arg("-y")
        .arg("-ss").arg(format!("{pick:.3}"))
        .arg("-i").arg(video_path)
        .arg("-map").arg("0:v:0")
        .arg("-frames:v").arg("1")
        .arg("-q:v").arg("1")
        .arg("-an")
        .arg(output)
        .status()
        .ok()?;

    if status.success() && output.is_file() {
        Some(())
    } else {
        let _ = fs::remove_file(output);
        None
    }
}

fn stable_backdrop_capture_time(video_path: &Path, duration: f64) -> f64 {
    if duration <= 0.0 {
        return 60.0;
    }
    let bucket = (stable_path_hash(video_path) % 10_000) as f64 / 10_000.0;
    let start = (duration * 0.25).max(30.0).min((duration - 2.0).max(0.0));
    let end = (duration * 0.55).max(start + 1.0).min((duration - 1.0).max(start + 1.0));
    (start + bucket * (end - start)).min((duration - 0.5).max(0.0))
}

fn stable_path_hash(path: &Path) -> u64 {
    stable_text_hash(&path.to_string_lossy())
}

fn stable_text_hash(text: &str) -> u64 {
    // FNV-1a: compact, deterministic across app versions, and sufficient for filenames.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn generate_episode_thumbnail(
    media_root: &Path,
    video_path: &Path,
    duration: f64,
    series_key: &str,
    season: i64,
    episode: i64,
    ffmpeg: Option<&Path>,
) -> Option<String> {
    let ffmpeg = ffmpeg?;
    let posters_dir = media_root.join("Posters").join("Episodes");
    if fs::create_dir_all(&posters_dir).is_err() {
        return None;
    }

    let safe_key = slugify(series_key);
    let output = posters_dir.join(format!("auto-{}-s{:02}e{:02}.jpg", safe_key, season.max(0), episode.max(0)));
    if output.is_file() {
        return Some(output.to_string_lossy().to_string());
    }

    let pick = stable_episode_capture_time(video_path, duration);
    let mut cmd = hidden_command(ffmpeg);
    let status = cmd
        .arg("-hide_banner")
        .arg("-loglevel").arg("error")
        .arg("-y")
        .arg("-ss").arg(format!("{:.3}", pick))
        .arg("-i").arg(video_path)
        .arg("-frames:v").arg("1")
        .arg("-q:v").arg("2")
        .arg(&output)
        .status()
        .ok()?;

    if status.success() && output.is_file() {
        Some(output.to_string_lossy().to_string())
    } else {
        None
    }
}

fn stable_episode_capture_time(video_path: &Path, duration: f64) -> f64 {
    if duration <= 0.0 {
        return 30.0;
    }
    let mut hasher = DefaultHasher::new();
    video_path.to_string_lossy().hash(&mut hasher);
    let bucket = (hasher.finish() % 1000) as f64 / 1000.0;
    let start = (duration * 0.20).max(15.0);
    let end = (duration * 0.60).max(start + 1.0);
    start + bucket * (end - start)
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}
fn system_time_secs(value: Option<std::time::SystemTime>) -> Option<i64> {
    value?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}
