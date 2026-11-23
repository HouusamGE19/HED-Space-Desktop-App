use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub file_path: String,
    pub series_key: Option<String>,
    pub series_title: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub music_collection_key: Option<String>,
    pub thumbnail_locked: bool,
    pub duration: f64,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub added_at: i64,
    pub modified_at: i64,
    pub progress_position: Option<f64>,
    pub progress_duration: Option<f64>,
    pub last_watched_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonItem {
    pub season_number: i64,
    pub episodes: Vec<MediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesItem {
    pub id: i64,
    pub series_key: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub seasons: Vec<SeasonItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicCollectionItem {
    pub id: i64,
    pub collection_key: String,
    pub title: String,
    pub poster_path: Option<String>,
    pub is_soundtrack: bool,
    pub songs: Vec<MediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryPayload {
    pub movies: Vec<MediaItem>,
    pub childhood: Vec<MediaItem>,
    pub songs: Vec<MediaItem>,
    pub music_collections: Vec<MusicCollectionItem>,
    pub atmospheres: Vec<MediaItem>,
    pub series: Vec<SeriesItem>,
    pub continue_watching: Vec<MediaItem>,
    pub recently_added: Vec<MediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub movies: usize,
    pub episodes: usize,
    pub anime: usize,
    pub childhood: usize,
    pub songs: usize,
    pub atmospheres: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    // Legacy artwork root retained for upgrades from older HED Space builds.
    pub media_root: Option<String>,
    pub library_source_count: usize,
    pub mpv_available: bool,
    pub ffprobe_available: bool,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySource {
    pub id: i64,
    pub path: String,
    pub source_type: String,
    pub is_primary: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: i64,
    pub kind: String,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub selected: bool,
    #[serde(default)]
    pub forced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub media_id: i64,
    pub title: String,
    pub media_kind: String,
    pub series_title: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub buffering: bool,
    pub volume: f64,
    pub audio_tracks: Vec<TrackInfo>,
    pub subtitle_tracks: Vec<TrackInfo>,
    pub next_media_id: Option<i64>,
    pub next_media_title: Option<String>,
    pub next_season_number: Option<i64>,
    pub next_episode_number: Option<i64>,
    pub previous_media_id: Option<i64>,
}
