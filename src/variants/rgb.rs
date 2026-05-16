use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, DrawingArea, GestureClick, Grid, Orientation, Overlay};
use pangocairo::cairo;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

thread_local! {
    static N_COLOURS: Cell<usize> = const { Cell::new(3) };
}

pub fn set_custom_n_colours(n: usize) {
    N_COLOURS.with(|c| c.set(n.clamp(2, 20)));
}

fn current_n_colours() -> usize {
    N_COLOURS.with(|c| c.get())
}

// ── RGB game model ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct RgbCell {
    mine:     Option<u8>,
    adj:      Vec<u8>,
    revealed: Vec<bool>,
    flag:     Option<u8>,
}

impl RgbCell {
    fn new(n: usize) -> Self {
        RgbCell { mine: None, adj: vec![0; n], revealed: vec![false; n], flag: None }
    }
    fn is_revealed(&self, ch: usize) -> bool { self.revealed[ch] }
    fn adj_ch(&self, ch: usize) -> u8 { self.adj[ch] }
    fn revealed_any(&self) -> bool { self.revealed.iter().any(|&r| r) }
    fn all_revealed(&self) -> bool { self.revealed.iter().all(|&r| r) }
    fn set_all_revealed(&mut self) { for r in &mut self.revealed { *r = true; } }
}

struct RgbGame {
    grid:         Vec<Vec<RgbCell>>,
    width:        usize,
    height:       usize,
    n:            usize,
    mine_count:   usize,
    state:        GameState,
    flags:        Vec<usize>,
    view_channel: usize,
}

impl RgbGame {
    fn new(width: usize, height: usize, mine_count: usize, n: usize) -> Self {
        let per_ch = mine_count.min((width * height).saturating_sub(9) / n.max(1));
        RgbGame {
            grid: vec![vec![RgbCell::new(n); width]; height],
            width, height, n,
            mine_count: per_ch,
            state: GameState::Ready,
            flags: vec![0; n],
            view_channel: 0,
        }
    }

