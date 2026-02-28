use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, DrawingArea, GestureClick, Grid, Orientation, Overlay};
use pangocairo::cairo;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::cell::RefCell;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

// ── RGB game model ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Channel { R, G, B }

impl Channel {
    fn next(self) -> Self {
        match self { Channel::R => Channel::G, Channel::G => Channel::B, Channel::B => Channel::R }
    }
    fn as_flag(self) -> RgbFlag {
        match self { Channel::R => RgbFlag::R, Channel::G => RgbFlag::G, Channel::B => RgbFlag::B }
    }
    fn mine_css(self) -> &'static str {
        match self { Channel::R => "cell-mine-r", Channel::G => "cell-mine-g", Channel::B => "cell-mine-b" }
    }
    fn btn_css(self) -> &'static str {
        match self { Channel::R => "channel-btn-r", Channel::G => "channel-btn-g", Channel::B => "channel-btn-b" }
    }
    fn label(self) -> &'static str {
        match self { Channel::R => "R", Channel::G => "G", Channel::B => "B" }
    }

}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RgbFlag { None, R, G, B }

#[derive(Clone, Copy, Debug)]
struct RgbCell {
    mine:       Option<Channel>,
    r_adj:      u8,        // adjacent R mines - computed for every cell, including mine cells
    g_adj:      u8,
    b_adj:      u8,
    revealed_r: bool,      // revealed while in R mode
    revealed_g: bool,
    revealed_b: bool,
    flag:       RgbFlag,
}

impl RgbCell {
    fn new() -> Self {
        RgbCell {
            mine: None,
            r_adj: 0, g_adj: 0, b_adj: 0,
            revealed_r: false, revealed_g: false, revealed_b: false,
            flag: RgbFlag::None,
        }
    }
    fn is_revealed(&self, ch: Channel) -> bool {
        match ch { Channel::R => self.revealed_r, Channel::G => self.revealed_g, Channel::B => self.revealed_b }
    }
    fn adj(&self, ch: Channel) -> u8 {
        match ch { Channel::R => self.r_adj, Channel::G => self.g_adj, Channel::B => self.b_adj }
    }
    fn revealed_any(&self) -> bool { self.revealed_r || self.revealed_g || self.revealed_b }
}

struct RgbGame {
    grid:         Vec<Vec<RgbCell>>,
    width:        usize,
    height:       usize,
    mine_count:   usize, // per channel
    state:        GameState,
    flags_r:      usize,
    flags_g:      usize,
    flags_b:      usize,
    view_channel: Channel,
}

