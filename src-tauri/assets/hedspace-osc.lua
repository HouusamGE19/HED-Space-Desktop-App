local mp = require 'mp'

local overlay = mp.create_osd_overlay('ass-events')
local state = {
    visible = true,
    last_input = mp.get_time(),
    menu = nil,
}
local ui = {}

local function clamp(v, a, b)
    if v < a then return a end
    if v > b then return b end
    return v
end

local function ass_escape(value)
    value = tostring(value or '')
    local ok, escaped = pcall(mp.command_native, {'escape-ass', value})
    if ok and escaped then return escaped end
    return value:gsub('\\', '\\\\'):gsub('{', '\\{'):gsub('}', '\\}')
end

local function color(rgb)
    -- ASS uses BGR order.
    local r = rgb:sub(1, 2)
    local g = rgb:sub(3, 4)
    local b = rgb:sub(5, 6)
    return b .. g .. r
end

local COLORS = {
    white = color('FFFFFF'),
    muted = color('B8B8B8'),
    red = color('E50914'),
    black = color('000000'),
    panel = color('111111'),
    track = color('5A5A5A'),
}

local function ahex(alpha)
    return string.format('%02X', clamp(math.floor(alpha + 0.5), 0, 255))
end

local function rect(parts, x1, y1, x2, y2, fill, alpha)
    parts[#parts + 1] = string.format(
        '{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H%s&\\1a&H%s&\\p1}m %.1f %.1f l %.1f %.1f l %.1f %.1f l %.1f %.1f{\\p0}',
        fill, ahex(alpha), x1, y1, x2, y1, x2, y2, x1, y2
    )
end

local function circle(parts, cx, cy, r, fill, alpha)
    local k = r * 0.5522847498
    parts[#parts + 1] = string.format(
        '{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H%s&\\1a&H%s&\\p1}m %.1f %.1f b %.1f %.1f %.1f %.1f %.1f %.1f b %.1f %.1f %.1f %.1f %.1f %.1f b %.1f %.1f %.1f %.1f %.1f %.1f b %.1f %.1f %.1f %.1f %.1f %.1f{\\p0}',
        fill, ahex(alpha),
        cx - r, cy,
        cx - r, cy - k, cx - k, cy - r, cx, cy - r,
        cx + k, cy - r, cx + r, cy - k, cx + r, cy,
        cx + r, cy + k, cx + k, cy + r, cx, cy + r,
        cx - k, cy + r, cx - r, cy + k, cx - r, cy
    )
end

local function text(parts, x, y, value, size, align, fill, alpha, bold)
    parts[#parts + 1] = string.format(
        '{\\an%d\\pos(%.1f,%.1f)\\fnSegoe UI\\fs%.1f\\b%d\\bord0\\shad0\\1c&H%s&\\1a&H%s&}%s',
        align or 7, x, y, size, bold and 1 or 0, fill or COLORS.white, ahex(alpha or 0), ass_escape(value)
    )
end

local function triangle(parts, cx, cy, r, fill, alpha)
    parts[#parts + 1] = string.format(
        '{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H%s&\\1a&H%s&\\p1}m %.1f %.1f l %.1f %.1f l %.1f %.1f{\\p0}',
        fill, ahex(alpha), cx - r * 0.55, cy - r, cx + r, cy, cx - r * 0.55, cy + r
    )
end