    fn place_mines(&mut self, first_x: usize, first_y: usize) {
        let mut rng = thread_rng();
        let mut positions: Vec<(usize, usize)> = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                (x as isize - first_x as isize).abs() > 1
                    || (y as isize - first_y as isize).abs() > 1
            })
            .collect();
        positions.shuffle(&mut rng);

        let m = positions.len();
        let per = self.mine_count;
        for ch in 0..self.n {
            let start = ch * per;
            let end = ((ch + 1) * per).min(m);
            for &(x, y) in &positions[start..end] {
                self.grid[y][x].mine = Some(ch as u8);
            }
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let counts = self.count_adj(x, y);
                self.grid[y][x].adj = counts;
            }
        }
    }

    fn count_adj(&self, x: usize, y: usize) -> Vec<u8> {
        let mut counts = vec![0u8; self.n];
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < self.width as isize && ny >= 0 && ny < self.height as isize {
                    if let Some(ch) = self.grid[ny as usize][nx as usize].mine {
                        counts[ch as usize] += 1;
                    }
                }
            }
        }
        counts
    }

    fn reveal(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }
        if self.state == GameState::Ready {
            self.place_mines(x, y);
            self.state = GameState::Playing;
        }
        if self.state != GameState::Playing { return false; }

        let ch = self.view_channel;
        let cell = &self.grid[y][x];
        if cell.all_revealed() || cell.flag.is_some() { return false; }

        self.grid[y][x].set_all_revealed();

        match self.grid[y][x].mine {
            Some(_) => {
                self.state = GameState::Lost;
                self.reveal_all_mines();
            }
            None => {
                if self.grid[y][x].adj_ch(ch) == 0 { self.cascade(x, y); }
                self.check_win();
            }
        }
        true
    }

    fn cascade(&mut self, sx: usize, sy: usize) {
        let ch = self.view_channel;
        let mut stack = vec![(sx, sy)];
        while let Some((cx, cy)) = stack.pop() {
            for dy in -1_isize..=1 {
                for dx in -1_isize..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = cx as isize + dx;
                    let ny = cy as isize + dy;
                    if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize { continue; }
                    let (nx, ny) = (nx as usize, ny as usize);
                    let cell = &self.grid[ny][nx];
                    if cell.mine.is_none() && !cell.is_revealed(ch) && cell.flag.is_none() {
                        self.grid[ny][nx].set_all_revealed();
                        if self.grid[ny][nx].adj_ch(ch) == 0 {
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
    }

    fn toggle_flag(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }
        if self.state != GameState::Playing { return false; }

        let ch = self.view_channel;
        let ch_u8 = ch as u8;
        let cell = &self.grid[y][x];
        if cell.is_revealed(ch) { return false; }

        let new_flag = match cell.flag {
            None => Some(ch_u8),
            Some(f) if f == ch_u8 => None,
            _ => return false,
        };

        if let Some(nf) = new_flag {
            if let Some(mine_ch) = self.grid[y][x].mine {
                if mine_ch != nf {
                    self.grid[y][x].flag = new_flag;
                    self.state = GameState::Lost;
                    self.reveal_all_mines();
                    return true;
                }
            }
        }

        if let Some(old_f) = self.grid[y][x].flag { self.flags[old_f as usize] -= 1; }
        if let Some(new_f) = new_flag { self.flags[new_f as usize] += 1; }
        self.grid[y][x].flag = new_flag;

        if let Some(nf) = new_flag {
            if let Some(mine_ch) = self.grid[y][x].mine {
                if mine_ch == nf { self.grid[y][x].set_all_revealed(); }
            }
        }

        self.check_win();
        true
    }

    fn chord(&mut self, x: usize, y: usize) -> bool {
        if self.state != GameState::Playing { return false; }
        let cell = &self.grid[y][x];
        if !cell.all_revealed() { return false; }
        let ch = self.view_channel;
        let adj = cell.adj_ch(ch);
        if adj == 0 { return false; }

        let ch_u8 = ch as u8;
        let mut flag_count = 0u8;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize { continue; }
                if self.grid[ny as usize][nx as usize].flag == Some(ch_u8) { flag_count += 1; }
            }
        }
        if flag_count != adj { return false; }

        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize { continue; }
                let ncell = &self.grid[ny as usize][nx as usize];
                if ncell.flag == Some(ch_u8) && ncell.mine != Some(ch_u8) { return false; }
            }
        }

        let mut changed = false;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize { continue; }
                let (nx, ny) = (nx as usize, ny as usize);
                let ncell = &self.grid[ny][nx];
                if ncell.flag.is_none() && !ncell.is_revealed(ch) {
                    self.reveal(nx, ny);
                    changed = true;
                }
            }
        }
        changed
    }

    fn reveal_all_mines(&mut self) {
        for row in &mut self.grid {
            for cell in row {
                if cell.mine.is_some() { cell.set_all_revealed(); }
            }
        }
    }

    fn check_win(&mut self) {
        for row in &self.grid {
            for cell in row {
                match cell.mine {
                    Some(mine_ch) => { if cell.flag != Some(mine_ch) { return; } }
                    None => { if !cell.all_revealed() { return; } }
                }
            }
        }
        self.state = GameState::Won;
    }

    fn sync_to(&self, game: &Rc<RefCell<Game>>) {
        let mut g = game.borrow_mut();
        g.state        = self.state;
        g.mine_count   = self.mine_count;
        g.flags_placed = self.flags[self.view_channel];
    }
}

// ── Colour and layout helpers ──────────────────────────────────────────────────

fn hsl_to_rgb_f(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    if s < 1e-10 { return (l, l, l); }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    fn hc(p: f64, q: f64, t: f64) -> f64 {
        let t = t.rem_euclid(1.0);
        if t < 1.0/6.0 { p + (q-p)*6.0*t }
        else if t < 0.5 { q }
        else if t < 2.0/3.0 { p + (q-p)*(2.0/3.0-t)*6.0 }
        else { p }
    }
    (hc(p, q, h + 1.0/3.0), hc(p, q, h), hc(p, q, h - 1.0/3.0))
}

/// Hue [0, 1) for channel ch in an n-channel palette.
/// For n≤7 uses ROYGBIV-anchored positions so labels match the actual colour.
/// For n>7 falls back to even spacing.
fn channel_hue(ch: usize, n: usize) -> f64 {
    // Hue fractions: R=0°  O=30°  Y=60°  G=120°  B=240°  I=270°  V=300°
    const N2: &[f64] = &[0.000, 0.667];
    const N3: &[f64] = &[0.000, 0.333, 0.667];
    const N4: &[f64] = &[0.000, 0.167, 0.333, 0.667];
    const N5: &[f64] = &[0.000, 0.167, 0.333, 0.667, 0.833];
    const N6: &[f64] = &[0.000, 0.083, 0.167, 0.333, 0.667, 0.833];
    const N7: &[f64] = &[0.000, 0.083, 0.167, 0.333, 0.667, 0.750, 0.833];
    match n {
        2 => N2[ch], 3 => N3[ch], 4 => N4[ch],
        5 => N5[ch], 6 => N6[ch], 7 => N7[ch],
        _ => ch as f64 / n as f64,
    }
}