impl RgbGame {
    fn new(width: usize, height: usize, mine_count: usize) -> Self {
        let per_ch = mine_count.min((width * height).saturating_sub(9) / 3);
        RgbGame {
            grid: vec![vec![RgbCell::new(); width]; height],
            width, height,
            mine_count: per_ch,
            state: GameState::Ready,
            flags_r: 0, flags_g: 0, flags_b: 0,
            view_channel: Channel::R,
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

        let n = self.mine_count;
        let m = positions.len();
        for &(x, y) in &positions[..n.min(m)] {
            self.grid[y][x].mine = Some(Channel::R);
        }
        for &(x, y) in &positions[n.min(m)..(2 * n).min(m)] {
            self.grid[y][x].mine = Some(Channel::G);
        }
        for &(x, y) in &positions[(2 * n).min(m)..(3 * n).min(m)] {
            self.grid[y][x].mine = Some(Channel::B);
        }

        // Compute adj for every cell - including mine cells, since they may be
        // safely revealed in another channel and should show that channel's count.
        for y in 0..self.height {
            for x in 0..self.width {
                let (r, g, b) = self.count_adj(x, y);
                self.grid[y][x].r_adj = r;
                self.grid[y][x].g_adj = g;
                self.grid[y][x].b_adj = b;
            }
        }
    }

    fn count_adj(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let (mut r, mut g, mut b) = (0u8, 0u8, 0u8);
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < self.width as isize && ny >= 0 && ny < self.height as isize {
                    match self.grid[ny as usize][nx as usize].mine {
                        Some(Channel::R) => r += 1,
                        Some(Channel::G) => g += 1,
                        Some(Channel::B) => b += 1,
                        None => {}
                    }
                }
            }
        }
        (r, g, b)
    }

    /// Reveal a cell in the active channel.
    /// Safe cells (no mine) → reveal all 3 channels at once.
    /// Another channel's mine → reveal only the current channel (safe, no death).
    /// Current channel's mine → death.
    fn reveal(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }
        if self.state == GameState::Ready {
            self.place_mines(x, y);
            self.state = GameState::Playing;
        }
        if self.state != GameState::Playing { return false; }

        let ch = self.view_channel;
        let cell = &self.grid[y][x];
        // Block only when fully revealed in all 3 channels, or flagged.
        // Allows clicking a cascade-revealed cell to fill in the other 2 channels.
        if (cell.revealed_r && cell.revealed_g && cell.revealed_b) || cell.flag != RgbFlag::None {
            return false;
        }

        // Direct click always reveals all 3 channels on this tile.
        self.grid[y][x].revealed_r = true;
        self.grid[y][x].revealed_g = true;
        self.grid[y][x].revealed_b = true;

        match self.grid[y][x].mine {
            Some(_) => {
                // Any mine is deadly - regardless of active channel.
                self.state = GameState::Lost;
                self.reveal_all_mines();
            }
            None => {
                // Safe cell - reveal all 3, cascade if current channel sees no adjacent mines.
                if self.grid[y][x].adj(ch) == 0 { self.cascade(x, y); }
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
                    // Cascade only reaches cells with NO mine of any channel.
                    // Mine cells (any colour) are borders, not interior.
                    // Since there's no mine to expose, it's safe to reveal all 3 channels.
                    if cell.mine.is_none() && !cell.is_revealed(ch) && cell.flag == RgbFlag::None {
                        self.grid[ny][nx].revealed_r = true;
                        self.grid[ny][nx].revealed_g = true;
                        self.grid[ny][nx].revealed_b = true;
                        if self.grid[ny][nx].adj(ch) == 0 {
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
    }

    /// Toggle the active channel's flag on a cell: None → channel_flag → None.
    /// Placing ANY flag colour on a mine whose colour doesn't match = instant death.
    fn toggle_flag(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }
        if self.state != GameState::Playing { return false; }

        let ch = self.view_channel;
        let ch_flag = ch.as_flag();
        let cell = &self.grid[y][x];
        if cell.is_revealed(ch) { return false; }

        let new_flag = match cell.flag {
            RgbFlag::None => ch_flag,
            f if f == ch_flag => RgbFlag::None,
            _ => return false, // another channel's flag - switch modes to touch it
        };

        if new_flag != RgbFlag::None {
            if let Some(mine_ch) = self.grid[y][x].mine {
                if mine_ch != ch {
                    // Wrong colour flag on a mine - instant death.
                    self.grid[y][x].flag = new_flag;
                    self.state = GameState::Lost;
                    self.reveal_all_mines();
                    return true;
                }
            }
        }

        match cell.flag {
            RgbFlag::R => self.flags_r -= 1,
            RgbFlag::G => self.flags_g -= 1,
            RgbFlag::B => self.flags_b -= 1,
            RgbFlag::None => {}
        }
        match new_flag {
            RgbFlag::R => self.flags_r += 1,
            RgbFlag::G => self.flags_g += 1,
            RgbFlag::B => self.flags_b += 1,
            RgbFlag::None => {}
        }
        self.grid[y][x].flag = new_flag;

        // Correctly flagging a mine reveals all 3 channels on that tile so the
        // other channels' adjacent counts become visible in the triangle.
        if new_flag != RgbFlag::None {
            if let Some(mine_ch) = self.grid[y][x].mine {
                if mine_ch == ch {
                    self.grid[y][x].revealed_r = true;
                    self.grid[y][x].revealed_g = true;
                    self.grid[y][x].revealed_b = true;
                }
            }
        }

        self.check_win();
        true
    }

    /// Chord in the active channel: if ch-flags around this cell == ch adj count,
    /// reveal all unflagged unrevealed-in-ch neighbours.
    fn chord(&mut self, x: usize, y: usize) -> bool {
        if self.state != GameState::Playing { return false; }
        let cell = &self.grid[y][x];
        // Must be fully revealed (direct clicks always reveal all 3).
        if !(cell.revealed_r && cell.revealed_g && cell.revealed_b) { return false; }
        let ch = self.view_channel;
        let adj = cell.adj(ch);
        if adj == 0 { return false; }

        let ch_flag = ch.as_flag();
        let mut flag_count = 0u8;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize { continue; }
                if self.grid[ny as usize][nx as usize].flag == ch_flag { flag_count += 1; }
            }
        }
        if flag_count != adj { return false; }

        // Every ch-flagged neighbour must actually be a ch-channel mine.
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize { continue; }
                let ncell = &self.grid[ny as usize][nx as usize];
                if ncell.flag == ch_flag && ncell.mine != Some(ch) { return false; }
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
                if ncell.flag == RgbFlag::None && !ncell.is_revealed(ch) {
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
                if cell.mine.is_some() {
                    cell.revealed_r = true;
                    cell.revealed_g = true;
                    cell.revealed_b = true;
                }
            }
        }
    }

    /// Win when every mine is correctly flagged and every non-mine cell is fully revealed.
    fn check_win(&mut self) {
        for row in &self.grid {
            for cell in row {
                match cell.mine {
                    Some(mine_ch) => {
                        if cell.flag != mine_ch.as_flag() { return; }
                    }
                    None => {
                        if !cell.revealed_r || !cell.revealed_g || !cell.revealed_b { return; }
                    }
                }
            }
        }
        self.state = GameState::Won;
    }

    fn sync_to(&self, game: &Rc<RefCell<Game>>) {
        let mut g = game.borrow_mut();
        g.state        = self.state;
        g.mine_count   = self.mine_count;
        g.flags_placed = match self.view_channel {
            Channel::R => self.flags_r,
            Channel::G => self.flags_g,
            Channel::B => self.flags_b,
        };
    }
}

