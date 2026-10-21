//! Heads-up display drawn over the 3D view (resolution independent).

use ob_engine::render::ShadeLuts;
use ob_game::anim::Qav;
use ob_game::game::Game;
use ob_game::resources::Resources;

/// Built-in 5x7 font (OpenBlood's own), rows top to bottom, 5 low bits per row.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        'D' => [0x1c, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1c],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x11, 0x0a, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        '.' => [0, 0, 0, 0, 0, 0x0c, 0x0c],
        ',' => [0, 0, 0, 0, 0x0c, 0x04, 0x08],
        ':' => [0, 0x0c, 0x0c, 0, 0x0c, 0x0c, 0],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0, 0x04],
        '?' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
        '-' => [0, 0, 0, 0x1f, 0, 0, 0],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        '\'' => [0x04, 0x04, 0x08, 0, 0, 0, 0],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        _ => [0; 7],
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_text(frame: &mut [u32], w: usize, h: usize, x: i32, y: i32, scale: i32, color: u32, text: &str) {
    let mut cx = x;
    for ch in text.chars() {
        let g = glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        // Drop shadow first, then glyph.
                        for (ox, oy, c) in [(scale / 2 + 1, scale / 2 + 1, 0u32), (0, 0, color)] {
                            let px = cx + col * scale + sx + ox;
                            let py = y + row as i32 * scale + sy + oy;
                            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                                frame[py as usize * w + px as usize] = c;
                            }
                        }
                    }
                }
            }
        }
        cx += 6 * scale;
    }
}

pub fn text_width(text: &str, scale: i32) -> i32 {
    text.chars().count() as i32 * 6 * scale
}

/// Draws a QAV frame (weapon) in 320x200 virtual space, centred horizontally.
#[allow(clippy::too_many_arguments)]
pub fn draw_qav(
    frame: &mut [u32],
    w: usize,
    h: usize,
    res: &Resources,
    luts: &ShadeLuts,
    q: &Qav,
    t: i32,
    bob: (f64, f64),
    base_shade: i32,
) {
    if q.frames.is_empty() {
        return;
    }
    let f = &q.frames[q.frame_at(t)];
    let sc = h as f64 / 200.0;
    let cx = w as f64 * 0.5;
    for l in f.layers.iter() {
        if l.picnum <= 0 {
            continue;
        }
        let tile = res.tiles.get(l.picnum);
        if tile.is_empty() {
            continue;
        }
        let zoom = if l.z > 0 { l.z as f64 / 65536.0 } else { 1.0 };
        let (tw, th) = (tile.width as f64, tile.height as f64);
        let vx = (q.x + l.x) as f64 + bob.0;
        let vy = (q.y + l.y) as f64 + bob.1;
        let (left, top) = if l.stat & 16 != 0 {
            (vx, vy)
        } else {
            (
                vx - (tw * 0.5 + tile.anm.xoffset as f64) * zoom,
                vy - (th * 0.5 + tile.anm.yoffset as f64) * zoom,
            )
        };
        let x0 = cx + (left - 160.0) * sc;
        let y0 = top * sc;
        let sw = tw * zoom * sc;
        let shh = th * zoom * sc;
        let lut = luts.get(l.pal);
        let shade = (l.shade as i32 + base_shade).clamp(0, 63) as usize * 256;
        let trans = l.stat & 1 != 0;
        let yflip = l.stat & 4 != 0;
        let px1 = (x0 + sw).min(w as f64).max(0.0) as usize;
        // Clamped so a layer entirely right of the screen gives an empty (not inverted) range.
        let px0 = (x0.max(0.0) as usize).min(px1);
        let py0 = y0.max(0.0) as usize;
        let py1 = (y0 + shh).min(h as f64).max(0.0) as usize;
        for py in py0..py1 {
            let mut ty = (((py as f64 + 0.5 - y0) / shh) * th) as usize;
            ty = ty.min(tile.height as usize - 1);
            if yflip {
                ty = tile.height as usize - 1 - ty;
            }
            let row = &mut frame[py * w..py * w + w];
            for (px, out) in (px0..px1).zip(&mut row[px0..px1]) {
                let tx = ((((px as f64 + 0.5 - x0) / sw) * tw) as usize).min(tile.width as usize - 1);
                let c = tile.pixels[tx * tile.height as usize + ty];
                if c == 255 {
                    continue;
                }
                let rgb = lut[shade + c as usize];
                *out = if trans {
                    let d = *out;
                    let m = |sh: u32| ((((d >> sh) & 255) + ((rgb >> sh) & 255)) / 2) << sh;
                    m(16) | m(8) | m(0)
                } else {
                    rgb
                };
            }
        }
    }
}