fn channel_colour(ch: usize, n: usize) -> (f64, f64, f64) {
    hsl_to_rgb_f(channel_hue(ch, n), 0.80, 0.55)
}

fn channel_colour_dark(ch: usize, n: usize) -> (f64, f64, f64) {
    hsl_to_rgb_f(channel_hue(ch, n), 0.60, 0.35)
}

/// Position for channel ch's corner in an N-gon layout, relative to (w, h).
/// Starts at top, goes clockwise. Centre shifted slightly down for visual balance.
fn channel_pos(ch: usize, n: usize, w: f64, h: f64) -> (f64, f64) {
    use std::f64::consts::PI;
    let angle = 2.0 * PI * ch as f64 / n as f64 - PI / 2.0;
    let r = if n <= 3 { 0.28 } else { 0.26 };
    (w * (0.5 + r * angle.cos()), h * (0.53 + r * angle.sin()))
}

fn channel_label(ch: usize, n: usize) -> String {
    match n {
        2 => ["R", "B"][ch].to_string(),
        3 => ["R", "G", "B"][ch].to_string(),
        4 => ["R", "Y", "G", "B"][ch].to_string(),
        5 => ["R", "Y", "G", "B", "V"][ch].to_string(),
        6 => ["R", "O", "Y", "G", "B", "V"][ch].to_string(),
        7 => ["R", "O", "Y", "G", "B", "I", "V"][ch].to_string(),
        _ => (ch + 1).to_string(),
    }
}

/// Generate CSS for mine backgrounds and the active channel button.
fn build_rgb_css(active_ch: usize, n: usize) -> String {
    let mut css = String::new();
    for ch in 0..n {
        let (r, g, b) = channel_colour(ch, n);
        let (ri, gi, bi) = ((r*255.0) as u8, (g*255.0) as u8, (b*255.0) as u8);
        css.push_str(&format!(
            ".cell-mine-rgb-{ch} {{ background-image: linear-gradient(rgba({ri},{gi},{bi},0.35), rgba({ri},{gi},{bi},0.35)); }}\n"
        ));
    }
    let (r, g, b) = channel_colour(active_ch, n);
    let (ri, gi, bi) = ((r*255.0) as u8, (g*255.0) as u8, (b*255.0) as u8);
    css.push_str(&format!(
        "button.channel-btn {{ background-color: rgba({ri},{gi},{bi},0.18); background-image: none; color: rgb({ri},{gi},{bi}); }}\n"
    ));
    css
}

// ── Board creation ─────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };
    let n = current_n_colours();

    let rgb: Rc<RefCell<RgbGame>> = Rc::new(RefCell::new(RgbGame::new(width, height, mine_count, n)));

    // ── Per-game CSS provider (mine colours + channel button colour) ──────────
    let rgb_css = Rc::new(gtk4::CssProvider::new());
    rgb_css.load_from_data(&build_rgb_css(0, n));
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &*rgb_css,
            gtk4::STYLE_PROVIDER_PRIORITY_USER + 1,
        );
    }

    // ── Channel selector button ───────────────────────────────────────────────
    let channel_btn = Button::with_label(&channel_label(0, n));
    channel_btn.add_css_class("channel-btn");

    // ── Game grid ─────────────────────────────────────────────────────────────
    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    for y in 0..height {
        for x in 0..width {
            let cell = make_cell_overlay(x, y, ctx, &rgb, n);
            grid.attach(&cell, x as i32, y as i32, 1, 1);
        }
    }

    let outer = GtkBox::new(Orientation::Vertical, 8);
    outer.set_halign(gtk4::Align::Center);
    outer.append(&channel_btn);
    outer.append(&grid);

    // ── Channel button: cycle through all N channels ──────────────────────────
    {
        let rgb_c    = rgb.clone();
        let game_c   = ctx.game.clone();
        let mine_c   = ctx.mine_label.clone();
        let board_c  = ctx.board_widget.clone();
        let btn_c    = channel_btn.clone();
        let css_c    = rgb_css.clone();
        channel_btn.connect_clicked(move |_| {
            let new_ch = {
                let mut r = rgb_c.borrow_mut();
                r.view_channel = (r.view_channel + 1) % n;
                r.view_channel
            };
            css_c.load_from_data(&build_rgb_css(new_ch, n));
            btn_c.set_label(&channel_label(new_ch, n));
            rgb_c.borrow().sync_to(&game_c);
            if let Some(b) = board_c.borrow().as_ref() {
                render_board(&rgb_c.borrow(), b);
            }
            update_mine_counter(&game_c, &mine_c);
        });
    }

    outer.upcast()
}