// ── Board creation ─────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let rgb: Rc<RefCell<RgbGame>> = Rc::new(RefCell::new(RgbGame::new(width, height, mine_count)));

    // ── Channel selector button ───────────────────────────────────────────────
    let channel_btn = Button::with_label(Channel::R.label());
    channel_btn.add_css_class("channel-btn");
    channel_btn.add_css_class(Channel::R.btn_css());

    // ── Game grid with Overlay cells ─────────────────────────────────────────
    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    for y in 0..height {
        for x in 0..width {
            let cell = make_cell_overlay(x, y, ctx, &rgb);
            grid.attach(&cell, x as i32, y as i32, 1, 1);
        }
    }

    // ── Outer box: channel button on top, grid below ──────────────────────────
    let outer = GtkBox::new(Orientation::Vertical, 8);
    outer.set_halign(gtk4::Align::Center);
    outer.append(&channel_btn);
    outer.append(&grid);

    // ── Channel button: cycle R → G → B → R ──────────────────────────────────
    {
        let rgb_c   = rgb.clone();
        let game_c  = ctx.game.clone();
        let mine_c  = ctx.mine_label.clone();
        let board_c = ctx.board_widget.clone();
        let btn_c   = channel_btn.clone();
        channel_btn.connect_clicked(move |_| {
            let new_ch = {
                let mut r = rgb_c.borrow_mut();
                r.view_channel = r.view_channel.next();
                r.view_channel
            };
            btn_c.remove_css_class("channel-btn-r");
            btn_c.remove_css_class("channel-btn-g");
            btn_c.remove_css_class("channel-btn-b");
            btn_c.add_css_class(new_ch.btn_css());
            btn_c.set_label(new_ch.label());
            rgb_c.borrow().sync_to(&game_c);
            // Re-render so button backgrounds reflect the new active channel.
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
                render_cell_widget(&widget, &rgb.grid[y][x], rgb.view_channel);
            }
        }
    }
}