fn tint(frame: &mut [u32], color: (f64, f64, f64), amount: f64) {
    if amount <= 0.01 {
        return;
    }
    let a = amount.min(0.8);
    let (r, g, b) = color;
    for p in frame.iter_mut() {
        let pr = ((*p >> 16) & 255) as f64;
        let pg = ((*p >> 8) & 255) as f64;
        let pb = (*p & 255) as f64;
        let nr = (pr + (r - pr) * a) as u32;
        let ng = (pg + (g - pg) * a) as u32;
        let nb = (pb + (b - pb) * a) as u32;
        *p = (nr << 16) | (ng << 8) | nb;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    frame: &mut [u32],
    w: usize,
    h: usize,
    res: &Resources,
    luts: &ShadeLuts,
    game: &Game,
    weapon: Option<&Qav>,
    fps: Option<f64>,
    message: Option<&str>,
) {
    let s = ((h as f64 / 200.0).round() as i32).max(1);
    if let Some(q) = weapon {
        let p = &game.player;
        let bob = (
            (p.bob_phase * 0.5).sin() * p.bob_amp / 64.0,
            (p.bob_phase).sin().abs() * p.bob_amp / 64.0,
        );
        let sect_shade = if p.sector >= 0 {
            (game.world.sectors[p.sector as usize].floor.shade as i32 / 2).max(0)
        } else {
            0
        };
        if !p.dead {
            draw_qav(frame, w, h, res, luts, q, game.weapon_view().1, bob, sect_shade);
        }
    }
    tint(frame, (255.0, 0.0, 0.0), game.damage_flash * 0.6);
    // A strong, short flash: in a recording of the E2M1 start the original is clearly yellow
    // at 0.125 s after the pickup, fainter at 0.25 s and clear by 0.375 s.
    tint(frame, (255.0, 230.0, 120.0), game.pickup_flash * 0.25);
    // Crosshair.
    let (cx, cy) = (w as i32 / 2, h as i32 / 2 - (10 * h as i32 / 200));
    for d in -2 * s..=2 * s {
        for t in 0..(s / 2).max(1) {
            for (x, y) in [(cx + d, cy + t), (cx + t, cy + d)] {
                if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                    frame[y as usize * w + x as usize] = 0x00c0c0c0;
                }
            }
        }
    }
    let p = &game.player;
    let have_bar = status_bar(frame, w, h, res, luts, game);
    inventory_hud(frame, w, h, res, luts, game);
    let big = s * 2;
    if !have_bar {
        draw_text(
            frame,
            w,
            h,
            4 * s,
            h as i32 - 12 * s,
            big,
            0x00d02020,
            &format!("{}", p.health.max(0)),
        );
        if p.armor.iter().any(|&a| a > 0) {
            let txt = format!("ARMOR {}", p.armor.iter().sum::<i32>());
            draw_text(frame, w, h, 40 * s, h as i32 - 10 * s, s, 0x004080ff, &txt);
        }
        // Ammo of current weapon.
        let a = &game.arsenal;
        let def = a.def();
        if def.ammo != 0 {
            let txt = format!("{}", a.ammo[def.ammo]);
            let tw = text_width(&txt, big);
            draw_text(
                frame,
                w,
                h,
                w as i32 - tw - 4 * s,
                h as i32 - 12 * s,
                big,
                0x004060ff,
                &txt,
            );
        }
        // Keys.
        let key_colors = [
            0x00e0e0e0, 0x0040c040, 0x00ff4020, 0x00c0c0ff, 0x00a040ff, 0x00ffff60, 0x00ff80ff,
        ];
        for (k, &c) in key_colors.iter().enumerate() {
            if game.player.keys[k] {
                let x0 = (w as i32 / 2 - 28 * s) + k as i32 * 8 * s;
                let y0 = h as i32 - 8 * s;
                for yy in y0..y0 + 5 * s {
                    for xx in x0..x0 + 6 * s {
                        if xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h {
                            frame[yy as usize * w + xx as usize] = c;
                        }
                    }
                }
            }
        }
    } else if p.armor.iter().any(|&a| a > 0) {
        tile_text(
            frame,
            w,
            h,
            res,
            luts,
            3,
            &format!("ARMOR {}", p.armor.iter().sum::<i32>()),
            72.0,
            190.0,
            Anchor::Left,
            1.0,
            0,
            0,
        );
    }
    if let Some(f) = fps {
        let txt = format!(
            "{:.0} FPS  {}X{}  {}  KILLS {}/{}  X {:.0} Y {:.0} Z {:.0} S {}",
            f, w, h, game.world.map_name, game.kills, game.total_kills, p.x, p.y, p.z, p.sector
        );
        draw_text(frame, w, h, 2 * s, 2 * s, s, 0x00ffff60, &txt);
    }
    if let Some(m) = message {
        if res.tiles.get(font_base(1) + 33).is_empty() {
            let tw = text_width(m, s);
            draw_text(frame, w, h, (w as i32 - tw) / 2, 20 * s, s, 0x00ffffff, m);
        } else {
            let tw = tile_text_width(res, 1, m, 1.0);
            tile_text(
                frame,
                w,
                h,
                res,
                luts,
                1,
                m,
                160.0 - tw / 2.0,
                14.0,
                Anchor::Center,
                1.0,
                0,
                0,
            );
        }
    }
    if p.dead {
        let m = "YOU ARE DEAD - PRESS E";
        let tw = text_width(m, big);
        draw_text(frame, w, h, (w as i32 - tw) / 2, h as i32 / 2, big, 0x00ff3030, m);
    }
}