#[allow(dead_code)]
pub fn update_board(_game: &Rc<RefCell<Game>>, _board: &gtk4::Widget) {}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_board(rgb: &RgbGame, board: &gtk4::Widget) {
    let grid = match find_grid(board) {
        Some(g) => g,
        None => return,
    };
    for y in 0..rgb.height {
        for x in 0..rgb.width {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                render_cell_widget(&widget, &rgb.grid[y][x], rgb.view_channel, rgb.n);
            }
        }
    }
}

fn render_cell_widget(widget: &gtk4::Widget, cell: &RgbCell, ch: usize, n: usize) {
    if let Some(overlay) = widget.downcast_ref::<Overlay>() {
        if let Some(btn) = overlay.first_child().and_then(|w| w.downcast::<Button>().ok()) {
            render_cell_button(&btn, cell, ch, n);
        }
        if let Some(da) = overlay.first_child()
            .and_then(|w| w.next_sibling())
            .and_then(|w| w.downcast::<DrawingArea>().ok())
        {
            da.queue_draw();
        }
    }
}

fn find_grid(widget: &gtk4::Widget) -> Option<Grid> {
    if let Some(g) = widget.downcast_ref::<Grid>() {
        return Some(g.clone());
    }
    if let Some(b) = widget.downcast_ref::<GtkBox>() {
        let mut child = b.first_child();
        while let Some(c) = child {
            if let Some(g) = c.downcast_ref::<Grid>() {
                return Some(g.clone());
            }
            child = c.next_sibling();
        }
    }
    None
}

fn render_cell_button(btn: &Button, cell: &RgbCell, ch: usize, n: usize) {
    btn.remove_css_class("cell-revealed");
    for i in 0..n {
        btn.remove_css_class(&format!("cell-mine-rgb-{i}"));
    }

    if let Some(mine_ch) = cell.mine {
        if cell.revealed_any() {
            btn.add_css_class("cell-revealed");
            if cell.flag != Some(mine_ch) {
                btn.add_css_class(&format!("cell-mine-rgb-{mine_ch}"));
            }
        }
    } else if cell.is_revealed(ch) {
        btn.add_css_class("cell-revealed");
    }
    btn.set_label(" ");
}

// ── Cairo cell drawing ────────────────────────────────────────────────────────

/// Render 🚩 hue-shifted to the given channel.
fn draw_flag_hue_shifted(cr: &cairo::Context, pctx: &pango::Context, cx: f64, cy: f64, ch: usize, n: usize) {
    let degrees = channel_hue(ch, n) * 360.0;
    let pt = 7.0_f64;
    let sz = (pt * 3.0) as i32;

    let Ok(mut surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, sz, sz) else { return };
    {
        let Ok(tc) = cairo::Context::new(&surf) else { return };
        let layout = pango::Layout::new(pctx);
        layout.set_font_description(Some(&pango::FontDescription::from_string("emoji 7")));
        layout.set_text("\u{1F6A9}");
        let (pw, ph) = layout.pixel_size();
        tc.move_to((sz as f64 - pw as f64) / 2.0, (sz as f64 - ph as f64) / 2.0);
        pangocairo::functions::show_layout(&tc, &layout);
    }

    if degrees.abs() > 1.0 {
        let stride = surf.stride() as usize;
        if let Ok(mut data) = surf.data() {
            for y in 0..sz as usize {
                for x in 0..sz as usize {
                    let i = y * stride + x * 4;
                    let a = data[i + 3];
                    if a == 0 { continue; }
                    let af = a as f64 / 255.0;
                    let b = (data[i    ] as f64 / 255.0 / af).clamp(0.0, 1.0);
                    let g = (data[i + 1] as f64 / 255.0 / af).clamp(0.0, 1.0);
                    let r = (data[i + 2] as f64 / 255.0 / af).clamp(0.0, 1.0);
                    let (r2, g2, b2) = hsl_hue_rotate(r, g, b, degrees);
                    data[i    ] = ((b2 * af) * 255.0).round() as u8;
                    data[i + 1] = ((g2 * af) * 255.0).round() as u8;
                    data[i + 2] = ((r2 * af) * 255.0).round() as u8;
                }
            }
        }
    }

    let _ = cr.set_source_surface(&surf, cx - sz as f64 / 2.0, cy - sz as f64 / 2.0);
    let _ = cr.paint();
    cr.set_source_rgb(0.0, 0.0, 0.0);
}