local function fullscreen_icon(parts, cx, cy, size, fill, alpha, is_fullscreen)
    local t = math.max(2, size * 0.10)
    local arm = size * 0.42
    local left, right = cx - size, cx + size
    local top, bottom = cy - size, cy + size

    if not is_fullscreen then
        -- Four outward corners: enter fullscreen.
        rect(parts, left, top, left + arm, top + t, fill, alpha)
        rect(parts, left, top, left + t, top + arm, fill, alpha)
        rect(parts, right - arm, top, right, top + t, fill, alpha)
        rect(parts, right - t, top, right, top + arm, fill, alpha)
        rect(parts, left, bottom - t, left + arm, bottom, fill, alpha)
        rect(parts, left, bottom - arm, left + t, bottom, fill, alpha)
        rect(parts, right - arm, bottom - t, right, bottom, fill, alpha)
        rect(parts, right - t, bottom - arm, right, bottom, fill, alpha)
    else
        -- Four inward corners: exit fullscreen.
        local inset = size * 0.40
        local il, ir = cx - inset, cx + inset
        local it, ib = cy - inset, cy + inset
        rect(parts, left, it, il, it + t, fill, alpha)
        rect(parts, il - t, top, il, it + t, fill, alpha)
        rect(parts, ir, it, right, it + t, fill, alpha)
        rect(parts, ir, top, ir + t, it + t, fill, alpha)
        rect(parts, left, ib - t, il, ib, fill, alpha)
        rect(parts, il - t, ib - t, il, bottom, fill, alpha)
        rect(parts, ir, ib - t, right, ib, fill, alpha)
        rect(parts, ir, ib - t, ir + t, bottom, fill, alpha)
    end
end

local function within(r, x, y)
    return r and x >= r.x1 and x <= r.x2 and y >= r.y1 and y <= r.y2
end

local function mouse()
    local pos = mp.get_property_native('mouse-pos') or {}
    return tonumber(pos.x) or -1, tonumber(pos.y) or -1, pos.hover == true
end

local function fmt_time(seconds)
    seconds = math.max(0, math.floor(tonumber(seconds) or 0))
    local h = math.floor(seconds / 3600)
    local m = math.floor((seconds % 3600) / 60)
    local s = seconds % 60
    if h > 0 then
        return string.format('%d:%02d:%02d', h, m, s)
    end
    return string.format('%d:%02d', m, s)
end

local function track_label(track, index)
    local title = track.title
    local lang = track.lang
    if title and title ~= '' then
        if lang and lang ~= '' then return title .. '  ·  ' .. string.upper(lang) end
        return title
    end
    if lang and lang ~= '' then return string.upper(lang) end
    return 'Track ' .. tostring(index)
end

local function show_controls()
    state.visible = true
    state.last_input = mp.get_time()
end

local function toggle_pause()
    mp.commandv('cycle', 'pause')
    show_controls()
end

local function seek_relative(amount)
    mp.commandv('seek', tostring(amount), 'relative+exact')
    show_controls()
end

local function close_player()
    mp.commandv('quit')
end

local function forced_subtitle_track(track)
    if track.forced == true then return true end
    local title = string.lower(tostring(track.title or ''))
    return string.find(title, 'forced', 1, true) ~= nil
end