/// Horizontal anchoring for virtual 320x200 HUD coordinates on wide screens.
#[derive(Clone, Copy, PartialEq)]
pub enum Anchor {
    Left,
    Center,
    Right,
}

/// Draws tile `pic` with its top-left corner at virtual (vx, vy) (320x200 space).
#[allow(clippy::too_many_arguments)]
pub fn vtile(
    frame: &mut [u32],
    w: usize,
    h: usize,
    res: &Resources,
    luts: &ShadeLuts,
    pic: i32,
    vx: f64,
    vy: f64,
    anchor: Anchor,
    zoom: f64,
    shade: i32,
    pal: u8,
) -> f64 {
    let t = res.tiles.get(pic);
    if t.is_empty() {
        return 0.0;
    }
    let sc = h as f64 / 200.0 * zoom;
    let base = match anchor {
        Anchor::Left => 0.0,
        Anchor::Center => w as f64 * 0.5 - 160.0 * h as f64 / 200.0,
        Anchor::Right => w as f64 - 320.0 * h as f64 / 200.0,
    };
    let x0 = base + vx * h as f64 / 200.0;
    let y0 = vy * h as f64 / 200.0;
    let sw = t.width as f64 * sc;
    let shh = t.height as f64 * sc;
    let lut = luts.get(pal);
    let so = shade.clamp(0, 63) as usize * 256;
    let px0 = x0.max(0.0) as usize;
    let px1 = (x0 + sw).min(w as f64).max(0.0) as usize;
    let py0 = y0.max(0.0) as usize;
    let py1 = (y0 + shh).min(h as f64).max(0.0) as usize;
    for py in py0..py1 {
        let ty = (((py as f64 + 0.5 - y0) / sc) as usize).min(t.height as usize - 1);
        for px in px0..px1 {
            let tx = (((px as f64 + 0.5 - x0) / sc) as usize).min(t.width as usize - 1);
            let c = t.pixels[tx * t.height as usize + ty];
            if c != 255 {
                frame[py * w + px] = lut[so + c as usize];
            }
        }
    }
    t.width as f64 * zoom
}