fn render_cell_widget(widget: &gtk4::Widget, cell: &RgbCell, ch: Channel) {
    if let Some(overlay) = widget.downcast_ref::<Overlay>() {
        if let Some(btn) = overlay.first_child().and_then(|w| w.downcast::<Button>().ok()) {
            render_cell_button(&btn, cell, ch);
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

/// Update the Button's CSS and label.
///
/// The only time the button itself shows a mine is when this channel's mine has
/// been triggered (you just died) - it fills the whole button to make it obvious.
/// Every other revealed state is handled by the DrawingArea triangle.
fn render_cell_button(btn: &Button, cell: &RgbCell, ch: Channel) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine-r");
    btn.remove_css_class("cell-mine-g");
    btn.remove_css_class("cell-mine-b");

    if let Some(mine_ch) = cell.mine {
        if cell.revealed_any() {
            btn.add_css_class("cell-revealed");
            if cell.flag != mine_ch.as_flag() {
                // Death-revealed mine (not correctly flagged) - colour by channel.
                btn.add_css_class(mine_ch.mine_css());
            }
            // Correctly flagged mine: plain grey background; flag + numbers show in triangle.
        }
    } else if cell.is_revealed(ch) {
        btn.add_css_class("cell-revealed");
    }
    btn.set_label(" "); // DA draws flags, numbers, and mines in the triangle
}

// ── Pangocairo triangle drawing ───────────────────────────────────────────────

/// Render 🚩 onto a tiny ImageSurface, rotate every pixel's hue by `degrees`,
/// then paint the result centred at (cx, cy). This matches CSS hue-rotate().
fn draw_flag_hue_shifted(cr: &cairo::Context, pctx: &pango::Context, cx: f64, cy: f64, degrees: f64) {
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
    } // tc dropped → surface flushed

    if degrees.abs() > 1.0 {
        let stride = surf.stride() as usize;
        if let Ok(mut data) = surf.data() {
            for y in 0..sz as usize {
                for x in 0..sz as usize {
                    let i = y * stride + x * 4;
                    let a = data[i + 3];
                    if a == 0 { continue; }
                    let af = a as f64 / 255.0;
                    // Cairo ARGB32 (premultiplied): byte order B G R A
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
    // reset source so subsequent strokes/fills aren't affected
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
        if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t }
        else if t < 0.5   { q }
        else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 }
        else { p }
    }
    (h2rgb(p, q, h + 1.0 / 3.0), h2rgb(p, q, h), h2rgb(p, q, h - 1.0 / 3.0))
}

/// Draw the triangle for one cell.
///   R corner - top-centre
///   G corner - bottom-left
///   B corner - bottom-right
///
/// Each corner shows either:
///   • a 💣 emoji  - if this channel's mine lives here (visible once the cell
///                    has been touched in any mode)
///   • an adj count - if the channel has been revealed here (and it's not a mine)
fn draw_rgb_triangle(cr: &cairo::Context, pctx: &pango::Context, w: i32, h: i32, cell: &RgbCell) {
    let w = w as f64;
    let h = h as f64;

    let corners: [(Channel, f64, f64, (f64, f64, f64)); 3] = [
        (Channel::R, 0.50, 0.26, (0.867, 0.267, 0.267)),
        (Channel::G, 0.22, 0.74, (0.267, 0.867, 0.267)),
        (Channel::B, 0.78, 0.74, (0.40,  0.533, 1.0  )),
    ];

    for (ch, rx, ry, (cr_r, cr_g, cr_b)) in corners {
        let cx = rx * w;
        let cy = ry * h;

        if cell.flag == ch.as_flag() {
            // Flag for this channel lives in this corner - hue-shifted to match.
            let hue_shift = match ch { Channel::R => 0.0, Channel::G => 120.0, Channel::B => 240.0 };
            draw_flag_hue_shifted(cr, pctx, cx, cy, hue_shift);
        } else if cell.mine == Some(ch) {
            // Mine lives in this corner - only show it when revealed in THIS channel
            // (death, or reveal_all_mines after death). Stepping on it in another
            // channel's mode should not expose its position.
            if !cell.is_revealed(ch) { continue; }
            let layout = pango::Layout::new(pctx);
            layout.set_font_description(Some(&pango::FontDescription::from_string("emoji 7")));
            layout.set_text("\u{1F4A3}");
            let (pw, ph) = layout.pixel_size();
            cr.move_to(cx - pw as f64 / 2.0, cy - ph as f64 / 2.0);
            pangocairo::functions::show_layout(cr, &layout);
        } else if cell.is_revealed(ch) {
            // Revealed in this channel - draw adj count (skip 0).
            let count = cell.adj(ch);
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
) -> Overlay {
    let overlay = Overlay::new();

    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click - chord if fully revealed, otherwise reveal
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
                if cell.revealed_r && cell.revealed_g && cell.revealed_b {
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

    // Right click - toggle active channel's flag (None ↔ channel flag)
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

    // DrawingArea: draws the per-channel triangle of revealed numbers
    let da = DrawingArea::new();
    da.set_can_target(false);
    {
        let rgb_c = rgb.clone();
        da.set_draw_func(move |da, cr, w, h| {
            let game = rgb_c.borrow();
            let cell = &game.grid[y][x];
            if cell.revealed_any() || cell.flag != RgbFlag::None {
                draw_rgb_triangle(cr, &da.pango_context(), w, h, cell);
            }
        });
    }
    overlay.add_overlay(&da);
    overlay.set_measure_overlay(&da, false);

    overlay
}
