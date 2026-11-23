# HED Space Desktop v0.6.29

**HED Space** is a private, local-first desktop media platform designed to turn a personal media collection into a polished cinematic experience. It brings movies, series, anime, childhood favourites, music, and immersive ambient videos together in one unified interface — without forcing the user into a traditional file-browser workflow.

The platform scans media directly from user-selected folders across multiple drives, groups episodic content into series and seasons, remembers playback progress, supports custom posters and backdrops, generates artwork from local video when needed, and uses mpv for high-quality local playback. The goal is simple: keep ownership and control of local media while giving it the presentation, continuity, and ease of use associated with a premium streaming service.

HED Space is built around **local control**: media remains in the user's chosen directories, library data is stored locally in SQLite, and the application does not require a client/server media architecture.

**© 2026 Houssam Elmouqaddam. All rights reserved.**

## Media root

On first launch choose the existing **HED Space** folder. The scanner expects these child folders (matching is case-insensitive):

```text
HED Space/
├─ Anime/                # Anime is scanned as series / seasons / episodes
├─ Childhood/            # Childhood videos are grouped as series, not movie tiles
├─ Movies & Series/
├─ Posters/
├─ Songs/
└─ Tube/                 # Atmospheres
```

The app remembers the selected root in its local SQLite database.

## Implemented

### Real local catalog

- Recursive scan of `Movies & Series`, `Anime`, `Childhood`, `Songs`, and `Tube`.
- Movies and standard episode names (`S01E02`, `1x02`) are detected.
- Series are grouped into seasons and episodes.
- Music tags (title / artist / album / duration) and video/audio duration are read with FFprobe when available.
- SQLite is the source of truth for the UI. The prototype's hard-coded catalog is cleared before desktop rendering.
- Rescanning updates paths without duplicating existing entries and removes records for files that disappeared.

### Artwork and metadata

From the HED Space UI you can edit local metadata and choose artwork using the native Windows file picker.

- Movies: poster + backdrop.
- Series: poster + backdrop.
- Episodes: thumbnail.
- Songs: cover.
- Atmospheres: thumbnail + backdrop.

Selected artwork is copied into `HED Space/Posters/<category>/...`, and the SQLite record points to the managed copy. Manual titles/descriptions are preserved across rescans.

### Home

- Continue Watching is based on real playback history, most recently watched first.
- Playback resumes from the saved position.
- The Home mosaic is populated from random local **movies and series only**.
- Songs and Atmospheres are not inserted into Home recommendations.
- Watch progress refreshes when focus returns to HED Space after playback.

### Listen

- `Songs/` is the actual music source.
- No preview-song mapping is used in desktop mode.
- Real local audio files drive the Listen player.
- Track metadata and covers can be changed locally.

### Atmospheres

- `Tube/` is the Atmospheres source.
- Entering Atmospheres starts the first local atmosphere automatically and loops it.
- Browse and switch to another local atmosphere without leaving the page.
- Use original atmosphere audio or choose a song from `Songs/` as a soundtrack.
- Normal-mode soundtrack playback is independent of the atmosphere timeline.
- Fullscreen uses mpv and displays **pure video only**. `Esc` exits.
- When a soundtrack is selected in fullscreen, HED Space uses a separate audio-only mpv process so the song can loop independently of the long atmosphere video.

### Netflix-style movie / episode player

Movies, Childhood videos and episodes launch through **mpv**. A transparent fullscreen Tauri overlay supplies the custom HED Space controls:

- Play / pause.
- Rewind / forward 10 seconds.
- Seek timeline and current/remaining time.
- Volume.
- Audio-track selection.
- Subtitle selection + Off.
- Previous / next episode.
- Automatic next-episode handoff near the end.
- `Space`, arrow keys, `M`, `N`, and `Esc` shortcuts.
- Controls and cursor fade when inactive.
- Progress saved to SQLite every few seconds and on exit.

### Desktop window

The normal Windows title bar is disabled. HED Space supplies its own small minimize / maximize / close controls in the existing header, so there is no native `— □ X` strip above the design.