local function current_tracks(kind)
    local all = mp.get_property_native('track-list') or {}
    local out = {}
    for _, tr in ipairs(all) do
        if tr.type == kind and not (kind == 'sub' and forced_subtitle_track(tr)) then
            out[#out + 1] = tr
        end
    end
    return out
end

local function draw_menu(parts, kind, w, h, s, mx, my)
    local tracks = current_tracks(kind == 'audio' and 'audio' or 'sub')
    local rows = {}
    if kind == 'subtitle' then
        rows[#rows + 1] = {off = true, selected = mp.get_property('sid', 'no') == 'no', label = 'Off'}
    end
    for i, tr in ipairs(tracks) do
        rows[#rows + 1] = {track = tr, selected = tr.selected == true, label = track_label(tr, i)}
    end

    local row_h = math.floor(46 * s)
    local pad = math.floor(14 * s)
    local panel_w = math.floor(390 * s)
    local max_rows = math.min(#rows, 8)
    local panel_h = math.max(row_h + pad * 2, max_rows * row_h + pad * 2)
    local right = math.floor(w - 52 * s)
    local bottom = math.floor(h - 118 * s)
    local left = right - panel_w
    local top = bottom - panel_h

    rect(parts, left, top, right, bottom, COLORS.panel, 20)
    rect(parts, left, top, right, top + 1, COLORS.white, 210)
    text(parts, left + pad, top - 12 * s, kind == 'audio' and 'Audio' or 'Subtitles', 18 * s, 1, COLORS.white, 0, true)

    ui.menu_rows = {}
    for i = 1, max_rows do
        local row = rows[i]
        local y1 = top + pad + (i - 1) * row_h
        local y2 = y1 + row_h
        local rr = {x1 = left + 4, y1 = y1, x2 = right - 4, y2 = y2}
        ui.menu_rows[#ui.menu_rows + 1] = {rect = rr, row = row, kind = kind}
        if within(rr, mx, my) then rect(parts, rr.x1, rr.y1, rr.x2, rr.y2, COLORS.white, 225) end
        if row.selected then
            rect(parts, left + pad, y1 + row_h * 0.25, left + pad + 4 * s, y2 - row_h * 0.25, COLORS.red, 0)
        end
        text(parts, left + pad + 16 * s, (y1 + y2) / 2, row.label, 18 * s, 4, row.selected and COLORS.white or COLORS.muted, 0, row.selected)
    end
    ui.menu_panel = {x1 = left, y1 = top, x2 = right, y2 = bottom}
end

local function render()
    local w, h = mp.get_osd_size()
    if not w or not h or w <= 0 or h <= 0 then return end
    overlay.res_x = w
    overlay.res_y = h

    local paused = mp.get_property_bool('pause', false)
    if paused then state.visible = true end
    if state.menu then state.visible = true end
    if not paused and not state.menu and mp.get_time() - state.last_input > 2.8 then
        state.visible = false
    end

    if not state.visible then
        overlay.data = ''
        overlay:update()
        ui = {}
        return
    end

    local s = math.min(w / 1920, h / 1080)
    s = clamp(s, 0.72, 1.65)
    local mx, my, hover = mouse()
    if not hover then mx, my = -1, -1 end
    local parts = {}
    ui = {menu_rows = {}}

    -- Smooth Netflix-style top/bottom shading.  The previous version used only
    -- a handful of wide ASS rectangles, which made visible horizontal bands
    -- on bright scenes.  Many thin overlapping bands + smoothstep interpolation
    -- keep the cinematic fade while making the transitions effectively seamless.
    local function smooth_fade(y1, y2, alpha1, alpha2, steps)
        local height = y2 - y1
        for i = 0, steps - 1 do
            local t0 = i / steps
            local t1 = (i + 1) / steps
            local t = (t0 + t1) * 0.5
            local eased = t * t * (3 - 2 * t)
            local alpha = alpha1 + (alpha2 - alpha1) * eased
            -- Tiny overlap prevents one-pixel seams from ASS rasterization/scaling.
            local by1 = y1 + height * t0
            local by2 = y1 + height * t1 + 1.25
            rect(parts, 0, by1, w, by2, COLORS.black, alpha)
        end
    end

    local top_h = math.floor(150 * s)
    smooth_fade(0, top_h, 126, 248, 48)

    local fade_h = math.floor(285 * s)
    smooth_fade(h - fade_h, h, 248, 42, 64)

    local margin = math.floor(56 * s)

    -- Netflix-style back affordance plus an explicit close button. Both return to HED Space.
    ui.back_home = {x1 = margin - 12 * s, y1 = 28 * s, x2 = margin + 190 * s, y2 = 82 * s}
    -- Keep the full click/hover hit area, but do not draw a black hover rectangle.
    text(parts, margin, 55 * s, '←  HED SPACE', 19 * s, 4, COLORS.white, 0, true)

    -- Close button. It closes mpv, and the Rust progress worker returns focus to HED Space.
    local close_cx, close_cy, close_r = w - 58 * s, 54 * s, 25 * s
    ui.close = {x1 = close_cx - 30 * s, y1 = close_cy - 30 * s, x2 = close_cx + 30 * s, y2 = close_cy + 30 * s}
    local close_hover = within(ui.close, mx, my)
    circle(parts, close_cx, close_cy, close_r, COLORS.black, close_hover and 48 or 92)
    text(parts, close_cx, close_cy - 1 * s, '×', 34 * s, 5, COLORS.white, 0, false)
    if close_hover then text(parts, close_cx - 40 * s, close_cy, 'Close', 15 * s, 6, COLORS.muted, 0, false) end

    local title = mp.get_property('media-title', '')
    local progress_y = h - 142 * s
    text(parts, margin, progress_y - 58 * s, title, 26 * s, 1, COLORS.white, 0, true)

    -- Timeline.
    local px1, px2 = margin, w - margin
    local duration = mp.get_property_number('duration', 0) or 0
    local position = mp.get_property_number('time-pos', 0) or 0
    local frac = duration > 0 and clamp(position / duration, 0, 1) or 0
    ui.progress = {x1 = px1, y1 = progress_y - 13 * s, x2 = px2, y2 = progress_y + 13 * s}
    local progress_hover = within(ui.progress, mx, my)
    local track_h = progress_hover and 7 * s or 4 * s
    rect(parts, px1, progress_y - track_h / 2, px2, progress_y + track_h / 2, COLORS.track, 65)
    local played_x = px1 + (px2 - px1) * frac
    rect(parts, px1, progress_y - track_h / 2, played_x, progress_y + track_h / 2, COLORS.red, 0)
    circle(parts, played_x, progress_y, progress_hover and 9 * s or 5 * s, COLORS.red, 0)

    local cy = h - 82 * s
    local button_r = 28 * s

    -- Play / pause.
    local play_cx = margin + 26 * s
    ui.play = {x1 = play_cx - button_r, y1 = cy - button_r, x2 = play_cx + button_r, y2 = cy + button_r}
    if within(ui.play, mx, my) then circle(parts, play_cx, cy, button_r, COLORS.white, 218) end
    if paused then
        triangle(parts, play_cx + 1 * s, cy, 13 * s, COLORS.white, 0)
    else
        rect(parts, play_cx - 9 * s, cy - 14 * s, play_cx - 3 * s, cy + 14 * s, COLORS.white, 0)
        rect(parts, play_cx + 4 * s, cy - 14 * s, play_cx + 10 * s, cy + 14 * s, COLORS.white, 0)
    end

    -- 10-second seek controls.
    local back_cx = play_cx + 78 * s
    local fwd_cx = back_cx + 72 * s
    ui.back10 = {x1 = back_cx - 31 * s, y1 = cy - 27 * s, x2 = back_cx + 31 * s, y2 = cy + 27 * s}
    ui.forward10 = {x1 = fwd_cx - 31 * s, y1 = cy - 27 * s, x2 = fwd_cx + 31 * s, y2 = cy + 27 * s}
    if within(ui.back10, mx, my) then circle(parts, back_cx, cy, 27 * s, COLORS.white, 220) end
    if within(ui.forward10, mx, my) then circle(parts, fwd_cx, cy, 27 * s, COLORS.white, 220) end
    text(parts, back_cx, cy, '−10', 18 * s, 5, COLORS.white, 0, true)
    text(parts, fwd_cx, cy, '+10', 18 * s, 5, COLORS.white, 0, true)

    -- Elapsed / total time.
    local time_x = fwd_cx + 58 * s
    text(parts, time_x, cy, fmt_time(position) .. '  /  ' .. fmt_time(duration), 17 * s, 4, COLORS.muted, 0, false)

    -- Right-side controls are laid out from the screen edge inward.
    -- Keep dedicated gaps between each hit target so the volume slider never
    -- collides with Audio/Subtitles at different resolutions or DPI scales.
    local fullscreen_x = w - 66 * s
    local sub_x = fullscreen_x - 150 * s
    local audio_x = sub_x - 155 * s

    -- Volume control. The slider ends well before the Audio button hitbox.
    local vol = clamp(mp.get_property_number('volume', 100) or 100, 0, 100)
    local muted = mp.get_property_bool('mute', false)
    local vol_x2 = audio_x - 82 * s
    local vol_x1 = vol_x2 - 120 * s
    ui.mute = {x1 = vol_x1 - 58 * s, y1 = cy - 25 * s, x2 = vol_x1 - 4 * s, y2 = cy + 25 * s}
    ui.volume = {x1 = vol_x1, y1 = cy - 16 * s, x2 = vol_x2, y2 = cy + 16 * s}
    text(parts, vol_x1 - 12 * s, cy, muted and 'Muted' or 'Vol', 16 * s, 6, muted and COLORS.red or COLORS.white, 0, true)
    rect(parts, vol_x1, cy - 2 * s, vol_x2, cy + 2 * s, COLORS.track, 60)
    local vol_end = vol_x1 + (vol_x2 - vol_x1) * (muted and 0 or vol / 100)
    rect(parts, vol_x1, cy - 2 * s, vol_end, cy + 2 * s, COLORS.white, 0)
    circle(parts, vol_end, cy, within(ui.volume, mx, my) and 6 * s or 4 * s, COLORS.white, 0)

    -- Audio, subtitles and fullscreen buttons.
    ui.audio = {x1 = audio_x - 50 * s, y1 = cy - 25 * s, x2 = audio_x + 50 * s, y2 = cy + 25 * s}
    ui.subtitle = {x1 = sub_x - 58 * s, y1 = cy - 25 * s, x2 = sub_x + 58 * s, y2 = cy + 25 * s}
    ui.fullscreen = {x1 = fullscreen_x - 31 * s, y1 = cy - 31 * s, x2 = fullscreen_x + 31 * s, y2 = cy + 31 * s}
    if within(ui.audio, mx, my) or state.menu == 'audio' then rect(parts, ui.audio.x1, ui.audio.y1, ui.audio.x2, ui.audio.y2, COLORS.white, 220) end
    if within(ui.subtitle, mx, my) or state.menu == 'subtitle' then rect(parts, ui.subtitle.x1, ui.subtitle.y1, ui.subtitle.x2, ui.subtitle.y2, COLORS.white, 220) end
    local fullscreen_hover = within(ui.fullscreen, mx, my)
    if fullscreen_hover then circle(parts, fullscreen_x, cy, 27 * s, COLORS.white, 220) end
    text(parts, audio_x, cy, 'Audio', 17 * s, 5, COLORS.white, 0, true)
    text(parts, sub_x, cy, 'Subtitles', 17 * s, 5, COLORS.white, 0, true)
    local is_fullscreen = mp.get_property_bool('fullscreen', false)
    fullscreen_icon(parts, fullscreen_x, cy, 12 * s, COLORS.white, 0, is_fullscreen)
    if fullscreen_hover then
        text(parts, fullscreen_x, cy - 45 * s, is_fullscreen and 'Exit fullscreen' or 'Fullscreen', 14 * s, 2, COLORS.muted, 0, false)
    end

    if state.menu then draw_menu(parts, state.menu, w, h, s, mx, my) end

    -- Clean central play affordance: larger white triangle only.
    if paused and not state.menu then
        local center_hit_r = 50 * s
        triangle(parts, w / 2 + 4 * s, h / 2, 28 * s, COLORS.white, 0)
        ui.center_play = {x1 = w / 2 - center_hit_r, y1 = h / 2 - center_hit_r, x2 = w / 2 + center_hit_r, y2 = h / 2 + center_hit_r}
    end

    overlay.data = table.concat(parts, '\n')
    overlay:update()
end

local function click()
    show_controls()
    local x, y, hover = mouse()
    if not hover then return end

    if within(ui.close, x, y) or within(ui.back_home, x, y) then close_player(); return end

    if state.menu and ui.menu_rows then
        for _, item in ipairs(ui.menu_rows) do
            if within(item.rect, x, y) then
                if item.row.off then
                    mp.set_property('sid', 'no')
                elseif item.row.track then
                    if item.kind == 'audio' then
                        mp.set_property_number('aid', item.row.track.id)
                    else
                        mp.set_property_number('sid', item.row.track.id)
                    end
                end
                state.menu = nil
                render()
                return
            end
        end
        if within(ui.menu_panel, x, y) then return end
    end

    if within(ui.progress, x, y) then
        local duration = mp.get_property_number('duration', 0) or 0
        if duration > 0 then
            local frac = clamp((x - ui.progress.x1) / (ui.progress.x2 - ui.progress.x1), 0, 1)
            mp.commandv('seek', tostring(frac * 100), 'absolute-percent+exact')
        end
        render()
        return
    end
    if within(ui.play, x, y) or within(ui.center_play, x, y) then toggle_pause(); render(); return end
    if within(ui.back10, x, y) then seek_relative(-10); render(); return end
    if within(ui.forward10, x, y) then seek_relative(10); render(); return end
    if within(ui.mute, x, y) then mp.commandv('cycle', 'mute'); render(); return end
    if within(ui.volume, x, y) then
        local frac = clamp((x - ui.volume.x1) / (ui.volume.x2 - ui.volume.x1), 0, 1)
        mp.set_property_number('volume', frac * 100)
        mp.set_property_bool('mute', false)
        render()
        return
    end
    if within(ui.audio, x, y) then
        state.menu = state.menu == 'audio' and nil or 'audio'
        render()
        return
    end
    if within(ui.subtitle, x, y) then
        state.menu = state.menu == 'subtitle' and nil or 'subtitle'
        render()
        return
    end
    if within(ui.fullscreen, x, y) then
        mp.commandv('cycle', 'fullscreen')
        show_controls()
        render()
        return
    end

    if state.menu then
        state.menu = nil
        render()
        return
    end

    -- Clicking the video toggles playback when the controls are already visible.
    toggle_pause()
    render()
end

local function mouse_move()
    show_controls()
    render()
end

mp.add_forced_key_binding('MOUSE_MOVE', 'hedspace-mouse-move', mouse_move)
mp.add_forced_key_binding('MBTN_LEFT', 'hedspace-click', click)
mp.add_forced_key_binding('ESC', 'hedspace-close', close_player)
mp.add_forced_key_binding('q', 'hedspace-close-q', close_player)
mp.add_forced_key_binding('SPACE', 'hedspace-pause', function() toggle_pause(); render() end)
mp.add_forced_key_binding('k', 'hedspace-pause-k', function() toggle_pause(); render() end)
mp.add_forced_key_binding('LEFT', 'hedspace-back10', function() seek_relative(-10); render() end)
mp.add_forced_key_binding('RIGHT', 'hedspace-forward10', function() seek_relative(10); render() end)
mp.add_forced_key_binding('j', 'hedspace-back10-j', function() seek_relative(-10); render() end)
mp.add_forced_key_binding('l', 'hedspace-forward10-l', function() seek_relative(10); render() end)
mp.add_forced_key_binding('m', 'hedspace-mute', function() mp.commandv('cycle', 'mute'); show_controls(); render() end)
mp.add_forced_key_binding('f', 'hedspace-fullscreen', function() mp.commandv('cycle', 'fullscreen'); show_controls(); render() end)
mp.add_forced_key_binding('a', 'hedspace-audio-menu', function() state.menu = state.menu == 'audio' and nil or 'audio'; show_controls(); render() end)
mp.add_forced_key_binding('s', 'hedspace-sub-menu', function() state.menu = state.menu == 'subtitle' and nil or 'subtitle'; show_controls(); render() end)

for _, prop in ipairs({'pause', 'media-title', 'duration', 'track-list', 'aid', 'sid', 'mute', 'volume', 'fullscreen'}) do
    mp.observe_property(prop, 'native', function() render() end)
end

mp.register_event('file-loaded', function()
    state.visible = true
    state.last_input = mp.get_time()
    render()
end)
mp.register_event('shutdown', function() overlay:remove() end)

mp.add_periodic_timer(0.10, render)
render()
