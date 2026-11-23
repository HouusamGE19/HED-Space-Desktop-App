(() => {
  const T = window.__TAURI__;
  if (!T?.core?.invoke) return;
  const { invoke } = T.core;
  const playerWindow = T.window.getCurrentWindow();
  const $ = s => document.querySelector(s);
  const shell = $('#playerShell');

  let snapshot = null;
  let draggingTimeline = false;
  let inactivityTimer = null;
  let pollTimer = null;
  let autoNextFor = null;
  let nextDismissedFor = null;
  let closing = false;
  let feedbackTimer = null;

  const escapeHtml = (value='') => String(value).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;').replaceAll("'",'&#039;');
  const fmt = value => {
    const total = Math.max(0, Math.floor(Number(value) || 0));
    const h = Math.floor(total / 3600), m = Math.floor((total % 3600) / 60), s = total % 60;
    return h ? `${h}:${String(m).padStart(2,'0')}:${String(s).padStart(2,'0')}` : `${m}:${String(s).padStart(2,'0')}`;
  };
  const contextFor = s => {
    if (!s) return '';
    if (s.media_kind === 'episode') {
      const ep = [s.season_number != null ? `S${s.season_number}` : '', s.episode_number != null ? `E${s.episode_number}` : ''].join('');
      return [s.series_title, ep].filter(Boolean).join(' · ');
    }
    return '';
  };
  const nextContextFor = s => {
    if (!s) return '';
    const ep = [s.next_season_number != null ? `S${s.next_season_number}` : '', s.next_episode_number != null ? `E${s.next_episode_number}` : ''].join('');
    return [s.series_title, ep].filter(Boolean).join(' · ');
  };
  const labelTrack = track => [track.lang?.toUpperCase(), track.title].filter(Boolean).join(' · ') || `${track.kind} ${track.id}`;
  const isForcedSubtitlePlaceholder = track => {
    const title = String(track?.title || '').trim().toLowerCase();
    return track?.forced === true || /^(forced|forced subtitle|forced subtitles)$/.test(title);
  };

  function revealControls(force=false) {
    shell.classList.remove('idle');
    clearTimeout(inactivityTimer);
    if (force || snapshot?.paused || $('#trackPanel') && !$('#trackPanel').hidden || draggingTimeline) return;
    inactivityTimer = setTimeout(() => {
      if (!snapshot?.paused && $('#trackPanel').hidden && !draggingTimeline) shell.classList.add('idle');
    }, 2600);
  }

  function setPlayState(paused) {
    shell.classList.toggle('is-paused', paused);
    shell.classList.toggle('is-playing', !paused);
    if (paused) revealControls(true); else revealControls();
  }

  function showSeekFeedback(value) {
    const box = $('#seekFeedback');
    $('#seekFeedbackText').textContent = value > 0 ? `+${value}` : String(value);
    box.classList.remove('show');
    void box.offsetWidth;
    box.classList.add('show');
    clearTimeout(feedbackTimer);
    feedbackTimer = setTimeout(() => box.classList.remove('show'), 620);
  }

  function renderTracks() {
    if (!snapshot) return;
    const audio = $('#audioMenu');
    audio.innerHTML = snapshot.audio_tracks.length
      ? snapshot.audio_tracks.map(t => `<button data-track-type="audio" data-track-id="${t.id}" class="${t.selected?'selected':''}"><span>${escapeHtml(labelTrack(t))}</span><small>Audio</small></button>`).join('')
      : '<div class="empty-track">No selectable audio tracks</div>';
    const subs = $('#subtitleMenu');
    const visibleSubtitleTracks = snapshot.subtitle_tracks.filter(t => !isForcedSubtitlePlaceholder(t));
    const visibleSubtitleSelected = visibleSubtitleTracks.some(t => t.selected);
    subs.innerHTML = `<button data-sub-off="1" class="${visibleSubtitleSelected?'':'selected'}"><span>Off</span><small>Subtitles</small></button>` + visibleSubtitleTracks.map(t => `<button data-track-type="subtitle" data-track-id="${t.id}" class="${t.selected?'selected':''}"><span>${escapeHtml(labelTrack(t))}</span><small>Subtitle</small></button>`).join('');

    document.querySelectorAll('[data-track-id]').forEach(button => button.onclick = async e => {
      e.stopPropagation();
      await invoke('select_player_track', { trackType:button.dataset.trackType, id:Number(button.dataset.trackId) });
      await poll(); revealControls(true);
    });
    document.querySelector('[data-sub-off]')?.addEventListener('click', async e => {
      e.stopPropagation();
      await invoke('player_action',{action:'sub_off',value:null});
      await poll(); revealControls(true);
    });
  }

  function renderNextEpisode() {
    const box = $('#nextEpisode');
    if (!snapshot?.next_media_id || !snapshot.duration || nextDismissedFor === snapshot.media_id) { box.hidden = true; return; }
    const remain = Math.max(0, snapshot.duration - snapshot.position);
    if (remain <= 12 && snapshot.position > 30) {
      box.hidden = false;
      $('#nextTitle').textContent = snapshot.next_media_title || 'Next episode';
      $('#nextMeta').textContent = nextContextFor(snapshot);
      $('#nextCountdown').textContent = remain <= 1.15 ? 'Starting…' : `Playing automatically in ${Math.max(1,Math.ceil(remain))}s`;
      if (remain <= 1.15 && autoNextFor !== snapshot.media_id) {
        autoNextFor = snapshot.media_id;
        playMedia(snapshot.next_media_id);
      }
    } else box.hidden = true;
  }

  function render() {
    if (!snapshot) return;
    shell.classList.remove('is-loading');
    shell.classList.toggle('is-buffering', !!snapshot.buffering);
    $('#playerTitle').textContent = snapshot.title;
    $('#bottomTitle').textContent = snapshot.title;
    const context = contextFor(snapshot);
    $('#playerContext').textContent = context;
    $('#bottomContext').textContent = context;
    setPlayState(snapshot.paused);

    const duration = Math.max(0, snapshot.duration || 0), pos = Math.max(0, snapshot.position || 0);
    if (!draggingTimeline) {
      $('#timeline').max = String(duration || 100);
      $('#timeline').value = String(Math.min(pos, duration || pos));
      const pct = duration ? (pos / duration) * 100 : 0;
      $('#timeline').style.setProperty('--played', `${Math.max(0,Math.min(100,pct))}%`);
    }
    $('#currentTime').textContent = fmt(pos);
    $('#totalTime').textContent = fmt(duration);
    if (document.activeElement !== $('#volume')) $('#volume').value = String(snapshot.volume ?? 100);
    $('#previous').disabled = snapshot.previous_media_id == null;
    $('#next').disabled = snapshot.next_media_id == null;
    renderTracks();
    renderNextEpisode();
  }

  async function poll() {
    try {
      snapshot = await invoke('get_player_snapshot');
      $('#playerError').hidden = true;
      render();
    } catch (error) {
      if (closing) return;
      const message = String(error);
      if (!/No active player/i.test(message)) {
        $('#playerErrorText').textContent = message;
        $('#playerError').hidden = false;
      }
    }
  }

  async function action(name,value=null) {
    try { await invoke('player_action',{action:name,value}); await poll(); }
    catch(error){ console.error(error); }
    revealControls();
  }

  async function seekBy(seconds) {
    showSeekFeedback(seconds);
    await action('seek_relative', seconds);
  }

  async function playMedia(id) {
    if (id == null) return;
    try {
      shell.classList.add('is-loading');
      await invoke('open_player',{mediaId:Number(id)});
      autoNextFor = null;
      nextDismissedFor = null;
      $('#trackPanel').hidden = true;
      revealControls(true);
      setTimeout(poll,180);
    } catch(error) {
      shell.classList.remove('is-loading');
      $('#playerErrorText').textContent=String(error); $('#playerError').hidden=false;
    }
  }

  async function closePlayer() {
    if (closing) return;
    closing = true;
    clearInterval(pollTimer); clearTimeout(inactivityTimer);
    try { await invoke('close_player'); } catch(error) { console.error(error); }
  }

  $('#playerBack').onclick = closePlayer;
  $('#exitPlayer').onclick = closePlayer;
  $('#errorBack').onclick = closePlayer;
  $('#centerPlay').onclick = e => {e.stopPropagation(); action('toggle_pause');};
  $('#bottomPlay').onclick = e => {e.stopPropagation(); action('toggle_pause');};
  $('#rewind').onclick = e => {e.stopPropagation(); seekBy(-10);};
  $('#forward').onclick = e => {e.stopPropagation(); seekBy(10);};
  $('#previous').onclick = e => {e.stopPropagation(); snapshot?.previous_media_id != null && playMedia(snapshot.previous_media_id);};
  $('#next').onclick = e => {e.stopPropagation(); snapshot?.next_media_id != null && playMedia(snapshot.next_media_id);};
  $('#playNextNow').onclick = e => {e.stopPropagation(); snapshot?.next_media_id != null && playMedia(snapshot.next_media_id);};
  $('#dismissNext').onclick = e => {e.stopPropagation(); if(snapshot){nextDismissedFor=snapshot.media_id;$('#nextEpisode').hidden=true;} revealControls();};
  $('#tracksButton').onclick = e => { e.stopPropagation(); $('#trackPanel').hidden = !$('#trackPanel').hidden; revealControls(true); };
  $('#closeTracks').onclick = e => {e.stopPropagation(); $('#trackPanel').hidden=true; revealControls();};
  $('#trackPanel').onclick = e => e.stopPropagation();
  $('#volumeButton').onclick = e => {e.stopPropagation(); const v=Number($('#volume').value); action('volume',v>0?0:80);};
  $('#volume').addEventListener('input',e=>{e.stopPropagation();action('volume',Number(e.target.value));});

  const timeline=$('#timeline');
  timeline.addEventListener('pointerdown',e=>{e.stopPropagation();draggingTimeline=true;revealControls(true)});
  timeline.addEventListener('input',e=>{
    const value=Number(e.target.value); const duration=Number(e.target.max)||0;
    const pct=duration?value/duration*100:0; timeline.style.setProperty('--played',`${pct}%`);
    $('#currentTime').textContent=fmt(value);
  });
  timeline.addEventListener('change',async e=>{draggingTimeline=false;await action('seek_absolute',Number(e.target.value));});
  timeline.addEventListener('pointermove',e=>{
    if(!snapshot?.duration)return;
    const rect=timeline.getBoundingClientRect(); const ratio=Math.max(0,Math.min(1,(e.clientX-rect.left)/rect.width));
    const preview=$('#timelinePreview'); preview.hidden=false; preview.style.left=`${ratio*100}%`; $('#previewTime').textContent=fmt(ratio*snapshot.duration);
  });
  timeline.addEventListener('pointerleave',()=>{$('#timelinePreview').hidden=true});

  document.addEventListener('pointermove',()=>revealControls(),{passive:true});
  document.addEventListener('pointerdown',e=>{
    if (!e.target.closest('button,input,.track-panel,.next-episode')) {
      $('#trackPanel').hidden = true;
      action('toggle_pause');
    }
    revealControls();
  });
  document.addEventListener('keydown',async e=>{
    const key=e.key.toLowerCase();
    if(e.key==='Escape'){ e.preventDefault(); if(!$('#trackPanel').hidden){$('#trackPanel').hidden=true;revealControls();return;} await closePlayer(); return; }
    if(e.key===' ' || key==='k'){e.preventDefault();action('toggle_pause');}
    else if(e.key==='ArrowLeft' || key==='j'){e.preventDefault();seekBy(-10);}
    else if(e.key==='ArrowRight' || key==='l'){e.preventDefault();seekBy(10);}
    else if(e.key==='ArrowUp'){e.preventDefault();action('volume',Math.min(130,(snapshot?.volume||100)+5));}
    else if(e.key==='ArrowDown'){e.preventDefault();action('volume',Math.max(0,(snapshot?.volume||100)-5));}
    else if(key==='m'){e.preventDefault();$('#volumeButton').click();}
    else if(key==='n' && snapshot?.next_media_id){e.preventDefault();playMedia(snapshot.next_media_id);}
    revealControls();
  });

  playerWindow.onCloseRequested(async event => {
    if (closing) return;
    event.preventDefault();
    await closePlayer();
  }).catch(console.warn);

  shell.classList.add('is-paused');
  revealControls(true);
  poll();
  pollTimer=setInterval(poll,450);
})();