## Playback tools

HED Space deliberately does **not** contain third-party media binaries in this source archive.

For full playback, make `mpv.exe` available either:

1. in `src-tauri/bin/mpv.exe` before building, or
2. on the Windows `PATH`.

For fast library duration/tag probing, make `ffprobe.exe` available either:

1. in `src-tauri/bin/ffprobe.exe` before building, or
2. on `PATH`.

FFprobe is optional for browsing; mpv is required for the movie/episode/fullscreen-atmosphere engine.

You can use `scripts/prepare-tools.ps1` to copy binaries you already have into the bundle folder.

## Windows development setup

Install the normal Tauri Windows prerequisites (Rust stable with the MSVC toolchain, Microsoft C++ Build Tools/WebView2 as required by Tauri) and Node.js/npm.

Then, from this folder:

```powershell
npm install
npm run dev
```

To build the Windows installer:

```powershell
npm run build
```

The Tauri bundle target is NSIS and is configured as a per-user installation.

Run this before development to see what is missing:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\check-windows.ps1
```

## Important first test

Do not test the entire collection first. Create or use a small real subset and verify this chain:

```text
real media file
  -> scanner
  -> SQLite
  -> HED Space card
  -> details/artwork
  -> mpv playback
  -> saved progress
  -> Continue Watching