/// First tile of Blood font `n` (characters start at ASCII 32).
pub fn font_base(n: i32) -> i32 {
    4096 + 96 * n
}

/// Glyph tile for `c` in `font`, falling back to other Blood fonts when the font lacks it
/// (the big menu font has no digits or punctuation). Returns (tile, y offset to keep the
/// glyph on the same baseline).
fn font_glyph(res: &Resources, font: i32, c: char) -> Option<(i32, f64)> {
    let idx = (c as i32 - 32).clamp(0, 95);
    let height = |f: i32| res.tiles.get(font_base(f) + ('A' as i32 - 32)).height as f64;
    for f in [font, 1, 0, 3, 4] {
        let pic = font_base(f) + idx;
        if !res.tiles.get(pic).is_empty() {
            let dy = if f == font {
                0.0
            } else {
                (height(font) - height(f)).max(0.0)
            };
            return Some((pic, dy));
        }
    }
    None
}

/// Width in virtual pixels of `text` in font `n`.
pub fn tile_text_width(res: &Resources, font: i32, text: &str, zoom: f64) -> f64 {
    text.chars()
        .map(|c| match font_glyph(res, font, c).filter(|_| c != ' ') {
            Some((pic, _)) => res.tiles.get(pic).width as f64 + 1.0,
            None => 6.0,
        })
        .sum::<f64>()
        * zoom
}

/// Draws text with a Blood tile font; returns the width drawn.
#[allow(clippy::too_many_arguments)]
pub fn tile_text(
    frame: &mut [u32],
    w: usize,
    h: usize,
    res: &Resources,
    luts: &ShadeLuts,
    font: i32,
    text: &str,
    vx: f64,
    vy: f64,
    anchor: Anchor,
    zoom: f64,
    shade: i32,
    pal: u8,
) -> f64 {
    let mut x = vx;
    for c in text.chars() {
        let Some((pic, dy)) = font_glyph(res, font, c).filter(|_| c != ' ') else {
            x += 6.0 * zoom;
            continue;
        };
        vtile(frame, w, h, res, luts, pic, x, vy + dy * zoom, anchor, zoom, shade, pal);
        x += (res.tiles.get(pic).width as f64 + 1.0) * zoom;
    }
    x - vx
}

/// Number drawn with Blood's status digit tiles (base = tile of '0').
#[allow(clippy::too_many_arguments)]
fn status_number(
    frame: &mut [u32],
    w: usize,
    h: usize,
    res: &Resources,
    luts: &ShadeLuts,
    base: i32,
    n: i32,
    right_x: f64,
    vy: f64,
    anchor: Anchor,
) {
    let s = n.max(0).to_string();
    let dw = res.tiles.get(base).width as f64 + 1.0;
    let mut x = right_x - dw * s.len() as f64;
    for c in s.chars() {
        let d = c as i32 - '0' as i32;
        vtile(frame, w, h, res, luts, base + d, x, vy, anchor, 1.0, 0, 0);
        x += dw;
    }
}

