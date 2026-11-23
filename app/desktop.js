(() => {
  const TAURI = window.__TAURI__;
  if (!TAURI?.core?.invoke) {
    console.warn('HED Space desktop bridge is available only inside Tauri.');
    return;
  }

  const { invoke, convertFileSrc } = TAURI.core;
  const { getCurrentWindow } = TAURI.window;
  const appWindow = getCurrentWindow();
  let startupWindowRevealed = false;

  async function revealStartupWindow() {
    if (startupWindowRevealed) return;
    startupWindowRevealed = true;
    // The native window starts hidden. Wait until WebView2 has painted two
    // frames of the already-visible intro, then reveal it. This removes the
    // brief white WebView/window flash seen before the intro on cold launch.
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    try {
      await appWindow.show();
    } catch (error) {
      console.warn('[HED Space] Could not reveal startup window', error);
    }
  }

  let desktopLibrary = null;
  let selectedDesktopEntity = null;
  let selectedSongId = null;
  let currentAtmosphereId = null;
  let currentSoundtrackId = null;
  let atmosphereShuffleEnabled = false;
  let atmosphereShuffleHistory = [];
  let mediaById = new Map();
  let seriesById = new Map();
  let musicCollectionById = new Map();
  let musicCollectionByKey = new Map();
  let activeMusicCollectionId = null;
  let listenShuffleEnabled = false;
  let listenShuffleHistory = [];
  let listenRepeatOneEnabled = false;
  let titleToMedia = new Map();


  // Permanent application artwork from app/Images/.
  // `hero-images.json` explicitly points each app section to an image file.
  const STANDARD_HERO_DEFAULTS = {
    home: 'home-hero.jpg',
    watch: 'watch-hero.jpg',
    library: 'library-hero.jpg',
    listen: 'listen-hero.jpg',
    atmospheres: 'atmospheres-hero.jpg'
  };
  const standardHeroImages = Object.create(null);

  function appImagesBaseUrl() {
    return new URL('Images/', document.baseURI);
  }

  function appImageUrl(filename) {
    return new URL(String(filename || '').replace(/^[/\\]+/, ''), appImagesBaseUrl()).href;
  }

  function canLoadImage(url) {
    return new Promise(resolve => {
      const img = new Image();
      let settled = false;
      const finish = ok => {
        if (settled) return;
        settled = true;
        img.onload = null;
        img.onerror = null;
        resolve(ok);
      };
      img.onload = () => finish(true);
      img.onerror = () => finish(false);
      img.src = `${url}${url.includes('?') ? '&' : '?'}hedv=${Date.now()}`;
    });
  }

  async function readHeroManifest() {
    const manifestUrl = new URL('hero-images.json', appImagesBaseUrl()).href;
    try {
      const response = await fetch(`${manifestUrl}?hedv=${Date.now()}`, { cache: 'no-store' });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const parsed = await response.json();
      return { ...STANDARD_HERO_DEFAULTS, ...(parsed || {}) };
    } catch (error) {
      console.warn('[HED Space] Could not read app/Images/hero-images.json; using default hero filenames.', error);
      return { ...STANDARD_HERO_DEFAULTS };
    }
  }

  function configuredHeroFiles(value) {
    const raw = Array.isArray(value) ? value : [value];
    return [...new Set(raw
      .map(item => String(item ?? '').trim())
      .filter(Boolean))];
  }

  function chooseSessionHero(items) {
    if (!items.length) return null;
    return items[Math.floor(Math.random() * items.length)] || items[0];
  }

  async function loadStandardHeroImages() {
    const configured = await readHeroManifest();

    await Promise.all(Object.keys(STANDARD_HERO_DEFAULTS).map(async section => {
      const filenames = configuredHeroFiles(configured?.[section]);
      standardHeroImages[section] = null;
      if (!filenames.length) return;

      // Check every configured file, then choose ONE valid image for this app
      // session. It remains stable until HED Space is restarted.
      const checks = await Promise.all(filenames.map(async filename => {
        const url = appImageUrl(filename);
        return {
          filename,
          url,
          available: await canLoadImage(url)
        };
      }));

      const valid = checks.filter(item => item.available);
      const missing = checks.filter(item => !item.available);

      for (const item of missing) {
        console.warn(`[HED Space] Configured ${section} hero not found: app/Images/${item.filename}`);
      }

      const selected = chooseSessionHero(valid);
      if (selected) {
        standardHeroImages[section] = selected.url;
        console.info(
          `[HED Space] Standard ${section} hero for this session: ${selected.filename} ` +
          `(${valid.length} valid configured image${valid.length === 1 ? '' : 's'})`
        );
      } else {
        console.warn(
          `[HED Space] No valid configured ${section} hero images were found; ` +
          `dynamic library artwork will be used.`
        );
      }
    }));
  }

  function standardHero(section) {
    return standardHeroImages[section] || null;
  }

  const UI_ZOOM_KEY = 'hed-space-ui-zoom';
  const UI_ZOOM_MIN = 0.70;
  const UI_ZOOM_MAX = 1.50;
  const UI_ZOOM_STEP = 0.10;
  let uiZoom = 1;

  function clampUiZoom(value) {
    const n = Number(value);
    if (!Number.isFinite(n)) return 1;
    return Math.min(UI_ZOOM_MAX, Math.max(UI_ZOOM_MIN, Math.round(n * 100) / 100));
  }

  function updateZoomReadout() {
    const pct = `${Math.round(uiZoom * 100)}%`;
    const readout = document.querySelector('#hedZoomValue');
    if (readout) readout.textContent = pct;
    const button = document.querySelector('#hedZoom');
    if (button) {
      button.title = `Interface zoom · ${pct}`;
      button.setAttribute('aria-label', `Interface zoom ${pct}`);
    }
    document.querySelector('#hedZoomOut')?.toggleAttribute('disabled', uiZoom <= UI_ZOOM_MIN + 0.001);
    document.querySelector('#hedZoomIn')?.toggleAttribute('disabled', uiZoom >= UI_ZOOM_MAX - 0.001);
  }

  function syncZoomSensitiveFixedOverlays() {
    // CSS `zoom` on <html> scales fixed overlays too. vw/vh still refer to the
    // unscaled viewport, so at >100% zoom a fullscreen Search layer becomes
    // wider than the actual app window and looks shifted/cropped.
    const searchOverlay = document.querySelector('#searchOverlay');
    if (!searchOverlay) return;
    const safeZoom = Math.max(0.01, uiZoom || 1);
    searchOverlay.style.width = `${window.innerWidth / safeZoom}px`;
    searchOverlay.style.height = `${window.innerHeight / safeZoom}px`;
  }

  function setUiZoom(value, persist = true) {
    uiZoom = clampUiZoom(value);
    document.documentElement.style.zoom = String(uiZoom);
    syncZoomSensitiveFixedOverlays();
    if (persist) {
      try { localStorage.setItem(UI_ZOOM_KEY, String(uiZoom)); } catch (_) {}
    }
    updateZoomReadout();
  }

  function restoreUiZoom() {
    let saved = 1;
    try { saved = Number(localStorage.getItem(UI_ZOOM_KEY) || 1); } catch (_) {}
    setUiZoom(saved, false);
  }

  function changeUiZoom(delta) { setUiZoom(uiZoom + delta); }

  restoreUiZoom();
  window.addEventListener('resize', syncZoomSensitiveFixedOverlays);

  // Windows filenames can legitimately begin with emoji/non-BMP characters.
  // JavaScript string indexing can split those into an invalid UTF-16 surrogate,
  // which makes encodeURIComponent throw "URIError: URI malformed". Keep every
  // string well-formed before it is used in HTML, SVG placeholders or asset URLs.
  const safeUnicode = (value='') => {
    const input = String(value ?? '');
    let output = '';
    for (let i = 0; i < input.length; i++) {
      const code = input.charCodeAt(i);
      if (code >= 0xD800 && code <= 0xDBFF) {
        const next = input.charCodeAt(i + 1);
        if (next >= 0xDC00 && next <= 0xDFFF) {
          output += input[i] + input[i + 1];
          i++;
        } else {
          output += '\uFFFD';
        }
      } else if (code >= 0xDC00 && code <= 0xDFFF) {
        output += '\uFFFD';
      } else {
        output += input[i];
      }
    }
    return output;
  };

  const htmlEscape = (value='') => safeUnicode(value)
    .replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;')
    .replaceAll('\"','&quot;').replaceAll("'",'&#039;');


  const durationText = seconds => {
    if (!Number.isFinite(seconds) || seconds <= 0) return '';
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    return h ? `${h}h ${m}m` : `${m} min`;
  };

  const progressPct = item => {
    const position = item?.progress_position || 0;
    const duration = item?.progress_duration || item?.duration || 0;
    return duration > 0 ? Math.max(0, Math.min(100, (position / duration) * 100)) : 0;
  };

  const placeholder = (title, shape='landscape') => {
    const w = shape === 'poster' ? 600 : shape === 'square' ? 700 : 1200;
    const h = shape === 'poster' ? 900 : shape === 'square' ? 700 : 675;
    const normalizedTitle = safeUnicode(title || 'HED Space');
    const initials = normalizedTitle.split(/\s+/).slice(0,3).map(x => Array.from(x)[0] || '').join('').toUpperCase();
    const safeTitle = htmlEscape(Array.from(normalizedTitle).slice(0,50).join(''));
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#15151a"/><stop offset="1" stop-color="#070708"/></linearGradient><radialGradient id="r"><stop stop-color="#ed2338" stop-opacity=".22"/><stop offset="1" stop-color="#ed2338" stop-opacity="0"/></radialGradient></defs><rect width="100%" height="100%" fill="url(#g)"/><circle cx="${w*.72}" cy="${h*.25}" r="${Math.min(w,h)*.42}" fill="url(#r)"/><text x="${w*.08}" y="${h*.68}" fill="#f5f1ec" font-family="Segoe UI,Arial" font-size="${Math.round(w*.12)}" font-weight="700">${htmlEscape(initials || 'HED')}</text><text x="${w*.08}" y="${h*.78}" fill="#8c888e" font-family="Segoe UI,Arial" font-size="${Math.round(w*.035)}">${safeTitle}</text></svg>`;
    return `data:image/svg+xml;charset=UTF-8,${encodeURIComponent(svg)}`;
  };

  const localUrl = path => {
    if (!path) return '';
    try {
      return convertFileSrc(safeUnicode(path));
    } catch (error) {
      console.warn('[HED Space] Could not convert local file path', path, error);
      return '';
    }
  };
  const mediaPoster = item => localUrl(item?.poster_path || item?.thumbnail_path) || placeholder(item?.title, item?.kind === 'song' ? 'square' : 'poster');
  const mediaLandscape = item => localUrl(item?.backdrop_path || item?.thumbnail_path || item?.poster_path) || placeholder(item?.title, 'landscape');
  const musicCollectionForSong = item => item?.music_collection_key ? musicCollectionByKey.get(item.music_collection_key) : null;
  const musicCollectionCover = collection => {
    if (!collection) return placeholder('Music','square');
    const firstArtwork = collection.songs?.find(song => song.thumbnail_path || song.poster_path);
    return localUrl(collection.poster_path)
      || localUrl(firstArtwork?.thumbnail_path || firstArtwork?.poster_path)
      || placeholder(collection.title, 'square');
  };
  const songCover = item => {
    const collection = musicCollectionForSong(item);
    // A manually selected song image always wins. Otherwise the collection
    // poster is inherited by every track in that physical folder.
    if (item?.thumbnail_locked) {
      return localUrl(item.thumbnail_path || item.poster_path) || placeholder(item.title, 'square');
    }
    return localUrl(collection?.poster_path)
      || localUrl(item?.thumbnail_path || item?.poster_path)
      || placeholder(item?.title, 'square');
  };
  const seriesPoster = item => localUrl(item?.poster_path) || placeholder(item?.title, 'poster');
  const seriesLandscape = item => localUrl(item?.backdrop_path || item?.poster_path) || placeholder(item?.title, 'landscape');

  function mediaMeta(item) {
    if (!item) return '';
    if (item.kind === 'episode') {
      const s = item.season_number ?? 1, e = item.episode_number ?? 1;
      return `S${s} E${e}${item.duration ? ` · ${durationText(item.duration)}` : ''}`;
    }
    if (item.kind === 'song') return [item.artist, item.album].filter(Boolean).join(' · ') || 'Local audio';
    if (item.kind === 'atmosphere') return `Atmosphere${item.duration ? ` · ${durationText(item.duration)}` : ''}`;
    const parts = [];
    if (item.year) parts.push(String(item.year));
    if (item.duration) parts.push(durationText(item.duration));
    if (!parts.length) parts.push(item.kind === 'childhood' ? 'Childhood' : 'Local movie');
    return parts.join(' · ');
  }

  function mediaCard(item, tag) {
    return {
      title: item.title,
      img: mediaLandscape(item),
      poster: mediaPoster(item),
      meta: mediaMeta(item),
      tag: tag || (item.kind === 'childhood' ? 'Childhood' : item.kind === 'episode' ? (item.series_title || 'Series') : 'Movie'),
      progress: progressPct(item),
      zoom: 1.04,
      position: '50% 50%',
      _entity: 'media',
      _id: item.id
    };
  }

  function seriesCard(item) {
    const count = item.seasons.reduce((n,s) => n + s.episodes.length, 0);
    return {
      title: item.title,
      img: seriesLandscape(item),
      poster: seriesPoster(item),
      meta: `${item.seasons.length} season${item.seasons.length === 1 ? '' : 's'} · ${count} episode${count === 1 ? '' : 's'}`,
      tag: item.category === 'anime' ? 'Anime' : item.category === 'childhood' ? 'Childhood' : 'Series', progress: 0, zoom: 1.04, position:'50% 50%', _entity:'series', _id:item.id
    };
  }

  function songCard(item) {
    return {
      title: item.title,
      img: songCover(item),
      poster: songCover(item),
      meta: item.artist || 'Local audio',
      tag: item.album || 'Music',
      progress: 0,
      _entity:'media', _id:item.id
    };
  }

  function installWindowChrome() {
    const header = document.querySelector('.header');
    const utilities = document.querySelector('.utilities');
    if (!header || !utilities || document.querySelector('.hed-window-controls')) return;
    header.setAttribute('data-tauri-drag-region','');

    const zoomWrap = document.createElement('div');
    zoomWrap.className = 'hed-zoom-wrap';
    zoomWrap.innerHTML = `
      <button class="utility hed-zoom" id="hedZoom" type="button" title="Interface zoom · 100%" aria-label="Interface zoom 100%" aria-expanded="false">
        <svg class="utility-ring" viewBox="0 0 39 39" aria-hidden="true"><circle cx="19.5" cy="19.5" r="19"></circle></svg>
        <svg class="utility-symbol hed-zoom-symbol" viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.6" cy="10.6" r="4.7"/><path d="M14.2 14.2 19.5 19.5M10.6 8.2v4.8M8.2 10.6H13"/></svg>
      </button>
      <div class="hed-zoom-popover" id="hedZoomPopover" aria-hidden="true">
        <span class="hed-zoom-label">Interface size</span>
        <div class="hed-zoom-row">
          <button type="button" id="hedZoomOut" aria-label="Zoom out">−</button>
          <button type="button" class="hed-zoom-value" id="hedZoomValue" title="Reset to 100%">100%</button>
          <button type="button" id="hedZoomIn" aria-label="Zoom in">+</button>
        </div>
        <small>Ctrl − &nbsp; Ctrl 0 &nbsp; Ctrl +</small>
      </div>`;
    utilities.append(zoomWrap);

    const zoomButton = zoomWrap.querySelector('#hedZoom');
    const zoomPopover = zoomWrap.querySelector('#hedZoomPopover');
    const closeZoomPopover = () => {
      zoomWrap.classList.remove('open');
      zoomPopover.setAttribute('aria-hidden','true');
      zoomButton.setAttribute('aria-expanded','false');
    };
    zoomButton.onclick = event => {
      event.stopPropagation();
      const open = zoomWrap.classList.toggle('open');
      zoomPopover.setAttribute('aria-hidden', String(!open));
      zoomButton.setAttribute('aria-expanded', String(open));
    };
    zoomWrap.querySelector('#hedZoomOut').onclick = event => { event.stopPropagation(); changeUiZoom(-UI_ZOOM_STEP); };
    zoomWrap.querySelector('#hedZoomIn').onclick = event => { event.stopPropagation(); changeUiZoom(UI_ZOOM_STEP); };
    zoomWrap.querySelector('#hedZoomValue').onclick = event => { event.stopPropagation(); setUiZoom(1); };
    zoomPopover.onclick = event => event.stopPropagation();
    document.addEventListener('click', event => { if (!zoomWrap.contains(event.target)) closeZoomPopover(); });
    updateZoomReadout();

    const sources = document.createElement('button');
    sources.className = 'utility hed-sources';
    sources.id = 'hedSources';
    sources.title = 'Library locations';
    sources.setAttribute('aria-label','Library locations');
    sources.innerHTML = `<svg class="utility-ring" viewBox="0 0 39 39" aria-hidden="true"><circle cx="19.5" cy="19.5" r="18.5"></circle></svg><svg class="utility-symbol hed-source-symbol" viewBox="0 0 24 24" aria-hidden="true"><path class="hed-folder-shape" d="M3.75 7.4c0-1.02.83-1.85 1.85-1.85h4.05l1.8 2.05h6.95c1.02 0 1.85.83 1.85 1.85v7.15c0 1.02-.83 1.85-1.85 1.85H5.6a1.85 1.85 0 0 1-1.85-1.85V7.4Z"/><path class="hed-folder-plus" d="M12 10.35v5.3M9.35 13h5.3"/></svg>`;
    utilities.append(sources);
    sources.onclick = () => openSourcesManager();

    const sync = document.createElement('button');
    sync.className = 'utility hed-sync';
    sync.id = 'hedSync';
    sync.title = 'Rescan local library';
    sync.innerHTML = `<svg class="utility-ring" viewBox="0 0 39 39"><circle cx="19.5" cy="19.5" r="19"></circle></svg><svg class="utility-symbol search-symbol" viewBox="0 0 24 24" aria-hidden="true"><path d="M19 7v5h-5M5 17v-5h5"/><path d="M7.4 8.1A7 7 0 0 1 19 12M16.6 15.9A7 7 0 0 1 5 12"/></svg>`;
    utilities.append(sync);

    const controls = document.createElement('div');
    controls.className = 'hed-window-controls';
    controls.innerHTML = `
      <button class="hed-window-control" id="hedMin" title="Minimize"><svg viewBox="0 0 16 16"><path d="M3 11.5h10"/></svg></button>
      <button class="hed-window-control" id="hedMax" title="Maximize"><svg viewBox="0 0 16 16"><rect x="3.2" y="3.2" width="9.6" height="9.6" rx=".7"/></svg></button>
      <button class="hed-window-control close" id="hedClose" title="Close"><svg viewBox="0 0 16 16"><path d="M3.5 3.5l9 9m0-9-9 9"/></svg></button>`;
    utilities.append(controls);
    document.querySelector('#hedMin').onclick = () => appWindow.minimize();
    document.querySelector('#hedMax').onclick = () => appWindow.toggleMaximize();
    document.querySelector('#hedClose').onclick = () => appWindow.close();
    sync.onclick = async () => {
      sync.classList.add('spinning');
      try {
        const report = await invoke('scan_library');
        await loadLibrary(false);
        flash(`Library refreshed · ${report.total} local items`);
      } catch (error) { flash(String(error)); }
      finally { sync.classList.remove('spinning'); }
    };
  }

  document.addEventListener('keydown', event => {
    if (!event.ctrlKey || event.altKey || event.metaKey) return;
    const key = event.key;
    if (key === '+' || key === '=' || key === 'Add') {
      event.preventDefault();
      changeUiZoom(UI_ZOOM_STEP);
    } else if (key === '-' || key === '_' || key === 'Subtract') {
      event.preventDefault();
      changeUiZoom(-UI_ZOOM_STEP);
    } else if (key === '0') {
      event.preventDefault();
      setUiZoom(1);
    }
  });

  function installSourcesManager() {
    if (document.querySelector('#hedSourcesModal')) return;
    const modal = document.createElement('section');
    modal.className = 'hed-sources-modal';
    modal.id = 'hedSourcesModal';
    modal.innerHTML = `<div class="hed-sources-card">
      <div class="hed-edit-head"><div><div class="hed-setup-kicker">Local library</div><h2>Library locations</h2></div><button class="hed-edit-close" id="hedSourcesClose">×</button></div>
      <p class="hed-sources-copy">Add folders from any drive and choose what each folder contains. Folders can be anywhere and can use any name. Auto-detect is for a parent folder that contains category subfolders such as Movies, Series, Anime, Childhood, Music, or Atmospheres.</p>
      <div class="hed-source-add">
        <label><span>This folder contains</span><select id="hedSourceType"><option value="auto">Auto-detect category subfolders</option><option value="movies">Movies</option><option value="series">Series / TV shows</option><option value="standard">Movies &amp; Series (mixed)</option><option value="anime">Anime</option><option value="childhood">Childhood / cartoons</option><option value="songs">Songs / music</option><option value="atmospheres">Atmospheres / ambient videos</option></select></label>
        <button class="primary" id="hedAddSource">+ Choose folder</button>
      </div>
      <div class="hed-source-list" id="hedSourceList"></div>
      <div class="hed-source-footer"><span id="hedSourceStatus"></span><button class="hed-ghost-button" id="hedSourcesRescan">Rescan all locations</button></div>
    </div>`;
    document.body.append(modal);
    document.querySelector('#hedSourcesClose').onclick = () => modal.classList.remove('open');
    modal.onclick = event => { if (event.target === modal) modal.classList.remove('open'); };
    document.querySelector('#hedAddSource').onclick = addSourceFromPicker;
    document.querySelector('#hedSourcesRescan').onclick = rescanAllSources;
  }

  const sourceTypeLabel = type => ({
    auto:'Auto-detect root', movies:'Movies', series:'Series / TV shows', standard:'Movies & Series', anime:'Anime', childhood:'Childhood / cartoons', songs:'Songs / music', atmospheres:'Atmospheres'
  }[type] || type);

  async function refreshSourcesManager() {
    const list = document.querySelector('#hedSourceList');
    if (!list) return;
    try {
      const sources = await invoke('list_library_sources');
      const typeOptions = source => [
        ['auto','Auto-detect root'],['movies','Movies'],['series','Series / TV shows'],['standard','Movies & Series'],['anime','Anime'],['childhood','Childhood / cartoons'],['songs','Songs / music'],['atmospheres','Atmospheres']
      ].map(([value,label]) => `<option value="${value}"${source.source_type===value?' selected':''}>${label}</option>`).join('');
      list.innerHTML = sources.length ? sources.map(source => `<div class="hed-source-row">
        <div class="hed-source-main"><strong>${htmlEscape(source.path)}</strong><small>Scan assignment</small></div>
        <select class="hed-source-type-select" data-source-type-id="${source.id}" aria-label="Folder assignment for ${htmlEscape(source.path)}">${typeOptions(source)}</select>
        <button class="hed-source-remove" data-source-id="${source.id}" title="Remove this scan location">Remove</button>
      </div>`).join('') : '<div class="hed-empty"><span><strong>No media folders yet</strong>Choose a folder above and assign what it contains.</span></div>';
      list.querySelectorAll('[data-source-type-id]').forEach(select => select.onchange = async () => {
        try {
          select.disabled = true;
          await invoke('update_library_source_type', { id:Number(select.dataset.sourceTypeId), sourceType:select.value });
          await rescanAllSources();
        } catch (error) { flash(String(error)); await refreshSourcesManager(); }
        finally { select.disabled = false; }
      });
      list.querySelectorAll('[data-source-id]').forEach(button => button.onclick = async () => {
        try {
          await invoke('remove_library_source', { id:Number(button.dataset.sourceId) });
          await refreshSourcesManager();
          const remaining = await invoke('list_library_sources');
          if (remaining.length) await rescanAllSources(); else {
            document.querySelector('#hedSourcesModal')?.classList.remove('open');
            document.querySelector('#hedSetup')?.classList.add('open');
          }
        } catch (error) { flash(String(error)); }
      });
    } catch (error) {
      list.innerHTML = `<div class="hed-empty">${htmlEscape(String(error))}</div>`;
    }
  }

  async function openSourcesManager() {
    installSourcesManager();
    document.querySelector('#hedSourcesModal').classList.add('open');
    await refreshSourcesManager();
  }

  async function addSourceFromPicker() {
    const status = document.querySelector('#hedSourceStatus');
    const sourceType = document.querySelector('#hedSourceType')?.value || 'auto';
    const path = await invoke('pick_media_root');
    if (!path) return;
    try {
      status.textContent = 'Adding location…';
      await invoke('add_library_source', { path, sourceType });
      await refreshSourcesManager();
      await rescanAllSources();
      status.textContent = 'Location added';
    } catch (error) { status.textContent = String(error); flash(String(error)); }
  }

  async function rescanAllSources() {
    const status = document.querySelector('#hedSourceStatus');
    try {
      if (status) status.textContent = 'Scanning all locations…';
      const report = await invoke('scan_library');
      await loadLibrary(false);
      if (status) status.textContent = `${report.total} local items across all locations`;
      flash(`Library refreshed · ${report.total} local items`);
    } catch (error) {
      if (status) status.textContent = String(error);
      flash(String(error));
    }
  }

  function installSetup() {
    if (document.querySelector('#hedSetup')) return;
    const setup = document.createElement('section');
    setup.className = 'hed-setup';
    setup.id = 'hedSetup';
    setup.innerHTML = `<div class="hed-setup-card hed-setup-card-wide"><div class="hed-setup-kicker">Local media library</div><h1>Add your first media folder.</h1><p>Your media can live anywhere on C:, D:, external drives, or any other local folder. Choose what the folder contains, then select it. Files are indexed in place — nothing is moved or renamed.</p><div class="hed-setup-assignment"><label><span>This folder contains</span><select id="hedSetupSourceType"><option value="movies">Movies</option><option value="series">Series / TV shows</option><option value="standard">Movies &amp; Series (mixed)</option><option value="anime">Anime</option><option value="childhood">Childhood / cartoons</option><option value="songs">Songs / music</option><option value="atmospheres">Atmospheres / ambient videos</option><option value="auto">Auto-detect category subfolders</option></select></label></div><div class="hed-setup-example"><strong>Examples</strong><br>D:&#92;Movies <span>→ Movies</span><br>D:&#92;Anime <span>→ Anime</span><br>E:&#92;TV Shows <span>→ Series / TV shows</span><br>C:&#92;My Media <span>→ Auto-detect if it contains category subfolders</span></div><p class="hed-setup-note"><strong>Auto-detect root:</strong> use this only when the selected folder contains subfolders such as <em>Movies</em>, <em>Series</em>, <em>Anime</em>, <em>Childhood</em>, <em>Music</em>, or <em>Atmospheres</em>.</p><div class="hed-setup-actions"><button class="primary" id="hedChooseRoot">Choose folder</button><span class="hed-setup-status" id="hedSetupStatus"></span></div></div>`;
    document.body.append(setup);
    document.querySelector('#hedChooseRoot').onclick = chooseRoot;
  }

  async function chooseRoot() {
    const status = document.querySelector('#hedSetupStatus');
    const sourceType = document.querySelector('#hedSetupSourceType')?.value || 'movies';
    const root = await invoke('pick_media_root');
    if (!root) return;
    status.textContent = 'Adding and scanning folder…';
    try {
      await invoke('add_library_source', { path:root, sourceType });
      const report = await invoke('scan_library');
      status.textContent = `${report.total} items found`;
      await loadLibrary(false);
      document.querySelector('#hedSetup').classList.remove('open');
      flash(`Connected · ${report.total} local items`);
    } catch (error) {
      status.textContent = String(error);
    }
  }

  function installEditor() {
    if (document.querySelector('#hedEdit')) return;
    const modal = document.createElement('section');
    modal.id = 'hedEdit'; modal.className = 'hed-edit';
    modal.innerHTML = `<div class="hed-edit-card"><div class="hed-edit-head"><div><div class="hed-setup-kicker">Local metadata</div><h2 id="hedEditHeading">Edit</h2></div><button class="hed-edit-close" id="hedEditClose">×</button></div><label class="hed-field"><span>Display title</span><input id="hedEditTitle" maxlength="180"></label><p class="hed-field-note">This changes only the name shown in HED Space. Your media filename is never renamed.</p><label class="hed-field hed-artist-field"><span>Artist</span><input id="hedEditArtist"></label><label class="hed-field hed-album-field"><span>Album</span><input id="hedEditAlbum"></label><label class="hed-field"><span>Description</span><textarea id="hedEditDescription"></textarea></label><div class="hed-edit-actions hed-edit-actions-split"><button class="hed-ghost-button" id="hedEditReset">Use scanned title</button><span class="hed-edit-spacer"></span><button class="hed-ghost-button" id="hedEditCancel">Cancel</button><button class="primary" id="hedEditSave">Save</button></div></div>`;
    document.body.append(modal);
    document.querySelector('#hedEditClose').onclick = closeEditor;
    document.querySelector('#hedEditCancel').onclick = closeEditor;
    document.querySelector('#hedEditReset').onclick = resetEditorTitle;
    modal.onclick = e => { if (e.target === modal) closeEditor(); };
    document.querySelector('#hedEditSave').onclick = saveEditor;
  }

  function openEditor(entityType, entity) {
    selectedDesktopEntity = { entityType, entity };
    const isSong = entityType === 'media' && entity.kind === 'song';
    document.querySelector('#hedEditHeading').textContent = entityType === 'series' ? 'Edit series' : 'Edit details';
    document.querySelector('#hedEditTitle').value = entity.title || '';
    document.querySelector('#hedEditDescription').value = entity.description || '';
    document.querySelector('#hedEditArtist').value = entity.artist || '';
    document.querySelector('#hedEditAlbum').value = entity.album || '';
    document.querySelector('.hed-artist-field').style.display = isSong ? '' : 'none';
    document.querySelector('.hed-album-field').style.display = isSong ? '' : 'none';
    document.querySelector('#hedEdit').classList.add('open');
  }
  function closeEditor(){ document.querySelector('#hedEdit')?.classList.remove('open'); }

  async function resetEditorTitle() {
    if (!selectedDesktopEntity) return;
    const { entityType, entity } = selectedDesktopEntity;
    try {
      await invoke('reset_title', { entityType, id: entity.id });
      closeEditor();
      await loadLibrary(false);
      const refreshed = entityType === 'series' ? seriesById.get(entity.id) : mediaById.get(entity.id);
      if (refreshed) {
        selectedDesktopEntity = { entityType, entity: refreshed };
        if (entityType === 'series') openSeriesById(refreshed.id); else openMediaById(refreshed.id);
      }
      flash('Restored scanned title');
    } catch (error) { flash(String(error)); }
  }

  async function saveEditor() {
    if (!selectedDesktopEntity) return;
    const { entityType, entity } = selectedDesktopEntity;
    const title = document.querySelector('#hedEditTitle').value.trim();
    if (!title) { flash('Title cannot be empty'); return; }
    try {
      await invoke('update_metadata', {
        entityType,
        id: entity.id,
        patch: {
          title,
          description: document.querySelector('#hedEditDescription').value.trim(),
          artist: document.querySelector('#hedEditArtist').value.trim() || null,
          album: document.querySelector('#hedEditAlbum').value.trim() || null
        }
      });
      closeEditor();
      await loadLibrary(false);
      const refreshed = entityType === 'series' ? seriesById.get(entity.id) : mediaById.get(entity.id);
      if (refreshed) {
        selectedDesktopEntity = { entityType, entity: refreshed };
        if (entityType === 'series') openSeriesById(refreshed.id); else openMediaById(refreshed.id);
      }
      flash('Saved locally');
    } catch (error) { flash(String(error)); }
  }

  async function chooseArtwork(entityType, id, artKind) {
    const selected = await invoke('pick_image');
    if (!selected) return;
    try {
      await invoke('set_artwork', { entityType, id, artKind, sourcePath:selected });
      await loadLibrary(false);
      flash(`${artKind[0].toUpperCase()+artKind.slice(1)} updated`);
    } catch (error) { flash(String(error)); }
  }

  async function chooseEpisodeArtwork(episodeId, seriesId, seasonIndex) {
    const selected = await invoke('pick_image');
    if (!selected) return;
    try {
      await invoke('set_artwork', { entityType:'media', id:Number(episodeId), artKind:'thumbnail', sourcePath:selected });
      await loadLibrary(false);
      openSeriesById(Number(seriesId), Number(seasonIndex));
      flash('Episode image updated');
    } catch (error) { flash(String(error)); }
  }

  function indexLibrary(lib) {
    mediaById = new Map();
    seriesById = new Map();
    musicCollectionById = new Map();
    musicCollectionByKey = new Map();
    titleToMedia = new Map();

    const allMedia = [...lib.movies, ...lib.childhood, ...lib.songs, ...lib.atmospheres, ...lib.series.flatMap(s => s.seasons.flatMap(x => x.episodes))];
    allMedia.forEach(item => {
      mediaById.set(item.id,item);
      if (!titleToMedia.has(item.title)) titleToMedia.set(item.title,item);
    });
    lib.series.forEach(item => seriesById.set(item.id,item));
    (lib.music_collections || []).forEach(item => {
      musicCollectionById.set(item.id,item);
      musicCollectionByKey.set(item.collection_key,item);
    });

    const collections = lib.music_collections || [];
    if (!activeMusicCollectionId || !musicCollectionById.has(Number(activeMusicCollectionId))) {
      activeMusicCollectionId = collections[0]?.id ?? null;
      listenShuffleHistory = [];
    }
  }

  function replaceStaticData(lib) {
    // Clear every production-facing prototype array so fake content cannot leak back into the UI.
    [action, heroesRow, epics, childhood, aot, hxh, animeWorlds].forEach(arr => arr.splice(0, arr.length));
    Object.keys(seriesData).forEach(key => delete seriesData[key]);
    Object.keys(descriptions).forEach(key => delete descriptions[key]);
    Object.keys(previewByTitle).forEach(key => delete previewByTitle[key]);
    Object.keys(posterMap).forEach(key => delete posterMap[key]);

    const movieCards = lib.movies.map(x => mediaCard(x,'Movie'));
    const seriesAndAnime = lib.series.filter(x => x.category !== 'childhood');
    const childhoodSeries = lib.series.filter(x => x.category === 'childhood');
    const seriesCards = seriesAndAnime.map(seriesCard);
    const childhoodCards = childhoodSeries.map(seriesCard);
    const songCards = lib.songs.map(songCard);
    const continueCards = lib.continue_watching.map(x => mediaCard(x, x.series_title || (x.kind === 'childhood' ? 'Childhood' : 'Continue')));
    const recentCards = lib.recently_added.map(x => mediaCard(x, x.series_title || (x.kind === 'childhood' ? 'Childhood' : x.kind === 'episode' ? 'Series' : 'Movie')));

    libraryCatalog.movies = movieCards;
    libraryCatalog.series = seriesCards;
    // Anime remains tagged as Anime on its cards, but lives in the same Series & Anime collection.
    libraryCatalog.anime = [];
    libraryCatalog.childhood = childhoodCards;
    libraryCatalog.music = songCards;

    rows.watch = [
      { title:'Continue watching', subtitle:'Most recently watched first', cards:continueCards },
      { title:'Movies', subtitle:'Your local cinema', cards:movieCards },
      { title:'Series & Anime', subtitle:'All episodic journeys in one collection', cards:seriesCards },
      { title:'Childhood', subtitle:'Shows grouped into seasons and episodes', cards:childhoodCards },
      { title:'Recently added', subtitle:'New on this PC', cards:recentCards }
    ].filter(row => row.cards.length);

    music.splice(0, music.length, ...songCards);
    soundtrack.splice(0, soundtrack.length);
    Object.keys(listenPreviewByTitle).forEach(key => delete listenPreviewByTitle[key]);
    lib.songs.forEach(song => { listenPreviewByTitle[song.title] = localUrl(song.file_path); });

    roomData.splice(0, roomData.length, ...lib.atmospheres.map(item => ({
      id:String(item.id), mediaId:item.id, title:item.title,
      src:localUrl(item.file_path),
      poster:localUrl(item.backdrop_path || item.thumbnail_path) || placeholder(item.title,'landscape'),
      thumb:localUrl(item.thumbnail_path || item.poster_path) || placeholder(item.title,'landscape'),
      meta:mediaMeta(item)
    })));

    lib.series.forEach(series => {
      seriesData[series.title] = {
        id:series.id,
        poster:seriesPoster(series),
        backdrop:seriesLandscape(series),
        meta:`${series.seasons.length} season${series.seasons.length===1?'':'s'}`,
        intro:series.description || 'Your local series collection.',
        seasons:series.seasons.map(season => ({
          name:`Season ${season.season_number}`,
          count:season.episodes.length,
          items:season.episodes.map(ep => mediaCard(ep,series.title))
        }))
      };
    });

    buildHome(lib);
    rebuildAtmosphereControls(lib);

    // Permanent app hero artwork, when provided in app/Images/, takes precedence.
    if (standardHero('home')) heroes.home.image = standardHero('home');
    if (standardHero('watch')) heroes.watch.image = standardHero('watch');
    if (standardHero('listen')) heroes.listen.image = standardHero('listen');
    if (standardHero('library')) heroes.library.image = standardHero('library');

    const fallbackHero=heroes.home.image||placeholder('HED Space','landscape');
    sweepConfig.home.image=`url("${standardHero('home')||heroes.home.image||fallbackHero}")`;
    sweepConfig.watch.image=`url("${standardHero('watch')||heroes.watch.image||fallbackHero}")`;
    sweepConfig.library.image=`url("${standardHero('library')||heroes.library?.image||standardHero('watch')||heroes.watch.image||fallbackHero}")`;
    sweepConfig.listen.image=`url("${standardHero('listen')||heroes.listen?.image||(lib.songs.length?songCover(lib.songs[0]):fallbackHero)}")`;
    sweepConfig.atmospheres.image=`url("${standardHero('atmospheres')||(lib.atmospheres.length?(localUrl(lib.atmospheres[0].thumbnail_path||lib.atmospheres[0].poster_path)||placeholder(lib.atmospheres[0].title,'landscape')):fallbackHero)}")`;

    // If the current section is already visible, apply its permanent image immediately.
    const currentHero = standardHero(app?.dataset?.section || currentSection || 'home');
    if (currentHero && heroBg) heroBg.style.backgroundImage = `url("${currentHero}")`;
  }

  function shuffle(items) {
    const copy = [...items];
    for (let i=copy.length-1;i>0;i--) { const j=Math.floor(Math.random()*(i+1)); [copy[i],copy[j]]=[copy[j],copy[i]]; }
    return copy;
  }

  function buildHome(lib) {
    const watchable = [...lib.movies.map(x=>({kind:'media',entity:x,card:mediaCard(x,'Movie')})), ...lib.series.map(x=>({kind:'series',entity:x,card:seriesCard(x)}))];
    const latest = lib.continue_watching[0];
    const random = shuffle(watchable).slice(0,18);
    const slots = [
      'feature','aot','music','gumball',
      'storm','hunter','jazz',
      'shaun','interstellar','late',
      'orbit','ember','noir',
      'vault','echo','after'
    ];
    homeTiles.splice(0, homeTiles.length);
    if (latest) {
      const card = mediaCard(latest, latest.series_title || 'Continue');
      homeTiles.push({cls:slots[0],display:latest.title,target:latest.title,kicker:`Continue · ${Math.round(progressPct(latest))}%`,img:card.img,kind:'media',mediaId:latest.id,zoom:1.02,pos:'50% 45%'});
    }
    for (const choice of random) {
      if (homeTiles.length >= slots.length) break;
      if (latest && choice.kind==='media' && choice.entity.id===latest.id) continue;
      const cls=slots[homeTiles.length];
      homeTiles.push({cls,display:choice.entity.title,target:choice.entity.title,kicker:choice.kind==='series'?'Series':'From your library',img:choice.card.img,kind:choice.kind,mediaId:choice.kind==='media'?choice.entity.id:null,seriesId:choice.kind==='series'?choice.entity.id:null,zoom:1.02,pos:'50% 45%'});
    }

    const featured = random[0] || (latest ? {kind:'media',entity:latest,card:mediaCard(latest)} : null);
    if (featured) {
      heroes.home.image = featured.card.img;
      heroes.watch.image = featured.card.img;
      heroBg.style.backgroundImage = `url("${featured.card.img}")`;
    } else {
      const blank=placeholder('HED Space','landscape');
      heroes.home.image=blank; heroes.watch.image=blank;
    }
  }

  function atmosphereSoundtrackSongs(lib=desktopLibrary) {
    return (lib?.music_collections || [])
      .filter(collection =>
        collection.is_soundtrack ||
        String(collection.title || '').trim().toLowerCase() === 'soundtracks'
      )
      .flatMap(collection => collection.songs || []);
  }

  function updateAtmosphereShuffleUi() {
    const button = document.querySelector('#shuffleAtmosSound');
    if (!button) return;
    button.classList.toggle('active', atmosphereShuffleEnabled);
    button.setAttribute('aria-pressed', String(atmosphereShuffleEnabled));
    button.title = atmosphereShuffleEnabled
      ? 'Soundtrack shuffle on · Soundtracks folder only'
      : 'Shuffle soundtrack songs';
    const label = document.querySelector('#shuffleAtmosLabel');
    if (label) label.textContent = atmosphereShuffleEnabled ? 'Shuffle on' : 'Shuffle';
  }

  function randomAtmosphereSoundtrack(excludeId=null) {
    const songs = atmosphereSoundtrackSongs();
    if (!songs.length) return null;
    if (songs.length === 1) return songs[0];

    const candidates = songs.filter(song => song.id !== Number(excludeId));
    const pool = candidates.length ? candidates : songs;
    return pool[Math.floor(Math.random() * pool.length)] || pool[0];
  }

  function advanceAtmosphereShuffle() {
    if (!atmosphereShuffleEnabled) return;
    const next = randomAtmosphereSoundtrack(currentSoundtrackId);
    if (!next) {
      atmosphereShuffleEnabled = false;
      updateAtmosphereShuffleUi();
      flash('No songs found in the Soundtracks collection');
      return;
    }

    if (currentSoundtrackId != null) {
      atmosphereShuffleHistory.push(currentSoundtrackId);
      if (atmosphereShuffleHistory.length > 80) atmosphereShuffleHistory.shift();
    }
    chooseAtmosphereSound(next.id, { automatic: true });
  }

  function rebuildAtmosphereControls(lib) {
    if (!lib.atmospheres.length) {
      currentAtmosphereId = null;
      $('#roomOptions').innerHTML = '<div class="hed-empty"><div><strong>No atmospheres found</strong>Add videos to HED Space\\Tube and rescan.</div></div>';
      return;
    }
    if (!currentAtmosphereId || !lib.atmospheres.some(x=>x.id===currentAtmosphereId)) currentAtmosphereId = lib.atmospheres[0].id;
    currentRoom = String(currentAtmosphereId);
    $('#roomOptions').innerHTML = lib.atmospheres.map(item => `<button class="room-option${item.id===currentAtmosphereId?' selected':''}" data-room="${item.id}"><img src="${localUrl(item.thumbnail_path||item.poster_path)||placeholder(item.title,'landscape')}" alt=""><span><strong>${htmlEscape(item.title)}</strong><small>${htmlEscape(mediaMeta(item))}</small></span><span>${item.id===currentAtmosphereId?'Inside':'Enter'}</span></button>`).join('');
    document.querySelectorAll('.room-option[data-room]').forEach(o => o.onclick = () => selectRoom(o.dataset.room));
    $('#roomOptions').insertAdjacentHTML('beforeend','<div class="hed-atmos-manage"><button id="hedAtmosEdit">Edit current</button><button id="hedAtmosThumb">Change thumbnail</button></div>');
    document.querySelector('#hedAtmosEdit').onclick=()=>{const item=mediaById.get(currentAtmosphereId);if(item)openEditor('media',item)};
    document.querySelector('#hedAtmosThumb').onclick=()=>{if(currentAtmosphereId)chooseArtwork('media',currentAtmosphereId,'thumbnail')};

    const atmosphereSoundtracks = atmosphereSoundtrackSongs(lib);
    const soundtrackRows = atmosphereSoundtracks.length
      ? atmosphereSoundtracks.map(song => `<button class="sound-option${song.id===currentSoundtrackId?' selected':''}" data-sound-id="${song.id}"><img src="${songCover(song)}" alt=""><span><strong>${htmlEscape(song.title)}</strong><small>${htmlEscape(song.artist||'Soundtracks')}</small></span><span>${song.id===currentSoundtrackId?'Active':'Play'}</span></button>`).join('')
      : '<div class="hed-soundtrack-empty"><strong>No soundtrack songs found</strong><span>Add audio inside a folder named <b>Soundtracks</b> under your Songs library.</span></div>';
    $('#soundPanel .sound-options').innerHTML = `<button class="sound-option${currentSoundtrackId==null?' selected':''}" data-sound-id=""><span class="sound-wave">≈</span><span><strong>Original atmosphere</strong><small>Use the audio inside the atmosphere video</small></span><span>${currentSoundtrackId==null?'Active':'Play'}</span></button>` + soundtrackRows;
    document.querySelectorAll('.sound-option[data-sound-id]').forEach(o => o.onclick = () => chooseAtmosphereSound(o.dataset.soundId));
  }

  function chooseAtmosphereSound(rawId, options={}) {
    pauseListenForNavigation();
    const automatic = Boolean(options.automatic);
    const id = rawId ? Number(rawId) : null;

    // "Original atmosphere" is NEVER part of Shuffle. Selecting it manually
    // explicitly exits soundtrack shuffle and restores the video's own audio.
    if (id == null) {
      atmosphereShuffleEnabled = false;
      atmosphereShuffleHistory = [];
    }

    currentSoundtrackId = id;
    const song = id ? mediaById.get(id) : null;
    currentSound = song?.title || 'Original atmosphere';

    if (song) {
      atmosVideo.muted = true;
      soundtrackAudio.src = localUrl(song.file_path);
      soundtrackAudio.loop = !atmosphereShuffleEnabled;
      soundtrackAudio.play().catch(()=>{});
    } else {
      soundtrackAudio.pause();
      soundtrackAudio.removeAttribute('src');
      soundtrackAudio.loop = false;
      atmosVideo.muted = false;
      atmosVideo.volume = .72;
      atmosVideo.play().catch(()=>{});
    }

    $('#soundStatus').textContent = atmosphereShuffleEnabled && song
      ? `Shuffle · ${currentSound}`
      : currentSound;

    if (!automatic) $('#soundPanel').classList.remove('open');
    updateAtmosphereShuffleUi();
    rebuildAtmosphereControls(desktopLibrary);
  }

  const shuffleAtmosSoundButton = document.querySelector('#shuffleAtmosSound');
  if (shuffleAtmosSoundButton) {
    shuffleAtmosSoundButton.onclick = () => {
      const songs = atmosphereSoundtrackSongs();
      if (!songs.length) {
        flash('Add songs to the Soundtracks folder first');
        return;
      }

      atmosphereShuffleEnabled = !atmosphereShuffleEnabled;
      atmosphereShuffleHistory = [];
      updateAtmosphereShuffleUi();

      if (atmosphereShuffleEnabled) {
        soundtrackAudio.loop = false;
        const next = randomAtmosphereSoundtrack(currentSoundtrackId);
        if (next) chooseAtmosphereSound(next.id, { automatic: true });
        flash('Atmosphere soundtrack shuffle · on');
      } else {
        soundtrackAudio.loop = currentSoundtrackId != null;
        $('#soundStatus').textContent = currentSoundtrackId != null
          ? (mediaById.get(currentSoundtrackId)?.title || 'Soundtrack')
          : 'Original atmosphere';
        flash('Atmosphere soundtrack shuffle · off');
      }
    };
    updateAtmosphereShuffleUi();
  }

  // Endless soundtrack shuffle. The default/original atmosphere audio is not
  // part of this pool; only songs from the physical Soundtracks collection are.
  soundtrackAudio.addEventListener('ended', () => {
    if (atmosphereShuffleEnabled && currentSection === 'atmospheres') {
      advanceAtmosphereShuffle();
    }
  });


  async function loadLibrary(rescan=true) {
    const targetSection = app?.dataset?.section || currentSection || 'home';
    if (rescan) {
      try { await invoke('scan_library'); } catch (e) { console.warn('Library rescan failed', e); }
    }
    desktopLibrary = await invoke('get_library');
    indexLibrary(desktopLibrary);
    replaceStaticData(desktopLibrary);
    if (targetSection === 'atmospheres') applyAtmosphere();
    else renderStandard(targetSection);
  }

  let startupRescanTimer = 0;
  function scheduleBackgroundStartupRescan() {
    clearTimeout(startupRescanTimer);
    startupRescanTimer = window.setTimeout(async () => {
      try {
        console.info('[HED Space] Background library rescan started');
        await invoke('scan_library');

        // Refresh the cached library after the worker finishes. Avoid rebuilding
        // the visible page while the user is inside a modal/player transition.
        if (!document.querySelector('#details')?.classList.contains('open') &&
            !document.querySelector('#seriesPanel')?.classList.contains('open')) {
          await loadLibrary(false);
        } else {
          desktopLibrary = await invoke('get_library');
          indexLibrary(desktopLibrary);
          replaceStaticData(desktopLibrary);
        }
        console.info('[HED Space] Background library rescan finished');
      } catch (error) {
        console.warn('[HED Space] Background library rescan failed', error);
      }
    }, 10000);
  }

  // ---- Replace prototype renderer with real IDs ----
  posterFor = card => card?.poster || card?.img || placeholder(card?.title,'poster');
  cardHTML = card => {
    const zoom=card.zoom||1.04,hover=zoom+.045,poster=posterFor(card)||card.img;
    return `<button class="media-card" style="--zoom:${zoom};--hover-zoom:${hover};--position:${card.position||'50% 50%'}" data-entity="${card._entity||'media'}" data-id="${card._id||''}" data-title="${htmlEscape(card.title)}" data-img="${card.img}" data-poster="${poster}"><img src="${card.img}" alt=""><span class="card-tag">${htmlEscape(card.tag||'HED Space')}</span><span class="card-copy"><strong>${htmlEscape(card.title)}</strong><small>${htmlEscape(card.meta||'')}</small></span>${card.progress?`<span class="progress" style="width:${card.progress}%"></span>`:''}</button>`;
  };

  renderFeed = section => {
    const sectionRows = rows[section] || [];
    if (!sectionRows.length) {
      feed.innerHTML = '<div class="hed-empty"><div><strong>Your local Watch library is empty.</strong>Add files and press the rescan button.</div></div>';
      return;
    }
    feed.innerHTML=sectionRows.map((row,i)=>`<section class="content-row"><div class="row-head"><div class="row-title"><h2>${htmlEscape(row.title)}</h2><span>${htmlEscape(row.subtitle)}</span></div><div class="row-controls"><button data-scroll="-1" data-row="${i}" aria-label="Previous">‹</button><button data-scroll="1" data-row="${i}" aria-label="Next">›</button></div></div><div class="rail" data-rail="${i}">${row.cards.map(cardHTML).join('')}</div></section>`).join('');
    feed.querySelectorAll('.media-card').forEach(card => card.onclick = () => openCard(card.dataset.entity, Number(card.dataset.id)));
    feed.querySelectorAll('[data-scroll]').forEach(btn=>btn.onclick=()=>feed.querySelector(`[data-rail="${btn.dataset.row}"]`).scrollBy({left:Number(btn.dataset.scroll)*720,behavior:motionLevel==='off'?'auto':'smooth'}));
    requestAnimationFrame(centerRails);
  };

  libraryTileHTML = (card,type) => { const playIcon = '<svg class="play-icon-library" viewBox="0 0 20 20" aria-hidden="true"><path d="M6.5 4.8 L14.7 10 L6.5 15.2 Z" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linejoin="round"></path></svg>'; return `<button class="poster-tile" data-entity="${card._entity||'media'}" data-id="${card._id||''}" data-library-type="${type}"><span class="poster-art"><img src="${posterFor(card)}" alt="${htmlEscape(card.title)}"><span class="poster-badge">${htmlEscape(card.tag||type)}</span></span><span class="poster-tile-copy"><strong>${htmlEscape(card.title)}</strong><small>${htmlEscape(card.meta||'')}</small></span><span class="poster-play">${playIcon}</span></button>`; };
  libraryMusicTileHTML = card => { const playIcon = '<svg class="play-icon-library" viewBox="0 0 20 20" aria-hidden="true"><path d="M6.5 4.8 L14.7 10 L6.5 15.2 Z" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linejoin="round"></path></svg>'; return `<button class="library-music-card" data-id="${card._id}"><span class="library-music-art"><img src="${card.img}" alt="${htmlEscape(card.title)}"><span class="library-music-kind">Music</span><span class="library-music-play">${playIcon}</span></span><span class="library-music-copy"><strong>${htmlEscape(card.title)}</strong><small>${htmlEscape(card.meta||'Local music')}</small></span></button>`; };

  let libraryHeroKey = '';
  function libraryHeroCandidates(filter='all') {
    if (!desktopLibrary) return [];
    const candidates = [];
    const add = (entity, kind, path) => {
      const url = localUrl(path);
      if (url) candidates.push({title:entity.title || 'HED Space', kind, url});
    };

    if (filter === 'all' || filter === 'media' || filter === 'movies') {
      desktopLibrary.movies.forEach(item => {
        if (item.backdrop_path) add(item, 'Movie', item.backdrop_path);
      });
    }
    if (filter === 'all' || filter === 'media' || filter === 'series') {
      desktopLibrary.series.filter(item => item.category !== 'childhood').forEach(item => {
        if (item.backdrop_path) add(item, item.category === 'anime' ? 'Anime' : 'Series', item.backdrop_path);
        else {
          const episode = item.seasons?.flatMap(season => season.episodes || []).find(ep => ep.thumbnail_path || ep.backdrop_path);
          if (episode) add(item, item.category === 'anime' ? 'Anime' : 'Series', episode.backdrop_path || episode.thumbnail_path);
        }
      });
    }
    if (filter === 'all' || filter === 'childhood') {
      desktopLibrary.series.filter(item => item.category === 'childhood').forEach(item => {
        if (item.backdrop_path) add(item, 'Childhood', item.backdrop_path);
        else {
          const episode = item.seasons?.flatMap(season => season.episodes || []).find(ep => ep.thumbnail_path || ep.backdrop_path);
          if (episode) add(item, 'Childhood', episode.backdrop_path || episode.thumbnail_path);
        }
      });
    }

    // Album art is usually square and lower resolution. For the Songs filter we deliberately
    // keep using a cinematic video backdrop when one exists rather than stretching a cover.
    if (!candidates.length && filter !== 'all') return libraryHeroCandidates('all');
    return candidates;
  }

  function updateLibraryHero(filter='all') {
    const bg = document.querySelector('#libraryHeroBg');
    if (!bg) return;

    const fixedLibraryHero = standardHero('library');
    if (fixedLibraryHero) {
      libraryHeroKey = 'standard:library';
      bg.style.backgroundImage = `url("${fixedLibraryHero}")`;
      bg.style.opacity = '.78';
      return;
    }

    const candidates = libraryHeroCandidates(filter);
    if (!candidates.length) {
      bg.style.backgroundImage = heroes?.watch?.image ? `url("${heroes.watch.image}")` : '';
      return;
    }
    const nextPool = candidates.length > 1 ? candidates.filter(item => `${item.kind}:${item.title}` !== libraryHeroKey) : candidates;
    const item = nextPool[Math.floor(Math.random() * nextPool.length)] || candidates[0];
    libraryHeroKey = `${item.kind}:${item.title}`;
    bg.style.opacity = '.35';
    window.setTimeout(() => {
      bg.style.backgroundImage = `url("${String(item.url).replaceAll('"','%22')}")`;
      bg.style.opacity = '.78';
    }, 110);
  }

  renderLibrary = (filter='all') => {
    updateLibraryHero(filter);
    const mediaItems=[...(libraryCatalog.movies||[]),...(libraryCatalog.series||[])];
    const childhoodItems=libraryCatalog.childhood||[];
    const groups=[
      ['media','Movies, Series & Anime',mediaItems],
      ['childhood','Childhood',childhoodItems]
    ].filter(([type])=>filter==='all'||filter===type);
    $('#libraryGroups').innerHTML=groups.map(([type,title,items])=>
      `<section class="library-group"><div class="library-group-head"><h2>${title}</h2></div>${items.length?`<div class="poster-grid">${items.map(x=>libraryTileHTML(x,type)).join('')}</div>`:'<div class="hed-empty">Nothing here yet.</div>'}</section>`
    ).join('');
    document.querySelectorAll('.poster-tile[data-id]').forEach(card=>card.onclick=()=>openCard(card.dataset.entity,Number(card.dataset.id)));
  };

  renderHome = () => {
    const root=document.querySelector('#homeMosaic'); if(!root)return;
    if(!homeTiles.length){root.innerHTML='<div class="hed-empty" style="grid-column:1/-1"><div><strong>Your HED Space is ready.</strong>Add movies or series and rescan.</div></div>';return}
    root.innerHTML=homeTiles.map((tile,i)=>`<button class="home-card home-card-${tile.cls}" data-home-index="${i}" style="--home-zoom:${tile.zoom||1.01};--home-pos:${tile.pos||'50% 50%'}"><img src="${tile.img}" alt=""><span class="home-card-copy"><small class="home-card-kicker">${htmlEscape(tile.kicker)}</small><strong class="home-card-title">${htmlEscape(tile.display)}</strong></span></button>`).join('');
    root.querySelectorAll('.home-card').forEach(card=>card.onclick=()=>{const tile=homeTiles[Number(card.dataset.homeIndex)]; if(tile.kind==='series')openSeriesById(tile.seriesId); else openMediaById(tile.mediaId)});
  };

  async function openCard(entity, id) {
    if (entity === 'series') return openSeriesById(id);
    return openMediaById(id);
  }

  function fitDisplayTitle(element, title) {
    if (!element) return;
    const length = Array.from(String(title || '')).length;
    element.classList.toggle('hed-title-long', length > 42);
    element.classList.toggle('hed-title-very-long', length > 72);
  }

  const hedPencilIcon = '<svg class="hed-pencil-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 19.5 5.8 14 15.7 4.1a2.25 2.25 0 0 1 3.2 0l1 1a2.25 2.25 0 0 1 0 3.2L10 18.2 4.5 19.5Z"></path><path d="m13.9 5.9 4.2 4.2"></path></svg>';

  function installInlineTitleEditors() {
    const title = document.querySelector('#detailsTitle');
    if (title && !document.querySelector('#hedInlineEditTitle')) {
      const row = document.createElement('div');
      row.className = 'hed-details-title-row';
      title.parentNode.insertBefore(row, title);
      row.appendChild(title);
      const button = document.createElement('button');
      button.id = 'hedInlineEditTitle';
      button.className = 'hed-inline-edit';
      button.type = 'button';
      button.title = 'Change display title';
      button.setAttribute('aria-label', 'Change display title');
      button.innerHTML = hedPencilIcon;
      button.onclick = () => selectedDesktopEntity && openEditor(selectedDesktopEntity.entityType, selectedDesktopEntity.entity);
      row.appendChild(button);
    }

    const seriesTitle = document.querySelector('#seriesTitle');
    if (seriesTitle && !document.querySelector('#hedInlineEditSeriesTitle')) {
      const row = document.createElement('div');
      row.className = 'hed-series-title-row';
      seriesTitle.parentNode.insertBefore(row, seriesTitle);
      row.appendChild(seriesTitle);
      const button = document.createElement('button');
      button.id = 'hedInlineEditSeriesTitle';
      button.className = 'hed-inline-edit';
      button.type = 'button';
      button.title = 'Change series title';
      button.setAttribute('aria-label', 'Change series title');
      button.innerHTML = hedPencilIcon;
      button.onclick = () => selectedDesktopEntity && openEditor('series', selectedDesktopEntity.entity);
      row.appendChild(button);
    }
  }

  function installDetailsActions() {
    installInlineTitleEditors();
    const actions = document.querySelector('.details-actions');
    if (actions && !document.querySelector('#hedDetailsExtra')) {
      const extra=document.createElement('div'); extra.id='hedDetailsExtra'; extra.className='hed-details-extra';
      extra.innerHTML='<button class="hed-ghost-button" id="hedEditDetails">Edit details</button><button class="hed-ghost-button" id="hedChangePoster">Change poster</button><button class="hed-ghost-button" id="hedChangeBackdrop">Change backdrop</button>';
      actions.parentElement.append(extra);
      document.querySelector('#hedEditDetails').onclick=()=>selectedDesktopEntity&&openEditor(selectedDesktopEntity.entityType,selectedDesktopEntity.entity);
      document.querySelector('#hedChangePoster').onclick=()=>selectedDesktopEntity&&chooseArtwork(selectedDesktopEntity.entityType,selectedDesktopEntity.entity.id, selectedDesktopEntity.entity.kind==='song'?'cover':['episode','atmosphere'].includes(selectedDesktopEntity.entity.kind)?'thumbnail':'poster');
      document.querySelector('#hedChangeBackdrop').onclick=()=>selectedDesktopEntity&&chooseArtwork(selectedDesktopEntity.entityType,selectedDesktopEntity.entity.id,'backdrop');
    }
    const panel=document.querySelector('#seriesPanel');
    if(panel&&!document.querySelector('#hedSeriesActions')){
      const extra=document.createElement('div'); extra.id='hedSeriesActions'; extra.className='hed-details-extra';
      extra.innerHTML='<button class="hed-ghost-button" id="hedSeriesEdit">Edit series</button><button class="hed-ghost-button" id="hedSeriesPoster">Change poster</button><button class="hed-ghost-button" id="hedSeriesBackdrop">Change backdrop</button>';
      panel.querySelector('.series-intro')?.after(extra);
      document.querySelector('#hedSeriesEdit').onclick=()=>selectedDesktopEntity&&openEditor('series',selectedDesktopEntity.entity);
      document.querySelector('#hedSeriesPoster').onclick=()=>selectedDesktopEntity&&chooseArtwork('series',selectedDesktopEntity.entity.id,'poster');
      document.querySelector('#hedSeriesBackdrop').onclick=()=>selectedDesktopEntity&&chooseArtwork('series',selectedDesktopEntity.entity.id,'backdrop');
    }
  }

  let detailsPreviewToken = 0;

  function stopDetailsPreview() {
    detailsPreviewToken += 1;
    const stage = document.querySelector('#detailsBackdrop');
    const video = stage?.querySelector('video');
    if (video) {
      try { video.pause(); } catch (_) {}
      video.removeAttribute('src');
      try { video.load(); } catch (_) {}
      video.remove();
    }
  }

  function previewStartTime(item) {
    const duration = Math.max(0, Number(item?.duration) || 0);
    if (!duration) return 0;
    const seed = (Math.abs(Number(item?.id) || 0) % 37) / 37;
    const target = duration * (0.20 + seed * 0.34);
    return Math.max(0, Math.min(target, Math.max(0, duration - 16)));
  }

  function mountDetailsVideo(src, item, clipIsCached = false) {
    const stage = document.querySelector('#detailsBackdrop');
    if (!stage || !src) return null;

    stage.querySelector('video')?.remove();
    const video = document.createElement('video');
    video.className = 'details-preview-video';
    video.muted = true;
    video.autoplay = true;
    video.loop = clipIsCached;
    video.playsInline = true;
    video.preload = 'auto';
    video.setAttribute('aria-hidden', 'true');
    stage.prepend(video);

    if (!clipIsCached) {
      const startAt = previewStartTime(item);
      const stopAt = startAt + 10;
      video.addEventListener('loadedmetadata', () => {
        try { video.currentTime = startAt; } catch (_) {}
      }, { once: true });
      video.addEventListener('timeupdate', () => {
        if (video.currentTime >= stopAt) {
          try { video.currentTime = startAt; video.play().catch(() => {}); } catch (_) {}
        }
      });
    }

    video.addEventListener('playing', () => {
      video.classList.add('is-playing');
    }, { once: true });

    video.src = src;
    video.play().catch(() => {});
    return video;
  }

  async function startDetailsPreview(item) {
    stopDetailsPreview();
    const token = detailsPreviewToken;
    if (!item || !['movie', 'episode', 'childhood'].includes(item.kind)) return;

    // Browser-compatible local files can play immediately with no generated cache.
    const ext = String(item.file_path || '').split('.').pop().toLowerCase();
    const directCandidate = ['mp4', 'm4v', 'mov', 'webm'].includes(ext);

    if (directCandidate) {
      const directVideo = mountDetailsVideo(localUrl(item.file_path), item, false);
      if (directVideo) {
        const directWorked = await new Promise(resolve => {
          let done = false;
          const finish = ok => { if (done) return; done = true; clearTimeout(timer); resolve(ok); };
          const timer = window.setTimeout(() => finish(false), 1500);
          directVideo.addEventListener('playing', () => finish(true), { once: true });
          directVideo.addEventListener('error', () => finish(false), { once: true });
        });
        if (token !== detailsPreviewToken) return;
        if (directWorked) return;
        stopDetailsPreview();
      }
    }

    // MKV/HEVC/etc. may not be WebView-decodable. Generate a small cached H.264
    // scene with ffmpeg. This is ONLY for the blurred details background.
    try {
      const path = await invoke('prepare_details_preview', { mediaId: item.id });
      if (token !== detailsPreviewToken || !path || !document.querySelector('#details')?.classList.contains('open')) return;
      mountDetailsVideo(localUrl(path), item, true);
    } catch (error) {
      console.warn('[HED Space] Could not prepare details motion preview', error);
    }
  }

  function openMediaById(id) {
    const item=mediaById.get(Number(id)); if(!item)return;
    selectedDesktopEntity={entityType:'media',entity:item};
    $('#detailsTitle').textContent=item.title; fitDisplayTitle($('#detailsTitle'), item.title);
    $('#detailsMeta').textContent=mediaMeta(item);
    $('#detailsDescription').textContent=item.description||`A local HED Space selection from ${item.series_title||item.kind}.`;
    $('#detailsPoster').src=item.kind==='song'?songCover(item):mediaPoster(item);
    $('#details').classList.add('open');
    stopDetailsPreview();
    const detailStage = $('#detailsBackdrop');
    detailStage.style.backgroundImage = 'none';
    detailStage.style.setProperty('--detail-backdrop', `url("${String(mediaLandscape(item)).replaceAll('"','%22')}")`);
    $('#detailsBackdrop').style.backgroundSize='cover'; $('#detailsBackdrop').style.backgroundPosition='center';
    const changePoster=document.querySelector('#hedChangePoster');
    if(changePoster) changePoster.textContent=item.kind==='song'?'Change cover':['episode','atmosphere'].includes(item.kind)?'Change thumbnail':'Change poster';
    const backdrop=document.querySelector('#hedChangeBackdrop'); if(backdrop) backdrop.style.display=['song','episode'].includes(item.kind)?'none':'';
    $('#detailsPlay').textContent=item.kind==='song'?'▶  Listen':item.kind==='atmosphere'?'Enter atmosphere':'▶  Play';
    startDetailsPreview(item);
  }

  // Stop the motion preview cleanly when the details overlay closes.
  closeDetails = () => {
    stopDetailsPreview();
    document.querySelector('#details')?.classList.remove('open');
    selectedMedia = null;
  };

  openDetails = (title,img,poster,card) => {
    const id=Number(card?.dataset?.id||0); if(id)return openMediaById(id);
    const item=titleToMedia.get(title); if(item)return openMediaById(item.id);
  };

  function openSeriesById(id, preferredSeasonIndex=0) {
    const series=seriesById.get(Number(id)); if(!series)return;
    selectedDesktopEntity={entityType:'series',entity:series};
    $('#seriesPoster').src=seriesPoster(series); $('#seriesTitle').textContent=series.title; fitDisplayTitle($('#seriesTitle'), series.title);
    const count=series.seasons.reduce((n,s)=>n+s.episodes.length,0);
    const seasonCount=series.seasons.length;
    const kicker=series.series_key?.startsWith('anime::')?'Anime collection':series.series_key?.startsWith('childhood::')?'Childhood collection':'Series collection';
    const kickerEl=document.querySelector('#seriesPanel .series-kicker'); if(kickerEl)kickerEl.textContent=kicker;
    $('#seriesMeta').textContent=`${seasonCount} ${seasonCount===1?'season':'seasons'} · ${count} ${count===1?'episode':'episodes'}`;
    $('#seriesIntro').textContent=series.description||'Choose a season, then an episode.';
    const firstEpisode=series.seasons.flatMap(s=>s.episodes)[0];
    const drawerBackdrop=localUrl(series.backdrop_path)||localUrl(firstEpisode?.thumbnail_path)||seriesLandscape(series);
    $('#seriesPanel').style.setProperty('--series-backdrop',`url("${drawerBackdrop}")`);
    $('#seriesPanel').classList.add('open');
    const safeSeasonIndex=Math.max(0,Math.min(Number(preferredSeasonIndex)||0,Math.max(0,series.seasons.length-1)));
    $('#seasonList').innerHTML=series.seasons.map((s,i)=>`<button data-season="${i}" class="${i===safeSeasonIndex?'active':''}"><span>Season ${s.season_number}</span><small>${s.episodes.length} ${s.episodes.length===1?'episode':'episodes'}</small></button>`).join('');
    const select=i=>{
      document.querySelectorAll('#seasonList button').forEach((b,n)=>b.classList.toggle('active',n===i));
      const season=series.seasons[i];
      $('#episodeList').innerHTML=season.episodes.map(ep=>`<article class="hed-episode-row" data-episode-row="${ep.id}"><button class="hed-episode-main" data-episode-id="${ep.id}" aria-label="Play ${htmlEscape(ep.title)}"><div class="hed-episode-thumb"><img src="${localUrl(ep.thumbnail_path)||mediaLandscape(ep)}" alt=""><span>E${String(ep.episode_number??1).padStart(2,'0')}</span></div><span class="hed-episode-copy"><strong>${htmlEscape(ep.title)}</strong><small>${htmlEscape(mediaMeta(ep))}</small></span><em aria-hidden="true">▶</em></button><button class="hed-episode-art-button" data-episode-art="${ep.id}" title="Change episode image" aria-label="Change image for ${htmlEscape(ep.title)}">${hedPencilIcon}</button></article>`).join('');
      document.querySelectorAll('.hed-episode-main[data-episode-id]').forEach(button=>button.onclick=()=>{closeSeries();openMediaById(Number(button.dataset.episodeId))});
      document.querySelectorAll('[data-episode-art]').forEach(button=>button.onclick=e=>{e.stopPropagation();chooseEpisodeArtwork(Number(button.dataset.episodeArt),series.id,i)});
    };
    document.querySelectorAll('#seasonList button').forEach((b,i)=>b.onclick=()=>select(i)); if(series.seasons.length)select(safeSeasonIndex);
  }
  openSeries = title => { const s=desktopLibrary?.series.find(x=>x.title===title); if(s)openSeriesById(s.id); };

  $('#detailsPlay').onclick = async () => {
    if(!selectedDesktopEntity)return;
    const entity=selectedDesktopEntity.entity;
    if(selectedDesktopEntity.entityType==='series')return;
    closeDetails();
    if(entity.kind==='song'){setActiveNav('listen');navigate('listen');setTimeout(()=>setListenNowById(entity.id,true),300);return}
    if(entity.kind==='atmosphere'){setActiveNav('atmospheres');navigate('atmospheres');setTimeout(()=>selectRoom(String(entity.id)),300);return}
    try{await invoke('open_player',{mediaId:entity.id});}catch(error){flash(String(error));}
  };

  $('#heroPlay').onclick = async () => {
    if(currentSection==='home'){
      const latest=desktopLibrary?.continue_watching?.[0];
      if(latest){try{await invoke('open_player',{mediaId:latest.id});}catch(e){flash(String(e))};return}
      const first=desktopLibrary?.movies?.[0]; if(first){try{await invoke('open_player',{mediaId:first.id});}catch(e){flash(String(e))}return}
      flash('Add a movie or series first'); return;
    }
    if(currentSection==='watch'){
      const first=desktopLibrary?.continue_watching?.[0]||desktopLibrary?.movies?.[0];
      if(first){try{await invoke('open_player',{mediaId:first.id});}catch(e){flash(String(e))}}
    }
  };

  const LISTEN_VOLUME_KEY = 'hed-space-listen-volume';
  let listenVolumeBeforeMute = .72;

  function updateListenVolumeUi() {
    const slider = document.querySelector('#listenVolume');
    const value = Math.round((Number.isFinite(listenAudio.volume) ? listenAudio.volume : .72) * 100);
    if (slider) {
      slider.value = String(value);
      slider.style.setProperty('--listen-volume-pct', `${value}%`);
    }
    const label = document.querySelector('#listenVolumeValue');
    if (label) label.textContent = String(value);
    const mute = document.querySelector('#listenVolumeMute');
    if (mute) {
      mute.textContent = value === 0 ? '🔇' : value < 45 ? '🔉' : '🔊';
      mute.setAttribute('aria-label', value === 0 ? 'Unmute music' : 'Mute music');
    }
  }

  function setListenVolume(value, persist = true) {
    const volume = Math.max(0, Math.min(1, Number(value) || 0));
    listenAudio.volume = volume;
    listenAudio.muted = false;
    if (volume > 0) listenVolumeBeforeMute = volume;
    if (persist) {
      try { localStorage.setItem(LISTEN_VOLUME_KEY, String(volume)); } catch (_) {}
    }
    updateListenVolumeUi();
  }

  function installListenVolume() {
    let saved = .72;
    try {
      const stored = Number(localStorage.getItem(LISTEN_VOLUME_KEY));
      if (Number.isFinite(stored)) saved = Math.max(0, Math.min(1, stored));
    } catch (_) {}
    setListenVolume(saved, false);

    const slider = document.querySelector('#listenVolume');
    if (slider) {
      slider.oninput = () => setListenVolume(Number(slider.value) / 100);
    }

    const mute = document.querySelector('#listenVolumeMute');
    if (mute) {
      mute.onclick = () => {
        if (listenAudio.volume > 0.001) {
          listenVolumeBeforeMute = listenAudio.volume;
          setListenVolume(0);
        } else {
          setListenVolume(listenVolumeBeforeMute > 0.001 ? listenVolumeBeforeMute : .72);
        }
      };
    }
  }

  function pauseListenForNavigation() {
    if (!listenAudio.paused) listenAudio.pause();
    listenIsPlaying = false;
    setListenPlayButtonState(false);
  }

  function pauseAtmosphereForNavigation() {
    try { atmosVideo.pause(); } catch (_) {}
    try { soundtrackAudio.pause(); } catch (_) {}
  }

  // The inline navigation code lives in index.html; expose tiny bridge helpers
  // so it can enforce one audio source at a time.
  window.hedPauseListenForNavigation = pauseListenForNavigation;
  window.hedPauseAtmosphereForNavigation = pauseAtmosphereForNavigation;

  // ---- Real local music collections ----
  function currentMusicCollection() {
    return activeMusicCollectionId ? (musicCollectionById.get(Number(activeMusicCollectionId)) || null) : null;
  }

  function currentListenSongs() {
    const selected = currentMusicCollection();
    // Normally Listen is always scoped to a physical folder collection.
    // Fall back to all songs only for legacy libraries that have no collections yet.
    return selected ? (selected.songs || []) : (desktopLibrary?.music_collections?.length ? [] : (desktopLibrary?.songs || []));
  }

  function setListenNowById(id, autoPlay=false) {
    const song=mediaById.get(Number(id)); if(!song||song.kind!=='song')return;
    if (autoPlay) pauseAtmosphereForNavigation();
    selectedSongId=song.id;
    const visibleSongs=currentListenSongs();
    listenCurrentIndex=Math.max(0,visibleSongs.findIndex(x=>x.id===song.id));
    $('#listenNowTitle').textContent=song.title;
    $('#listenNowArtist').textContent=song.artist||musicCollectionForSong(song)?.title||song.album||'Local audio';
    $('#listenNowArt').src=songCover(song);
    document.querySelectorAll('[data-song-id]').forEach(el=>el.classList.toggle('is-current',Number(el.dataset.songId)===song.id));
    const src=localUrl(song.file_path); if(listenAudio.src!==src){listenAudio.src=src;listenAudio.load()}
    listenAudio.loop = listenRepeatOneEnabled;
    if(autoPlay)listenAudio.play().then(()=>{listenIsPlaying=true;setListenPlayButtonState(true)}).catch(()=>{});
  }

  setListenNow = (title,autoPlay=false) => {
    const song=desktopLibrary?.songs.find(x=>x.title===title);
    if(song)setListenNowById(song.id,autoPlay);
  };

  allListenTracks = () => currentListenSongs().map(songCard);

  async function chooseMusicCollectionArtwork(id) {
    const selected = await invoke('pick_image');
    if (!selected) return;
    try {
      await invoke('set_artwork', {
        entityType:'music_collection',
        id:Number(id),
        artKind:'poster',
        sourcePath:selected
      });
      await loadLibrary(false);
      activeMusicCollectionId = Number(id);
      renderListen();
      flash('Collection poster updated · inherited by its songs');
    } catch (error) {
      flash(String(error));
    }
  }

  function renderMusicCollections() {
    const collections = desktopLibrary?.music_collections || [];

    const collectionCards = collections.map(collection => `
      <div class="listen-collection-card${Number(activeMusicCollectionId)===collection.id?' active':''}">
        <button class="listen-collection-open" data-music-collection="${collection.id}">
          <span class="listen-collection-art"><img src="${musicCollectionCover(collection)}" alt=""></span>
          <span class="listen-collection-copy">
            <strong>${htmlEscape(collection.title)}</strong>
            <small>${collection.songs.length} track${collection.songs.length===1?'':'s'}${collection.is_soundtrack?' · Atmosphere soundtracks':''}</small>
          </span>
        </button>
        <button class="listen-collection-edit" data-music-collection-art="${collection.id}" title="Change collection poster" aria-label="Change ${htmlEscape(collection.title)} poster">${hedPencilIcon}</button>
      </div>`).join('');

    $('#listenQuick').innerHTML = collectionCards || '<div class="hed-empty"><div><strong>No music collections yet.</strong>Put songs inside folders such as Songs, Soundtracks, or Spacetoon, then rescan.</div></div>';

    document.querySelectorAll('[data-music-collection]').forEach(button => {
      button.onclick = () => {
        activeMusicCollectionId = Number(button.dataset.musicCollection);
        selectedSongId = null;
        listenShuffleHistory = [];
        renderListen();
      };
    });

    document.querySelectorAll('[data-music-collection-art]').forEach(button => {
      button.onclick = event => {
        event.stopPropagation();
        chooseMusicCollectionArtwork(Number(button.dataset.musicCollectionArt));
      };
    });
  }

  renderListen = () => {
    const selectedCollection = currentMusicCollection();
    const songs = currentListenSongs();
    const recent=songs.slice(0,6), mixes=shuffle(songs).slice(0,6);

    renderMusicCollections();

    const recentHeading=document.querySelector('#listenRecentHeading');
    const recentSub=document.querySelector('#listenRecentSub');
    const mixesHeading=document.querySelector('#listenMixesHeading');
    const mixesSub=document.querySelector('#listenMixesSub');
    const trackHeading=document.querySelector('#listenTrackHeading');
    const trackSub=document.querySelector('#listenTrackSubheading');

    if(recentHeading) recentHeading.textContent=selectedCollection ? `${selectedCollection.title} · highlights` : 'Recently played';
    if(recentSub) recentSub.textContent=selectedCollection ? `${songs.length} tracks in this collection` : 'Your rotation';
    if(mixesHeading) mixesHeading.textContent=selectedCollection ? 'Shuffle this collection' : 'Made for your collection';
    if(mixesSub) mixesSub.textContent=selectedCollection ? 'A different order every time' : 'Local mixes';
    if(trackHeading) trackHeading.textContent=selectedCollection ? selectedCollection.title : 'All tracks';
    if(trackSub) trackSub.textContent=selectedCollection ? `${songs.length} local tracks` : `${songs.length} local audio files`;

    const album=x=>`<button class="listen-album" data-song-id="${x.id}"><span class="listen-album-art"><img src="${songCover(x)}" alt=""><span class="listen-album-play">▶</span></span><strong>${htmlEscape(x.title)}</strong><small>${htmlEscape(x.artist||musicCollectionForSong(x)?.title||x.album||'Local audio')}</small></button>`;
    $('#listenRecent').innerHTML=recent.map(album).join('');
    $('#listenMixes').innerHTML=mixes.map(album).join('');

    $('#listenTrackList').innerHTML=songs.length
      ? songs.map((x,i)=>`<button class="listen-track" data-song-id="${x.id}"><span class="listen-track-index">${i+1}</span><img src="${songCover(x)}" alt=""><span class="listen-track-main"><strong>${htmlEscape(x.title)}</strong><small>${htmlEscape(x.artist||musicCollectionForSong(x)?.title||'Local audio')}</small></span><span class="listen-track-album">${htmlEscape(musicCollectionForSong(x)?.title||x.album||'Local library')}</span><span class="listen-track-time">${durationText(x.duration)||'—'}</span></button>`).join('')
      : '<div class="hed-empty"><div><strong>No tracks in this collection.</strong>Rescan after adding audio files.</div></div>';

    const side=document.querySelector('.listen-side-panel');
    if(side){
      if(songs.length){
        const pick=shuffle(songs)[0];
        side.style.display='';
        const img=side.querySelector('.listen-side-feature img'); if(img)img.src=songCover(pick);
        const small=side.querySelector('.listen-side-feature-copy small'); if(small)small.textContent=selectedCollection?.title||pick.artist||'From your library';
        const strong=side.querySelector('.listen-side-feature-copy strong'); if(strong)strong.textContent=pick.title;
        const collectionNames=(desktopLibrary?.music_collections||[]).map(x=>x.title).slice(0,6);
        const chips=side.querySelector('.listen-chip-row'); if(chips)chips.innerHTML=(collectionNames.length?collectionNames:['Local music']).map(x=>`<span class="listen-chip">${htmlEscape(x)}</span>`).join('');
      }else side.style.display='none';
    }

    document.querySelectorAll('[data-song-id]').forEach(el=>el.onclick=()=>setListenNowById(Number(el.dataset.songId),true));

    if(songs.length){
      const currentVisible = selectedSongId && songs.some(song=>song.id===selectedSongId);
      setListenNowById(currentVisible?selectedSongId:songs[0].id,false);
    } else {
      $('#listenNowTitle').textContent='No songs here';
      $('#listenNowArtist').textContent=selectedCollection?.title||'Add a music folder';
      $('#listenNowArt').src=selectedCollection?musicCollectionCover(selectedCollection):placeholder('Music','square');
    }
  };

  function updateListenShuffleUi() {
    const button = document.querySelector('#listenShuffle');
    if (!button) return;
    button.classList.toggle('active', listenShuffleEnabled);
    button.setAttribute('aria-pressed', String(listenShuffleEnabled));
    button.title = listenShuffleEnabled
      ? 'Shuffle on · random tracks stay inside this collection'
      : 'Shuffle off · play this collection in order';
  }

  function updateListenRepeatUi() {
    const button = document.querySelector('#listenRepeat');
    if (!button) return;
    button.classList.toggle('active', listenRepeatOneEnabled);
    button.setAttribute('aria-pressed', String(listenRepeatOneEnabled));
    button.title = listenRepeatOneEnabled
      ? 'Repeat current song · on'
      : 'Repeat current song';
  }

  function randomListenIndex(songs, currentIndex) {
    if (songs.length <= 1) return 0;
    let next = currentIndex;
    for (let attempts = 0; attempts < 8 && next === currentIndex; attempts += 1) {
      next = Math.floor(Math.random() * songs.length);
    }
    if (next === currentIndex) next = (currentIndex + 1) % songs.length;
    return next;
  }

  function playListenAtIndex(index, autoPlay=true, remember=true) {
    const songs = currentListenSongs();
    if (!songs.length) return;

    const normalized = ((Number(index) || 0) % songs.length + songs.length) % songs.length;
    if (remember && selectedSongId) {
      listenShuffleHistory.push(selectedSongId);
      if (listenShuffleHistory.length > 80) listenShuffleHistory.shift();
    }
    listenCurrentIndex = normalized;
    setListenNowById(songs[normalized].id, autoPlay);
  }

  function advanceListenTrack(direction=1) {
    const songs = currentListenSongs();
    if (!songs.length) return;

    const currentIndex = Math.max(0, songs.findIndex(song => song.id === selectedSongId));

    if (direction < 0 && listenShuffleEnabled && listenShuffleHistory.length) {
      const previousId = listenShuffleHistory.pop();
      const previousIndex = songs.findIndex(song => song.id === previousId);
      if (previousIndex >= 0) {
        playListenAtIndex(previousIndex, true, false);
        return;
      }
    }

    const nextIndex = listenShuffleEnabled
      ? randomListenIndex(songs, currentIndex)
      : (currentIndex + direction + songs.length) % songs.length;

    playListenAtIndex(nextIndex, true, direction > 0);
  }

  $('#listenPlayPause').onclick=()=>{
    const song=mediaById.get(selectedSongId);
    if(!song)return;
    if(listenIsPlaying){listenAudio.pause();listenIsPlaying=false;setListenPlayButtonState(false)}
    else{setListenNowById(song.id,true)}
  };

  $('#listenPrev').onclick=()=>advanceListenTrack(-1);
  $('#listenNext').onclick=()=>advanceListenTrack(1);

  const listenShuffleButton = document.querySelector('#listenShuffle');
  if (listenShuffleButton) {
    listenShuffleButton.onclick = () => {
      listenShuffleEnabled = !listenShuffleEnabled;
      listenShuffleHistory = [];
      updateListenShuffleUi();
      flash(listenShuffleEnabled
        ? `Shuffle on · ${currentMusicCollection()?.title || 'current collection'}`
        : 'Shuffle off');
    };
    updateListenShuffleUi();
  }

  const listenRepeatButton = document.querySelector('#listenRepeat');
  if (listenRepeatButton) {
    listenRepeatButton.onclick = () => {
      listenRepeatOneEnabled = !listenRepeatOneEnabled;
      listenAudio.loop = listenRepeatOneEnabled;
      updateListenRepeatUi();
      flash(listenRepeatOneEnabled ? 'Repeat current song · on' : 'Repeat current song · off');
    };
    listenAudio.loop = listenRepeatOneEnabled;
    updateListenRepeatUi();
  }

  // Keep playback continuous. Repeat-current takes priority. Otherwise,
  // Shuffle chooses another track from this collection, or normal playback
  // advances in order.
  listenAudio.addEventListener('ended',()=>{
    if (listenRepeatOneEnabled) {
      listenAudio.currentTime = 0;
      listenAudio.play().then(()=>{
        listenIsPlaying = true;
        setListenPlayButtonState(true);
      }).catch(()=>{});
      return;
    }
    advanceListenTrack(1);
  });

  listenAudio.addEventListener('timeupdate',()=>{
    if(!listenAudio.duration)return;
    const pct=listenAudio.currentTime/listenAudio.duration*100;
    const fill=document.querySelector('.listen-progress-fill');if(fill)fill.style.width=pct+'%';
    const labels=document.querySelectorAll('.listen-progress-row > span');
    if(labels[0])labels[0].textContent=formatTime(listenAudio.currentTime);
    if(labels[1])labels[1].textContent=formatTime(listenAudio.duration)
  });

  function installListenActions(){
    const now=document.querySelector('.listen-now-copy'); if(!now||document.querySelector('.hed-listen-actions'))return;
    const actions=document.createElement('div');actions.className='hed-listen-actions';actions.innerHTML='<button id="hedEditSong">Edit track</button><button id="hedSongCover">Change this song cover</button>';now.after(actions);
    document.querySelector('#hedEditSong').onclick=()=>{const song=mediaById.get(selectedSongId);if(song)openEditor('media',song)};
    document.querySelector('#hedSongCover').onclick=()=>{if(selectedSongId)chooseArtwork('media',selectedSongId,'cover')};
  }

  // ---- Atmospheres: auto-start first local Tube video; mpv owns pure fullscreen. ----
  // Never expose the room thumbnail while the embedded video is changing/loading.
  // We keep the surface black until WebView2 has decoded a real frame.
  const atmosViewEl = document.querySelector('.atmos-view');
  const setAtmosVideoReady = ready => {
    atmosVideo.classList.toggle('hed-atmos-video-loading', !ready);
    atmosViewEl?.classList.toggle('hed-atmos-video-ready', ready);
  };
  atmosVideo.addEventListener('loadstart', () => setAtmosVideoReady(false));
  atmosVideo.addEventListener('emptied', () => setAtmosVideoReady(false));
  atmosVideo.addEventListener('loadeddata', () => requestAnimationFrame(() => setAtmosVideoReady(true)));
  atmosVideo.addEventListener('playing', () => setAtmosVideoReady(true));

  selectRoom = rawId => {
    const id=Number(rawId); const item=mediaById.get(id); if(!item||item.kind!=='atmosphere')return;
    currentAtmosphereId=id; currentRoom=String(id);
    setAtmosVideoReady(false);
    atmosVideo.removeAttribute('poster');
    atmosVideo.poster='';
    atmosVideo.src=localUrl(item.file_path); atmosVideo.loop=true; atmosVideo.load(); atmosVideo.muted=currentSoundtrackId!=null;
    atmosVideo.play().catch(()=>{atmosVideo.muted=true;atmosVideo.play().catch(()=>{})});
    const roomTitleEl=$('#roomTitle'); if(roomTitleEl) roomTitleEl.textContent=item.title; $('#roomPanel').classList.remove('open'); rebuildAtmosphereControls(desktopLibrary);
  };
  applyAtmosphere = () => {
    setTheme('atmospheres'); currentSection='atmospheres'; app.className='app atmospheres'; app.dataset.section='atmospheres'; document.body.classList.add('immersive'); window.scrollTo({top:0,behavior:'auto'});
    if(desktopLibrary?.atmospheres?.length){selectRoom(String(currentAtmosphereId||desktopLibrary.atmospheres[0].id))}
  };
  let atmosphereFullscreenLaunching = false;
  let atmosphereFullscreenMonitor = 0;

  function resumeEmbeddedAtmosphereAfterFullscreen() {
    if (currentSection !== 'atmospheres') return;

    if (currentSoundtrackId != null) {
      atmosVideo.muted = true;
      atmosVideo.play().catch(() => {});
      soundtrackAudio.loop = !atmosphereShuffleEnabled;
      if (soundtrackAudio.src) {
        soundtrackAudio.play().catch(() => {});
      }
    } else {
      soundtrackAudio.pause();
      atmosVideo.muted = false;
      atmosVideo.volume = .72;
      atmosVideo.play().catch(() => {});
    }
  }

  function monitorAtmosphereFullscreenExit() {
    clearTimeout(atmosphereFullscreenMonitor);
    const check = async () => {
      try {
        const active = await invoke('atmosphere_fullscreen_active');
        if (active) {
          atmosphereFullscreenMonitor = window.setTimeout(check, 450);
          return;
        }
      } catch (error) {
        console.warn('[HED Space] Could not check Atmospheres fullscreen state', error);
      }

      atmosphereFullscreenLaunching = false;
      const button = document.querySelector('#fullscreenAtmos');
      if (button) button.disabled = false;
      resumeEmbeddedAtmosphereAfterFullscreen();
    };
    atmosphereFullscreenMonitor = window.setTimeout(check, 450);
  }

  $('#fullscreenAtmos').onclick=async()=>{
    if(!currentAtmosphereId){flash('Add a video to Tube first');return}
    if(atmosphereFullscreenLaunching)return;

    atmosphereFullscreenLaunching = true;
    const button = document.querySelector('#fullscreenAtmos');
    if (button) button.disabled = true;

    // IMPORTANT: the fullscreen mpv session owns Atmospheres audio while it is
    // open. Pause the WebView video + soundtrack FIRST so the selected song is
    // never heard twice (browser audio + mpv audio).
    try { atmosVideo.pause(); } catch (_) {}
    try { soundtrackAudio.pause(); } catch (_) {}
    pauseListenForNavigation();

    try {
      const fullscreenSoundtrackIds = atmosphereShuffleEnabled
        ? atmosphereSoundtrackSongs().map(song => song.id)
        : (currentSoundtrackId != null ? [currentSoundtrackId] : []);

      await invoke('open_atmosphere_fullscreen',{
        atmosphereId:currentAtmosphereId,
        soundtrackId:currentSoundtrackId,
        soundtrackIds:fullscreenSoundtrackIds,
        shuffleSoundtracks:atmosphereShuffleEnabled
      });
      monitorAtmosphereFullscreenExit();
    } catch(error) {
      atmosphereFullscreenLaunching = false;
      if (button) button.disabled = false;
      resumeEmbeddedAtmosphereAfterFullscreen();
      flash(String(error));
    }
  };
  $('#fullscreenLabel').textContent='Fullscreen';

  // Search should only use real library.
  const desktopSearchItems = () => [
    ...libraryCatalog.movies.map(x=>({...x,kind:'Movies'})),
    ...libraryCatalog.series.map(x=>({...x,kind:'Series & Anime'})),
    ...libraryCatalog.childhood.map(x=>({...x,kind:'Childhood'})),
    ...libraryCatalog.music.map(x=>({...x,kind:'Songs'})),
    ...roomData.map(x=>({title:x.title,img:x.poster,meta:x.meta,tag:'Atmospheres',kind:'Atmospheres',_entity:'media',_id:x.mediaId}))
  ];
  renderSearch = (query='') => {
    const q=query.trim().toLowerCase();
    const results=desktopSearchItems()
      .filter(x=>!q||`${x.title} ${x.meta||''} ${x.tag||''} ${x.kind}`.toLowerCase().includes(q))
      .slice(0,80);
    const groups=[...new Set(results.map(x=>x.kind))];

    $('#searchResults').innerHTML=results.length
      ? groups.map(kind=>{
          const items=results.filter(x=>x.kind===kind);

          if(kind==='Songs'){
            return `<section class="search-group search-music-group">
              <h2>${kind}<span>${items.length} results</span></h2>
              <div class="search-music-grid">
                ${items.map(x=>`
                  <button class="search-music-card" data-song-search-id="${x._id||''}">
                    <span class="search-music-art">
                      <img src="${x.img}" alt="${htmlEscape(x.title)}">
                      <span class="search-music-kind">Music</span>
                      <span class="search-music-play">▶</span>
                    </span>
                    <span class="search-music-copy">
                      <strong>${htmlEscape(x.title)}</strong>
                      <small>${htmlEscape(x.meta||'Local music')}</small>
                    </span>
                  </button>`).join('')}
              </div>
            </section>`;
          }

          return `<section class="search-group">
            <h2>${kind}<span>${items.length} results</span></h2>
            <div class="search-result-grid">
              ${items.map(x=>`<button class="search-result" data-entity="${x._entity||'media'}" data-id="${x._id||''}"><img src="${posterFor(x)||x.img}" alt=""><strong>${htmlEscape(x.title)}</strong><small>${htmlEscape(x.meta||x.tag||'')}</small></button>`).join('')}
            </div>
          </section>`;
        }).join('')
      : '<div class="search-empty">No matching titles in your local space.</div>';

    document.querySelectorAll('.search-result[data-id]').forEach(btn=>btn.onclick=()=>{
      closeSearch();
      openCard(btn.dataset.entity,Number(btn.dataset.id));
    });

    document.querySelectorAll('.search-music-card[data-song-search-id]').forEach(btn=>btn.onclick=()=>{
      const id=Number(btn.dataset.songSearchId);
      const song=mediaById.get(id);
      const collection=song ? musicCollectionForSong(song) : null;
      if(collection) activeMusicCollectionId=collection.id;
      listenShuffleHistory=[];
      closeSearch();
      setActiveNav('listen');
      navigate('listen');
      setTimeout(()=>{renderListen();setListenNowById(id,false)},motionLevel==='full'?560:160);
    });
  };

  let focusRefreshTimer=null;
  window.addEventListener('focus',()=>{
    clearTimeout(focusRefreshTimer);
    focusRefreshTimer=setTimeout(async()=>{
      if(!desktopLibrary)return;
      try{await loadLibrary(false);}catch(error){console.warn('Could not refresh playback progress after focus',error)}
    },350);
  });

  async function boot() {
    console.info('[HED Space] desktop bridge booting');
    installWindowChrome(); installSetup(); installSourcesManager(); installEditor(); installDetailsActions(); installListenActions(); installListenVolume();

    // The intro is visible in the HTML before JavaScript runs, so there is no
    // one-frame flash of the unfinished Watch screen. Restart its animation now.
    try { sessionStorage.removeItem('hed-intro-seen'); } catch (_) {}
    if (typeof window.showIntro === 'function') window.showIntro();
    await revealStartupWindow();

    // Hero artwork and app information can load in parallel.
    const heroPromise = loadStandardHeroImages().catch(error => {
      console.warn('[HED Space] Standard hero preload failed', error);
    });

    try {
      const info = await invoke('get_app_info');
      if(!info.library_source_count){
        await heroPromise;
        document.querySelector('#hedSetup').classList.add('open');
        return;
      }

      if(!info.mpv_available) console.warn('mpv not found yet; browsing and editing still work, playback will show a clear setup error.');

      // FAST STARTUP: render the SQLite library immediately. Do not make the
      // user wait for a full filesystem + ffprobe scan on every launch.
      await loadLibrary(false);
      renderStandard('home');
      setActiveNav('home');

      // Permanent app images are local files and normally finish during the intro.
      // Re-apply them when ready without delaying the first usable render.
      heroPromise.then(() => {
        if (!desktopLibrary) return;
        replaceStaticData(desktopLibrary);
        const section = app?.dataset?.section || currentSection || 'home';
        if (section === 'atmospheres') applyAtmosphere();
        else renderStandard(section);
      });

      // Keep the library fresh, but do the expensive scan after startup.
      scheduleBackgroundStartupRescan();
    } catch(error) {
      console.error(error);
      document.querySelector('#hedSetup').classList.add('open');
      document.querySelector('#hedSetupStatus').textContent=String(error);
    }
  }

  boot();
})();