```

Once that succeeds, point the app at the full HED Space folder and rescan.

## Current validation status

The frontend JavaScript and the original inline prototype scripts have been syntax-checked, the HTML and JSON files have been parsed, and the SQLite schema is validated by `scripts/validate-source.py`.

This environment is not a Windows Tauri build machine, so the native Windows layer (Tauri window creation, named-pipe mpv IPC, native mpv OSC playback and NSIS packaging) still needs its runtime/build test on Windows. The source-level validator passes here.

## v0.1.7 — single-window native mpv player

The Watch player now uses **one visible window: mpv itself**. Earlier builds tried to place a transparent Tauri/WebView2 control window above mpv. On some Windows/GPU/WebView2 combinations mpv remained above that overlay, so only the raw video was visible and the controls could not be clicked.

v0.1.7 removes that fragile two-window design. HED Space launches mpv fullscreen with mpv's built-in OSC enabled and themed for HED Space:

- fullscreen borderless playback
- bottom control bar with a dark translucent base
- red HED/Netflix-style seek accent
- play/pause, seeking, volume, audio, subtitles and fullscreen controls rendered inside the mpv window
- audio/subtitle track menus available from the OSC
- controls fade after mouse inactivity
- `Space`/`K` play-pause
- `Left`/`Right` or `J`/`L` seek exactly 10 seconds
- `M` mute, `A` cycle audio, `S` cycle subtitles, `V` subtitle visibility
- `Esc` or `Q` closes the player and returns focus to HED Space
- mpv JSON IPC remains active so HED Space still saves real playback position/duration into SQLite for Continue Watching

Atmosphere fullscreen remains intentionally pure video without the Watch-player OSC.


## v0.1.8 player UI
The Watch player uses a custom HED Space Lua OSC rendered directly inside mpv. It adds a Netflix-style fading overlay, large timeline, play/pause, ±10-second seek, volume slider, audio/subtitle menus, and a clickable close button while keeping mpv as the playback engine.


## v0.2.0 title management
- Movie/video release filenames are cleaned before display (resolution, codec, bit depth and release-group suffixes are removed).
- Release year is stored separately and shown in metadata instead of being forced into the title.
- The details and series screens include a pencil button for editing the display title.
- Custom titles live in SQLite and remain locked across rescans; media files are never renamed.
- “Use scanned title” unlocks and restores the scanner-generated title.
- Long legitimate titles automatically use a smaller responsive type size instead of overflowing the details card.


## v0.2.1 player update

The single-window mpv player now includes a clickable fullscreen/exit-fullscreen button at the bottom-right. The existing `F` keyboard shortcut continues to work.


## Optional but recommended: ffmpeg.exe

If you also place `ffmpeg.exe` in `src-tauri/bin/`, HED Space can automatically generate **episode thumbnails** by extracting a still frame from each episode the first time it scans your library. Without `ffmpeg.exe`, episode thumbnails fall back to manual images or placeholders.


## Interface zoom

HED Space v0.6.29 adds an interface zoom control beside the top-right utility icons. Open the magnifier button and use **− / +**, click the percentage to reset to **100%**, or use **Ctrl− / Ctrl+ / Ctrl0**. The chosen zoom level is remembered locally.


## v0.6.29 subtitle styling

The movie/episode player now uses a cleaner HED Space subtitle preset: Segoe UI Semibold, smaller text, a thin soft outline/shadow, and a little more bottom margin. HED Space uses mpv's subtitle renderer; this styling does not transcode or re-encode the video. Bitmap subtitles such as PGS/DVD subtitles keep their authored appearance because they are images rather than text.


## v0.6.29 — high-resolution hero backdrops

When `ffmpeg.exe` is present in `src-tauri/bin`, HED Space now generates a dedicated high-resolution landscape backdrop when a movie or series does not already have one. The frame is extracted directly from the original local video at its native source resolution (no resizing step) and saved under `HED Space/Posters/Backdrops/`. Existing or manually selected backdrops always take priority and are never overwritten by rescans.


## Anime and Childhood grouping (v0.6.29)

Choose the **HED Space root**, not the Anime folder itself. For example, if your anime lives at `D:\HED Space\Anime`, choose `D:\HED Space` as the media root. Every video under `Anime/` and `Childhood/` is treated as an episode and grouped into a series. HED Space recognizes `S01E01`, `1x01`, `Episode 01`, `EP01`, and common anime names such as `Show - 01`. If a filename has no episode number, sorted files get stable fallback episode numbers.


## Multiple library locations (v0.6.29)

HED Space no longer requires every collection to live under one disk. The primary HED Space root is still used for managed artwork, but the **Library locations** button in the top-right can add any number of additional folders or drives.

Examples that can be scanned together:

- `C:\Users\elmou\Videos\HED Space` — primary root
- `D:\HED Space` — auto-detected as another HED Space-style root
- `D:\HED Space\Anime` — can be added directly as Anime
- any other folder can be assigned as Movies & Series, Anime, Childhood, Songs, or Atmospheres

All enabled locations are scanned in one transaction so media from one drive is not deleted when another drive is scanned. If an enabled source is unavailable, the scan stops instead of removing its database entries.


## v0.6.29 — Childhood series grouping

`Childhood` is now strictly a **series** collection. Files inside show/season folders group by those folders. If episodes are stored directly inside the `Childhood` folder, HED Space clusters them using repeated meaningful filename words while ignoring common broadcaster/episode terms (for example, `Cartoon Network` / `كرتون نتورك`). This is designed so flat Gumball episode files become one Gumball series instead of individual movie-like cards. A rescan migrates matching existing media rows in place, preserving their media IDs and progress.


### v0.6.29 series drawer polish

The series drawer now uses a wider cinematic layout, a cleaner backdrop gradient, larger 16:9 episode thumbnails, compact season pills, correct singular/plural metadata, and a dedicated play affordance. Series folders ending in `S2` / `Season 2` are normalized to the base show title and the suffix is used as the season number.


## v0.6.29 — Series & Anime + episode artwork

- Anime and standard series are shown together in a single **Series & Anime** collection while retaining Anime badges/categories.
- Each episode in the series drawer has its own **Image** edit button. Episode artwork is stored independently from the series poster and copied into the managed `Posters/Episodes` area.
- Changing a series poster still changes only the series-level artwork; episode thumbnails remain individually controllable.


### v0.6.29 anime season grouping
Anime release folders such as `[Anime Time] Attack on titan (Season 01)`, `(Season 02)`, and `[Season 4 Part 2]` are normalized to one show. The season number is kept separately, so Watch displays a single **Attack on Titan** card with its seasons inside.


### v0.6.29 series artwork behavior
Changing a **series poster** now immediately applies that poster to the thumbnail of every episode in the series. You can still use the per-episode **Image** button afterwards to give selected episodes their own thumbnail. Changing a series backdrop remains series-only.


## Generic library folders (v0.6.29)

HED Space no longer requires a folder named `HED Space`. On first launch, choose any local folder and assign what it contains: Movies, Series / TV shows, Movies & Series (mixed), Anime, Childhood / cartoons, Songs / music, Atmospheres, or Auto-detect. Add more folders later from **Library locations**. Auto-detect is intended for a parent folder containing category subfolders such as `Movies`, `Series`, `Anime`, `Childhood`, `Music`, or `Atmospheres`. Existing older HED Space roots are imported automatically as normal library sources.


### Ignored folders

The scanner skips any directory named `Openings` (case-insensitive), including everything inside it. This keeps anime/series opening videos from appearing as episodes. A rescan also removes previously indexed files that now fall under an ignored `Openings` folder.


## v0.6.29 warning cleanup

Removed obsolete code left over from the old single-library-root setup. The current multi-folder library source system is unchanged; this release only removes dead Rust functions that could produce compiler warnings.


## v0.6.29 visual fix

Replaced the font-based pencil/crayon glyph with a centered SVG icon in series and episode edit controls. This removes the baseline offset that made the icon appear off-center inside circular buttons.


## v0.6.29 Library hero cleanup

Removed the descriptive sentence from the Library hero and removed the floating featured-media label that could overlap the first collection heading (for example, `Childhood · Gumball`). The Library hero is now artwork + title + filters only.

## Copyright

HED Space and this application implementation are © 2026 **Houssam Elmouqaddam**. All rights reserved.

Third-party software and media remain subject to their respective licenses and rights holders. HED Space does not claim ownership of user-provided media, artwork, mpv, FFmpeg, or other third-party components.


## Standard application hero images

HED Space can now use permanent hero artwork that ships with the application instead of choosing the upper-page image from the media library.

Add images under:

```text
app/
└── Images/
    ├── home-hero.jpg
    ├── watch-hero.jpg
    ├── library-hero.jpg
    ├── listen-hero.jpg
    └── atmospheres-hero.jpg