/// Inventory selection, power-up timers and breath meter.
pub fn inventory_hud(frame: &mut [u32], w: usize, h: usize, res: &Resources, luts: &ShadeLuts, game: &Game) {
    use ob_game::inventory::{INV_ITEM_TYPES, INV_MAX, INV_NAMES};
    let inv = &game.inventory;
    let k = inv.selected;
    if inv.charge[k] > 0.0 {
        // Icon sits in the large box of the right status panel.
        if let Some(pic) = game.item_pic(INV_ITEM_TYPES[k]) {
            let t = res.tiles.get(pic);
            if !t.is_empty() {
                let zoom = (18.0 / t.width.max(t.height) as f64).min(1.0);
                let (iw, ih) = (t.width as f64 * zoom, t.height as f64 * zoom);
                vtile(
                    frame,
                    w,
                    h,
                    res,
                    luts,
                    pic,
                    288.0 - iw / 2.0,
                    188.0 - ih / 2.0,
                    Anchor::Right,
                    zoom,
                    0,
                    0,
                );
            }
        }
        let pct = (inv.charge[k] / INV_MAX[k] * 100.0).round() as i32;
        let on = inv.active[k] && k != 0;
        let label = format!("{} {}%{}", INV_NAMES[k], pct, if on { " ON" } else { "" });
        let tw = tile_text_width(res, 3, &label, 1.0);
        tile_text(
            frame,
            w,
            h,
            res,
            luts,
            3,
            &label,
            318.0 - tw,
            166.0,
            Anchor::Right,
            1.0,
            0,
            0,
        );
    }
    let mut y = 4.0;
    for (name, secs) in game.powerups.list() {
        let txt = format!("{name} {:.0}", secs.ceil());
        let tw = tile_text_width(res, 3, &txt, 1.0);
        tile_text(frame, w, h, res, luts, 3, &txt, 316.0 - tw, y, Anchor::Right, 1.0, 0, 0);
        y += 8.0;
    }
    if game.player.underwater && game.air < ob_game::game::AIR_SECONDS {
        let txt = if game.air > 0.0 {
            format!("AIR {:.0}", game.air.ceil())
        } else {
            "DROWNING".to_string()
        };
        let tw = tile_text_width(res, 1, &txt, 1.0);
        tile_text(
            frame,
            w,
            h,
            res,
            luts,
            1,
            &txt,
            160.0 - tw / 2.0,
            150.0,
            Anchor::Center,
            1.0,
            0,
            0,
        );
    }
}

/// The original-style compact status display (health/ammo boxes and key panel).
pub fn status_bar(frame: &mut [u32], w: usize, h: usize, res: &Resources, luts: &ShadeLuts, game: &Game) -> bool {
    const PANEL_LEFT: i32 = 2201;
    const PANEL_RIGHT: i32 = 2173;
    const RED_DIGITS: i32 = 2190;
    const BLUE_DIGITS: i32 = 2240;
    let left = res.tiles.get(PANEL_LEFT);
    if left.is_empty() || res.tiles.get(RED_DIGITS).is_empty() {
        return false;
    }
    let ly = 200.0 - left.height as f64;
    vtile(frame, w, h, res, luts, PANEL_LEFT, 0.0, ly, Anchor::Left, 1.0, 0, 0);
    status_number(
        frame,
        w,
        h,
        res,
        luts,
        RED_DIGITS,
        game.player.health,
        33.0,
        ly + 3.0,
        Anchor::Left,
    );
    let a = &game.arsenal;
    let def = a.def();
    if def.ammo != 0 {
        status_number(
            frame,
            w,
            h,
            res,
            luts,
            BLUE_DIGITS,
            a.ammo[def.ammo],
            66.0,
            ly + 3.0,
            Anchor::Left,
        );
    }
    let right = res.tiles.get(PANEL_RIGHT);
    if !right.is_empty() {
        let rx = 320.0 - right.width as f64;
        let ry = 200.0 - right.height as f64;
        vtile(frame, w, h, res, luts, PANEL_RIGHT, rx, ry, Anchor::Right, 1.0, 0, 0);
        // The six key icons (9x9 tiles 2220..2225; 2226 is a large status graphic, not a key)
        // sit in a row left of the right panel, so they cover neither its boxes nor the
        // inventory label above it.
        const KEY_ICON: i32 = 2220;
        const KEYS: usize = 6;
        for k in 0..KEYS {
            if game.player.keys[k] {
                vtile(
                    frame,
                    w,
                    h,
                    res,
                    luts,
                    KEY_ICON + k as i32,
                    rx - 2.0 - (KEYS - k) as f64 * 10.0,
                    200.0 - 11.0,
                    Anchor::Right,
                    1.0,
                    0,
                    0,
                );
            }
        }
    }
    true
}