fn hsl_hue_rotate(r: f64, g: f64, b: f64, degrees: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-10 { return (r, g, b); }

    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    let h = (h / 6.0 + degrees / 360.0).rem_euclid(1.0);

    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    fn h2rgb(p: f64, q: f64, t: f64) -> f64 {
        let t = t.rem_euclid(1.0);
        if t < 1.0/6.0 { p + (q-p)*6.0*t }
        else if t < 0.5 { q }
        else if t < 2.0/3.0 { p + (q-p)*(2.0/3.0-t)*6.0 }
        else { p }
    }
    (h2rgb(p, q, h + 1.0/3.0), h2rgb(p, q, h), h2rgb(p, q, h - 1.0/3.0))
}

/// Draw the N-gon cell: each channel gets one corner of a regular N-gon.
fn draw_rgb_cell(cr: &cairo::Context, pctx: &pango::Context, w: i32, h: i32, cell: &RgbCell, n: usize) {
    let (wf, hf) = (w as f64, h as f64);

    for ch in 0..n {
        let (cx, cy) = channel_pos(ch, n, wf, hf);
        let (cr_r, cr_g, cr_b) = hsl_to_rgb_f(channel_hue(ch, n), 0.80, 0.65);

        if cell.flag == Some(ch as u8) {
            draw_flag_hue_shifted(cr, pctx, cx, cy, ch, n);
        } else if cell.mine == Some(ch as u8) {
            if !cell.is_revealed(ch) { continue; }
            let layout = pango::Layout::new(pctx);
            layout.set_font_description(Some(&pango::FontDescription::from_string("emoji 7")));
            layout.set_text("\u{1F4A3}");
            let (pw, ph) = layout.pixel_size();
            cr.move_to(cx - pw as f64 / 2.0, cy - ph as f64 / 2.0);
            pangocairo::functions::show_layout(cr, &layout);
        } else if cell.is_revealed(ch) {
            let count = cell.adj_ch(ch);
            if count == 0 { continue; }
            cr.set_source_rgb(cr_r, cr_g, cr_b);
            let layout = pango::Layout::new(pctx);
            layout.set_font_description(Some(&pango::FontDescription::from_string("Sans Bold 8")));
            layout.set_text(&count.to_string());
            let (pw, ph) = layout.pixel_size();
            cr.move_to(cx - pw as f64 / 2.0, cy - ph as f64 / 2.0);
            pangocairo::functions::show_layout(cr, &layout);
        }
    }
}

// ── Cell widget factory ───────────────────────────────────────────────────────

fn make_cell_overlay(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    rgb: &Rc<RefCell<RgbGame>>,
    n: usize,
) -> Overlay {
    let overlay = Overlay::new();

    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click: chord if fully revealed, otherwise reveal
    {
        let rgb_c    = rgb.clone();
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let timer_c  = ctx.timer_label.clone();
        let face_c   = ctx.face_button.clone();
        let start_c  = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();
        btn.connect_clicked(move |_| {
            {
                let r = rgb_c.borrow();
                let cell = &r.grid[y][x];
                if cell.all_revealed() {
                    drop(r);
                    if !rgb_c.borrow_mut().chord(x, y) { return; }
                    rgb_c.borrow().sync_to(&game_c);
                    if let Some(b) = board_c.borrow().as_ref() {
                        render_board(&rgb_c.borrow(), b);
                    }
                    update_mine_counter(&game_c, &mine_c);
                    update_face(&game_c, &face_c, &source_c);
                    return;
                }
            }
            let was_ready = rgb_c.borrow().state == GameState::Ready;
            rgb_c.borrow_mut().reveal(x, y);
            rgb_c.borrow().sync_to(&game_c);
            if was_ready && rgb_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }
            if let Some(b) = board_c.borrow().as_ref() {
                render_board(&rgb_c.borrow(), b);
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click: toggle active channel's flag
    {
        let right    = GestureClick::new();
        right.set_button(3);
        let rgb_c    = rgb.clone();
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            rgb_c.borrow_mut().toggle_flag(x, y);
            rgb_c.borrow().sync_to(&game_c);
            if let Some(b) = board_c.borrow().as_ref() {
                render_board(&rgb_c.borrow(), b);
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        btn.add_controller(right);
    }

    overlay.set_child(Some(&btn));

    let da = DrawingArea::new();
    da.set_can_target(false);
    {
        let rgb_c = rgb.clone();
        da.set_draw_func(move |da, cr, w, h| {
            let game = rgb_c.borrow();
            let cell = &game.grid[y][x];
            if cell.revealed_any() || cell.flag.is_some() {
                draw_rgb_cell(cr, &da.pango_context(), w, h, cell, n);
            }
        });
    }
    overlay.add_overlay(&da);
    overlay.set_measure_overlay(&da, false);

    overlay
}