```

JPG, JPEG, PNG and WebP are supported. The application automatically detects the extension. A supplied standard image takes priority over dynamic library artwork; if it is missing, the normal media-derived fallback is used.

For crisp full-screen hero artwork, use **3840×2160** where possible, with **1920×1080** as a practical minimum. These files are application assets and are included when the app is packaged.


## v0.6.29 fixes

- Removed the remaining Rust `PlayerSession.title` dead-code warning.
- Standard application hero artwork is now controlled by `app/Images/hero-images.json`, so images can use arbitrary filenames instead of requiring hard-coded names.
- Standard hero artwork is re-applied after the dynamic library is rebuilt, preventing media-library backdrops from replacing configured application artwork.
- Replaced the old prototype “More information” panel with a finished HED Space platform description and the copyright notice for Houssam Elmouqaddam.


## v0.6.29 startup intro restored

The animated HED Space welcome sequence is shown again on every desktop application launch. It is no longer suppressed by the desktop bridge or by the prototype session flag. The existing Skip intro / Enter your space controls continue to work.


## v0.6.29 fast startup

Startup no longer waits for a complete media-library scan before showing the application.

- The HED Space intro is visible from the very first rendered frame, removing the brief unfinished Watch-screen flash.
- The existing SQLite library is loaded immediately.
- The full filesystem/ffprobe scan runs in the background after startup and refreshes the UI when it completes.
- Standard app hero images are checked in parallel rather than one-by-one.

This keeps automatic library refresh while making repeat launches feel substantially faster.


## v0.6.29 moving details background

Movie and episode details now use a muted, blurred moving scene from the selected title instead of a static backdrop whenever possible.

- MP4/MOV/WebM sources are previewed directly when WebView2 can decode them.
- MKV/HEVC and other unsupported browser formats fall back to a cached 14-second H.264 preview generated by `ffmpeg.exe`.
- The generated preview is only UI artwork and never changes or recompresses the original movie used by mpv playback.
- The page scrollbar is locked while the details modal is open.


## v0.6.29 responsiveness + scroll fix

- The details/movie information overlay hides only its own scrollbar. Normal Home, Watch, Listen, Atmospheres, and Library page scrolling is no longer modified.
- Full library scans now execute on a Rust background worker instead of Tauri's UI thread.
- The automatic startup rescan waits 10 seconds before starting, so it does not collide with the intro transition.
- The cached SQLite library still appears immediately; scanning and ffprobe work continue in the background.


## v0.6.29 details visual polish

- The system/page scrollbar is hidden only while the movie/episode details overlay is open; all normal pages keep their regular scrolling.
- The static fallback backdrop is now blurred and darkened too, so there is no sharp image while a moving preview is being prepared.
- Moving preview clips fade in once playback actually starts.
- FFmpeg preview generation is faster (10 seconds, max 960px wide, ultrafast preset) because these clips are intentionally blurred UI backgrounds and are not used for movie playback.


## v0.6.29 refined intro

The welcome sequence now uses less repetitive copy:

- “Your collection, beautifully alive.”
- “Movies, series, music and atmospheres — brought together in one private cinematic experience.”
- “Enter HED Space →”

The animation and startup behavior are unchanged.


## v0.6.29 multiple standard hero images

`app/Images/hero-images.json` now supports both a single filename and an array of filenames for every section.

When an array is used, HED Space verifies the configured files, randomly selects one valid image once per application launch, and keeps that image stable for the whole session. Missing configured files are reported in the console; dynamic library artwork is only used when no configured image for that section can be loaded.


## v0.6.29 Listen collections

Listen now treats the first physical folder under a Songs source as a music collection.

Example:

```text
Songs/
├── Songs/
├── Soundtracks/
└── Spacetoon/
```

This produces three selectable Listen collections. Choosing `Spacetoon` filters the Listen experience to the tracks physically stored under that folder.

Collection artwork is independent from song artwork:

- Change a collection poster and all songs in that collection inherit it.
- Manually change a specific song cover and that song keeps its own cover instead.
- Manual per-song cover choices are marked in SQLite so rescanning does not replace them.

Atmospheres now use only tracks from a collection/folder named **Soundtracks** in the Soundtrack picker. Other music collections remain available in Listen but are not presented as atmosphere soundtracks.


## v0.6.29 Atmospheres transition fix

Fixed the brief zoom/crop jump when entering Atmospheres. The section transition previously left a transform on the main app while the fixed full-screen atmosphere video was being mounted. On Windows/WebView2 that temporarily changed the video's containing block, so the first frame appeared at a different scale. The transform is now removed before the Atmospheres view becomes visible, keeping the video geometry stable from its first visible frame.


## v0.6.29 Atmospheres + Listen audio fixes

- Atmospheres no longer uses the scale/sweep section transition at all. Entry is now a short opacity crossfade, eliminating the remaining first-frame zoom/crop jump in WebView2.
- The Listen player now has a real working volume slider and mute/unmute button. Volume is remembered locally between launches.
- Audio is section-exclusive: leaving Listen pauses the active song, and leaving Atmospheres pauses both the atmosphere video audio and any selected atmosphere soundtrack.
- Starting a Listen song also stops Atmospheres audio, preventing two independent audio sources from overlapping.


## v0.6.29 search music cards

Song results in Search now use the same square music-card presentation as the Library instead of being stretched into movie-style 2:3 poster cards. Search music cards keep square cover art, a music badge, compact metadata, and a hover play control. Selecting a song result moves directly to Listen and selects that track.


## v0.6.29 Search scrollbar fix

Search now has a single scrollbar. While the full-screen Search overlay is open,
the underlying application document is temporarily prevented from scrolling.
Only the Search overlay itself remains scrollable. Closing Search restores normal
Home, Watch, Listen, Atmospheres, and Library scrolling.


## v0.6.29 All music collection icon

The **All music** card in Listen now uses a dedicated built-in icon instead of
the generic placeholder artwork. This gives the top collection card a cleaner,
more intentional visual identity.


## v0.6.29 Atmospheres label cleanup

Removed the visible `Now inside` / current atmosphere title message from the
immersive Atmospheres screen. The video, soundtrack status, browse controls,
fullscreen control, and soundtrack control remain unchanged.


## v0.6.29 expanded Home mosaic

Home now displays up to **16 watchable cards** instead of 10. The extra six
items continue the existing editorial mosaic with two additional rows on large
screens, responsive two-column placement on medium screens, and the existing
single-column mobile layout.


## v0.6.29 Atmospheres fullscreen soundtrack ownership

Fixed duplicate soundtrack audio when entering external fullscreen Atmospheres.

Before launching the mpv fullscreen session, HED Space now pauses the embedded
WebView atmosphere video and its soundtrack. The mpv fullscreen session becomes
the only audio owner. HED Space monitors that external session and resumes the
embedded Atmospheres audio only after the mpv fullscreen window has actually
closed.

The Fullscreen button is also temporarily disabled while the external session is
starting, preventing accidental double launches.


## v0.6.29 Listen collection playback + Shuffle

The virtual **All music** card has been removed. Listen now opens directly into
the first real physical music collection and switching collection always scopes
the player to that folder.

Playback now continues automatically when a song ends:

- Shuffle **off**: the next track plays in order, wrapping within the collection.
- Shuffle **on**: HED Space chooses a random different track from the same
  selected collection and continues indefinitely.
- The Next button follows the same rule.
- Previous uses recent shuffle history when Shuffle is enabled.
- The Shuffle button is now a real toggle with a green active state.
- Opening a song from Search automatically switches Listen to that song's
  physical collection first.


## v0.6.29 Home mosaic sizing

The Home page editorial card wall now uses a more varied mosaic layout:

- mixed wide, tall, and standard tiles
- less repetitive equal-size rows
- denser desktop arrangement
- responsive tablet/mobile fallback kept intact

## v0.6.29 Intro polish

- changed intro headline to **Welcome to your space.**
- locked page scrolling while the intro is visible
- removed the underlying scrollbar during intro playback


## v0.6.29 Blue Atmospheres transition restored

The blue cinematic transition into Atmospheres is back.

The zoom bug remains specifically guarded against: Atmospheres no longer uses
the `section-moving` transform that scales the main application. The blue sweep
animates as an independent fixed overlay, and the Atmospheres view is mounted
while that overlay fully covers the screen. The fixed video therefore stays at
its final viewport geometry from its first visible frame.


## v0.6.29 Listen Repeat control

The Repeat icon in the Listen player is now operational.

- Click once to enable **Repeat current song**.
- The button turns green and shows a small `1`.
- When the track ends it restarts from the beginning.
- Click again to disable repeat.
- With repeat disabled, existing Shuffle / normal next-track behavior continues.

## v0.6.29 subtitle visual polish

The mpv subtitle presentation is now much cleaner and closer to a modern streaming player:

- smaller, stable subtitle size in fullscreen
- regular Segoe UI instead of heavy semibold text
- very thin dark outline instead of the thick cartoon-like stroke
- soft shadow for readability over bright scenes
- lower placement with more natural side margins
- slight edge softening for a less harsh rendered look

These settings only change subtitle rendering. Movie/video quality is unchanged.


## v0.6.29 Atmospheres soundtrack Shuffle

Atmospheres now has its own **Shuffle** control.

- Shuffle uses only songs from the physical `Soundtracks` music collection.
- `Original atmosphere` / native video audio is never included in Shuffle.
- Turning Shuffle on immediately starts a random soundtrack song.
- When that song ends, another different soundtrack is chosen automatically.
- Shuffle continues indefinitely while the Atmospheres page remains active.
- Choosing `Original atmosphere` manually turns Shuffle off.
- Fullscreen Atmospheres also receives the complete Soundtracks playlist and
  uses mpv playlist shuffle + infinite playlist looping, so Shuffle continues
  in fullscreen without starting duplicate browser audio.


## v0.6.29 intro punctuation

Removed the final period from the welcome headline:

**Welcome to your space**


## v0.6.29 subtitle style reset

Subtitle styling is now fully normalized by HED Space instead of partially
respecting the subtitle file's own ASS styling.

The default player presentation now uses:

- regular Segoe UI
- smaller 23px subtitle text
- no heavy black outline
- soft dark shadow only
- wider line margins
- stable size in fullscreen
- embedded subtitle fonts disabled
- authored ASS styling stripped so oversized/custom subtitle styles cannot take
  over the HED Space player

This affects subtitle rendering only and does not alter video quality.

## v0.6.29 subtitle rework

- cleaner video-relative subtitle sizing
- thin edge + soft shadow instead of the previous harsh styling
- centered multiline dialogue with wider margins
- SDH labels such as `[toddler]`, `[music]`, and speaker cues filtered from supported text subtitles


## v0.6.29 subtitle changes reverted

All subtitle-rendering experiments from v0.6.14 through v0.6.18 were removed.
The player is back to the subtitle configuration used in v0.6.13.


## v0.6.29 first-library setup spacing

The empty-library setup screen now has more breathing room:

- consistent space around the setup card
- slightly wider card so explanatory text wraps less
- more separation between folder type, examples, note, and action
- background/page scrolling is locked while setup is open
- compact-height fallback keeps the dialog usable on smaller windows


## v0.6.29 Atmospheres scroll removal

Atmospheres now locks both document axes while the immersive view is active.
The page scrollbar and overscroll are removed, while the atmosphere video and
controls remain fixed to the viewport.


## v0.6.29 smaller first-library popup

The first-library setup dialog is now more compact while preserving the spacing
added in v0.6.20. The card is narrower, the heading and body text are slightly
smaller, and the internal sections use tighter vertical spacing.


## v0.6.29 slightly smaller first-library popup

Reduced the setup dialog one more step without changing its overall design:
narrower width, slightly smaller heading, tighter padding, and slightly less
vertical spacing.


## v0.6.29 larger startup window

HED Space now opens at a larger default size of **1600 × 1000** and remains
centered on launch. The window is still resizable and is not forced into
fullscreen/maximized mode.


## v0.6.29 Atmospheres layout repair

Fixed the Atmospheres layout after increasing the startup window size.

- Atmospheres now occupies exactly the space below the normal HED Space header.
- Bottom Atmosphere controls are explicitly anchored inside the visible client area.
- Soundtrack status is anchored with the controls.
- No page scrollbar is reintroduced.
- The Atmospheres video itself is never transformed/scaled during this layout fix,
  preserving the existing anti-zoom protection.


## v0.6.29 Atmospheres panel close polish

The Soundtrack and Browse Atmospheres panels now use a custom cinematic close
control instead of the plain text `×`:

- larger glass circle
- thin blue accent ring
- subtle status-light detail
- custom drawn cross
- blue glow/rotation hover interaction
- accessible focus state and label


## v0.6.29 phantom Forced subtitle cleanup

The player no longer shows embedded `Forced · ENG` placeholder/forced subtitle
tracks in the subtitle menu.

- normal subtitle tracks still appear and can be selected
- movies now start with subtitles Off, preventing mpv from automatically
  selecting an embedded forced track
- this change does **not** alter subtitle font, size, rendering, or video quality


## v0.6.29 centered play icons

Library and Listen play badges now use a centered vector play icon.

- Library poster cards use an SVG play icon instead of the text triangle
- Listen quick cards, Listen album cards, and Library music cards use the same centered play glyph
- circle buttons now center the icon with flex alignment for cleaner geometry


## v0.6.29 distinct play-button designs

Play buttons now intentionally differ by section:

- **Listen:** bright green circular button with a solid black play triangle.
- **Library:** white circular button with black border and a black outlined play triangle.

Both icons remain optically centered inside their circles.


## v0.6.34 corrected Windows icon

The Windows app icon now uses the exact 512×512 icon supplied by the user.
`src-tauri/icons/icon.png` is the exact source artwork, while `icon.ico`
contains native 16/20/24/32/40/48/64/128/256 px layers generated only by
high-quality downsampling of that same artwork. No alternate HED/H logo is used.


## v0.6.34 no command-prompt flashes

All external media tools are now spawned through a shared Windows process
helper using `CREATE_NO_WINDOW`. This suppresses console/CMD windows from
ffmpeg, ffprobe, and mpv while preserving mpv's actual video window.


## v0.6.34 clean player back hover

The mpv player keeps the full clickable `← HED SPACE` back target, but hovering
it no longer draws the large black rectangle behind the label.


## v0.6.43 long-session fixes

- Cold launch now starts the Tauri window hidden with a dark native/WebView
  background and reveals it only after the intro has painted, removing the
  brief white startup flash.
- Forced subtitle tracks are removed from the mpv subtitle menu when mpv marks
  them forced or the embedded track title contains `FORCED`.
- Universal Search compensates for HED Space interface zoom, so the fullscreen
  search surface and content stay centered in smaller windows.
- Movie/series/episode playback and Atmosphere fullscreen now target the monitor
  containing the HED Space app window. mpv receives matching screen/fullscreen
  display selection so fullscreen does not jump back to the primary monitor.


## v0.6.43 physical-monitor fullscreen placement

Windows fullscreen no longer depends on Tauri and mpv agreeing on monitor names
or monitor indexes. HED Space now finds the physical monitor containing the
largest area of the app window, launches mpv paused/windowed, moves that exact
mpv process window to the selected monitor using its Windows HWND/PID, and only
then switches mpv to `fs-screen=current`. This is used for movies, series/anime
episodes, and Atmospheres.


## v0.6.43 Windows-native same-monitor fullscreen

Windows monitor selection now comes directly from HED Space's native HWND with
`MonitorFromWindow` and `GetMonitorInfoW`. The mpv window is explicitly restored
before it is moved, then fullscreened on `current`, and its exact target monitor
rectangle is re-applied after the fullscreen transition.

The v0.6.36 Rust warnings are also removed by compiling monitor-name/index fields
only on non-Windows targets.


## v0.6.43 clean section hero titles

Large section hero titles now use clean title-style wording with no commas or
periods between/end of lines, and each new line starts with an uppercase letter:

- Home: `Your space / Tonight`
- Watch: `Every story / Line by line`
- Listen: `Your rotation / Still yours`
- Library: `Everything / Findable`


## v0.6.43 remaining large-title cleanup

Cleaned the remaining large branding/section titles to match the same style:
no comma or period punctuation in the heading, and every new line begins with
an uppercase letter.

- Intro: `Welcome to / Your space`
- Listen landing: `Your sound / Without the file browser`
- Library landing: `Everything / Within reach`
- About: `Your collection / Your space`


## v0.6.43 playback, repeat and Library cleanup

- Player paused overlay uses a larger white play triangle with no black center circle.
- Listen Repeat One uses the audio element's native loop mode; the legacy prototype ended handler is disabled in the Tauri desktop app.
- Removed the false Windows autoplay warning messages from Listen and Atmosphere soundtrack playback.
- Library contains two content groups only: Movies, Series & Anime; and Childhood. Songs remain in Listen and Search.


## v0.6.43 transparent Library section headers

Removed the solid black background band behind Library section headers/counts. The section title and item count remain, but the header now blends directly into the Library background.


## v0.6.43 Atmosphere loading flash fix

Embedded Atmospheres no longer expose their thumbnail/poster for a split second when changing rooms or while WebView2 is preparing the video surface. The video surface stays black until a real decoded frame is available, then fades in quickly.
