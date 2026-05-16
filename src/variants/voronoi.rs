use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{DrawingArea, GestureClick};

use crate::game::{CellState, GameState};
use super::{BoardContext, start_timer, stop_timer};

// ── Voronoi diagram via Sutherland-Hodgman half-plane clipping ─────────────────

fn sq_dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    dx * dx + dy * dy
}

/// True iff p is in the half-plane "closer to pi than pj".
fn in_half_plane(p: (f64, f64), pi: (f64, f64), pj: (f64, f64)) -> bool {
    let (dx, dy) = (pi.0 - pj.0, pi.1 - pj.1);
    let c = (pi.0 * pi.0 + pi.1 * pi.1 - pj.0 * pj.0 - pj.1 * pj.1) * 0.5;
    p.0 * dx + p.1 * dy >= c
}

fn bisector_intersect(a: (f64, f64), b: (f64, f64), pi: (f64, f64), pj: (f64, f64)) -> (f64, f64) {
    let n = (pi.0 - pj.0, pi.1 - pj.1);
    let m = ((pi.0 + pj.0) * 0.5, (pi.1 + pj.1) * 0.5);
    let d = (b.0 - a.0, b.1 - a.1);
    let denom = d.0 * n.0 + d.1 * n.1;
    if denom.abs() < 1e-12 {
        return ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    }
    let t = ((m.0 - a.0) * n.0 + (m.1 - a.1) * n.1) / denom;
    let t = t.clamp(0.0, 1.0);
    (a.0 + t * d.0, a.1 + t * d.1)
}

/// Sutherland-Hodgman clip of `poly` to the half-plane "closer to pi than pj".
/// Returns (clipped_poly, was_clipped).  was_clipped is true if the bisector
/// actually intersected the polygon, meaning i and j may be Voronoi neighbors.
fn clip_poly(poly: &[(f64, f64)], pi: (f64, f64), pj: (f64, f64)) -> (Vec<(f64, f64)>, bool) {
    if poly.len() < 3 { return (poly.to_vec(), false); }
    let mut out = Vec::new();
    let mut clipped = false;
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let a_in = in_half_plane(a, pi, pj);
        let b_in = in_half_plane(b, pi, pj);
        match (a_in, b_in) {
            (true,  true)  => out.push(b),
            (true,  false) => { out.push(bisector_intersect(a, b, pi, pj)); clipped = true; }
            (false, true)  => { out.push(bisector_intersect(a, b, pi, pj)); out.push(b); clipped = true; }
            (false, false) => {}
        }
    }
    (out, clipped)
}

fn polygon_centroid(poly: &[(f64, f64)]) -> (f64, f64) {
    let n = poly.len();
    if n == 0 { return (0.5, 0.5); }
    let mut area = 0.0_f64;
    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;
    for i in 0..n {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        area += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    area *= 0.5;
    if area.abs() < 1e-10 {
        let sx = poly.iter().map(|p| p.0).sum::<f64>() / n as f64;
        let sy = poly.iter().map(|p| p.1).sum::<f64>() / n as f64;
        return (sx, sy);
    }
    (cx / (6.0 * area), cy / (6.0 * area))
}

fn compute_voronoi_polys(sites: &[(f64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let n = sites.len();
    let bbox = vec![(0.0_f64, 0.0_f64), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let mut polys: Vec<Vec<(f64, f64)>> = vec![bbox; n];
    for i in 0..n {
        let pi = sites[i];
        for j in 0..n {
            if i == j { continue; }
            if polys[i].is_empty() { break; }
            let (clipped, _) = clip_poly(&polys[i], pi, sites[j]);
            polys[i] = clipped;
        }
    }
    polys
}

/// Raster scan: assign each pixel to its nearest site, then mark pairs of
/// sites that own adjacent pixels as Voronoi neighbors.  Exact and fast.
fn compute_adjacency_raster(sites: &[(f64, f64)]) -> Vec<Vec<usize>> {
    let n = sites.len();
    let res = ((n as f64).sqrt() as usize * 12).clamp(80, 400);

    let mut grid = vec![0usize; res * res];
    for row in 0..res {
        let y = (row as f64 + 0.5) / res as f64;
        for col in 0..res {
            let x = (col as f64 + 0.5) / res as f64;
            grid[row * res + col] = nearest_site(sites, x, y);
        }
    }

    let mut adj_set: Vec<std::collections::HashSet<usize>> =
        vec![std::collections::HashSet::new(); n];
    for row in 0..res {
        for col in 0..res {
            let ci = grid[row * res + col];
            if col + 1 < res {
                let cj = grid[row * res + col + 1];
                if ci != cj { adj_set[ci].insert(cj); adj_set[cj].insert(ci); }
            }
            if row + 1 < res {
                let cj = grid[(row + 1) * res + col];
                if ci != cj { adj_set[ci].insert(cj); adj_set[cj].insert(ci); }
            }
        }
    }
    adj_set.into_iter().map(|s| s.into_iter().collect()).collect()
}

fn lloyd_relax(sites: &mut Vec<(f64, f64)>) {
    let polys = compute_voronoi_polys(sites);
    for (i, poly) in polys.iter().enumerate() {
        if poly.len() >= 3 {
            sites[i] = polygon_centroid(poly);
        }
    }
}

fn nearest_site(sites: &[(f64, f64)], px: f64, py: f64) -> usize {
    sites.iter().enumerate()
        .min_by(|(_, a), (_, b)| {
            sq_dist(**a, (px, py)).partial_cmp(&sq_dist(**b, (px, py))).unwrap()
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ── Game logic ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct VCell {
    is_mine: bool,
    state:   CellState,
    adjacent: u8,
}

impl VCell {
    fn new() -> Self { VCell { is_mine: false, state: CellState::Hidden, adjacent: 0 } }
}

struct GameVor {
    cells:          Vec<VCell>,
    adj:            Vec<Vec<usize>>,
    sites:          Vec<(f64, f64)>,
    polys:          Vec<Vec<(f64, f64)>>,
    state:          GameState,
    mines_total:    usize,
    flags_placed:   usize,
    cells_revealed: usize,
    total_safe:     usize,
}

impl GameVor {
    fn new(
        sites: Vec<(f64, f64)>,
        polys: Vec<Vec<(f64, f64)>>,
        adj:   Vec<Vec<usize>>,
        mines: usize,
    ) -> Self {
        let n = sites.len();
        let mines = mines.clamp(1, n.saturating_sub(9));
        GameVor {
            cells:          vec![VCell::new(); n],
            adj, sites, polys,
            state:          GameState::Ready,
            mines_total:    mines,
            flags_placed:   0,
            cells_revealed: 0,
            total_safe:     n - mines,
        }
    }

    fn remaining_mines(&self) -> i32 {
        self.mines_total as i32 - self.flags_placed as i32
    }

    fn place_mines(&mut self, first: usize) {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let n = self.cells.len();
        let safe: std::collections::HashSet<usize> =
            std::iter::once(first).chain(self.adj[first].iter().copied()).collect();
        let mut placed = 0;
        while placed < self.mines_total {
            let i = rng.gen_range(0..n);
            if self.cells[i].is_mine || safe.contains(&i) { continue; }
            self.cells[i].is_mine = true;
            placed += 1;
        }
        for i in 0..n {
            if self.cells[i].is_mine { continue; }
            self.cells[i].adjacent = self.adj[i].iter()
                .filter(|&&j| self.cells[j].is_mine)
                .count() as u8;
        }
    }

    fn reveal(&mut self, idx: usize) {
        if self.state == GameState::Ready {
            self.place_mines(idx);
            self.state = GameState::Playing;
        }
        if !matches!(self.state, GameState::Playing) { return; }
        if self.cells[idx].state != CellState::Hidden { return; }
        if self.cells[idx].is_mine {
            self.state = GameState::Lost;
            for c in &mut self.cells { if c.is_mine { c.state = CellState::Revealed; } }
            return;
        }
        let mut q: VecDeque<usize> = VecDeque::new();
        q.push_back(idx);
        while let Some(ci) = q.pop_front() {
            if self.cells[ci].state != CellState::Hidden { continue; }
            self.cells[ci].state = CellState::Revealed;
            self.cells_revealed += 1;
            if self.cells[ci].adjacent == 0 {
                for &ni in &self.adj[ci] {
                    if self.cells[ni].state == CellState::Hidden { q.push_back(ni); }
                }
            }
        }
        if self.cells_revealed >= self.total_safe { self.state = GameState::Won; }
    }

    fn toggle_flag(&mut self, idx: usize) {
        match self.cells[idx].state {
            CellState::Hidden  => { self.cells[idx].state = CellState::Flagged; self.flags_placed += 1; }
            CellState::Flagged => { self.cells[idx].state = CellState::Hidden;  if self.flags_placed > 0 { self.flags_placed -= 1; } }
            _ => {}
        }
    }

    fn chord(&mut self, idx: usize) -> bool {
        if self.cells[idx].state != CellState::Revealed { return false; }
        let flags = self.adj[idx].iter()
            .filter(|&&j| self.cells[j].state == CellState::Flagged)
            .count() as u8;
        if flags != self.cells[idx].adjacent { return false; }
        let to: Vec<usize> = self.adj[idx].iter()
            .filter(|&&j| self.cells[j].state == CellState::Hidden)
            .copied().collect();
        for j in to { self.reveal(j); }
        true
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn num_color(n: u8) -> (f64, f64, f64) {
    match n {
        1 => (0.20, 0.50, 1.00), 2 => (0.20, 0.80, 0.20),
        3 => (1.00, 0.30, 0.30), 4 => (0.20, 0.20, 0.90),
        5 => (1.00, 0.50, 0.00), 6 => (0.00, 0.80, 0.80),
        7 => (0.88, 0.88, 0.88), _ => (0.55, 0.55, 0.55),
    }
}

fn draw_text(
    cr: &gtk4::cairo::Context,
    cx: f64, cy: f64,
    text: &str,
    r: f64, g: f64, b: f64,
    font_px: f64,
) {
    if font_px < 4.0 { return; }
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_text(text);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Sans");
    fd.set_weight(pango::Weight::Bold);
    fd.set_size(((font_px * 0.72 * pango::SCALE as f64) as i32).max(1));
    layout.set_font_description(Some(&fd));
    let (lw, lh) = layout.pixel_size();
    cr.move_to(cx - lw as f64 / 2.0, cy - lh as f64 / 2.0);
    cr.set_source_rgb(r, g, b);
    pangocairo::functions::show_layout(cr, &layout);
}

const MARGIN: f64 = 6.0;

fn to_screen(p: (f64, f64), w: f64, h: f64) -> (f64, f64) {
    (MARGIN + p.0 * (w - 2.0 * MARGIN),
     MARGIN + p.1 * (h - 2.0 * MARGIN))
}

fn draw_board(
    cr: &gtk4::cairo::Context,
    game: &GameVor,
    w: f64, h: f64,
    tc: &super::ThemeColors,
) {
    use std::f64::consts::PI;

    let [bgr, bgg, bgb] = tc.bg;
    cr.set_source_rgb(bgr, bgg, bgb);
    let _ = cr.paint();

    let hidden_rgb = super::mix3(tc.bg, tc.fg, 0.25);
    let revd_rgb   = super::mix3(tc.bg, tc.fg, 0.10);
    let [fgr, fgg, fgb] = tc.fg;

    for (i, cell) in game.cells.iter().enumerate() {
        let poly = &game.polys[i];
        if poly.len() < 3 { continue; }

        let screen: Vec<(f64, f64)> = poly.iter().map(|&p| to_screen(p, w, h)).collect();
        let cx = screen.iter().map(|p| p.0).sum::<f64>() / screen.len() as f64;
        let cy = screen.iter().map(|p| p.1).sum::<f64>() / screen.len() as f64;
        let cell_r = screen.iter()
            .map(|&(x, y)| ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt())
            .fold(0.0_f64, f64::max);

        // Polygon path
        cr.move_to(screen[0].0, screen[0].1);
        for &(x, y) in &screen[1..] { cr.line_to(x, y); }
        cr.close_path();

        // Fill
        let (fr, fg_c, fb_c) = match cell.state {
            CellState::Hidden | CellState::Flagged =>
                (hidden_rgb[0], hidden_rgb[1], hidden_rgb[2]),
            CellState::Revealed if cell.is_mine =>
                (0.65, 0.18, 0.18),
            CellState::Revealed =>
                (revd_rgb[0], revd_rgb[1], revd_rgb[2]),
            _ => (hidden_rgb[0], hidden_rgb[1], hidden_rgb[2]),
        };
        cr.set_source_rgb(fr, fg_c, fb_c);
        let _ = cr.fill_preserve();

        // Edge
        let [wr, wg, wb] = super::mix3(tc.bg, tc.fg, 0.50);
        cr.set_source_rgba(wr, wg, wb, 0.45);
        cr.set_line_width(1.0);
        let _ = cr.stroke();

        // Content
        if cell_r >= 5.0 {
            if cell_r < 12.0 {
                let dot_color: Option<(f64, f64, f64)> = match cell.state {
                    CellState::Flagged => Some((0.95, 0.15, 0.15)),
                    CellState::Revealed if cell.is_mine => Some((0.05, 0.05, 0.05)),
                    CellState::Revealed if cell.adjacent > 0 => Some(num_color(cell.adjacent)),
                    _ => None,
                };
                if let Some((dr, dg, db)) = dot_color {
                    cr.arc(cx, cy, (cell_r * 0.22).max(1.0), 0.0, 2.0 * PI);
                    cr.set_source_rgb(dr, dg, db);
                    let _ = cr.fill();
                }
            } else {
                let font_px = (cell_r * 0.65).clamp(4.0, 20.0);
                match cell.state {
                    CellState::Flagged =>
                        draw_text(cr, cx, cy, "🚩", 1.0, 1.0, 1.0, font_px),
                    CellState::Revealed if cell.is_mine =>
                        draw_text(cr, cx, cy, "💣", 0.9, 0.9, 0.9, font_px),
                    CellState::Revealed if cell.adjacent > 0 => {
                        let (nr, ng, nb) = num_color(cell.adjacent);
                        draw_text(cr, cx, cy, &cell.adjacent.to_string(), nr, ng, nb, font_px);
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = (fgr, fgg, fgb);
}

// ── UI helpers ────────────────────────────────────────────────────────────────

fn set_mine_lbl(game: &GameVor, ml: &Rc<RefCell<Option<gtk4::Label>>>) {
    if let Some(lbl) = ml.borrow().as_ref() {
        lbl.set_text(&format!("{:03}", game.remaining_mines()));
    }
}

fn set_face(
    state: GameState,
    fb: &Rc<RefCell<Option<gtk4::Button>>>,
    sr: &Rc<RefCell<Option<gtk4::glib::SourceId>>>,
) {
    if let Some(btn) = fb.borrow().as_ref() {
        match state {
            GameState::Won  => { btn.set_label("\u{1F60E}"); stop_timer(sr); }
            GameState::Lost => { btn.set_label("\u{1F635}"); stop_timer(sr); }
            _               => {}
        }
    }
}

fn screen_to_unit(sx: f64, sy: f64, w: f64, h: f64) -> (f64, f64) {
    ((sx - MARGIN) / (w - 2.0 * MARGIN),
     (sy - MARGIN) / (h - 2.0 * MARGIN))
}

// ── Board creation ────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let n = width * height;

    // Generate sites with 1 Lloyd relaxation for more uniform cell sizes.
    let mut sites: Vec<(f64, f64)> = {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..n).map(|_| (rng.gen(), rng.gen())).collect()
    };
    lloyd_relax(&mut sites);

    let polys = compute_voronoi_polys(&sites);
    let adj   = compute_adjacency_raster(&sites);
    let game = Rc::new(RefCell::new(GameVor::new(sites, polys, adj, mine_count)));

    set_mine_lbl(&game.borrow(), &ctx.mine_label);

    // Board size: aim for cells with radius ~20px; clamp to [400, 700].
    let board_px = (40.0 * (n as f64).sqrt()).clamp(400.0, 700.0) as i32;

    let da = DrawingArea::new();
    da.set_size_request(board_px, board_px);
    da.set_halign(gtk4::Align::Center);

    {
        let g = game.clone();
        da.set_draw_func(move |widget, cr, w, h| {
            let tc = super::get_theme_colors(widget);
            draw_board(cr, &g.borrow(), w as f64, h as f64, &tc);
        });
    }

    let ml = ctx.mine_label.clone();
    let fb = ctx.face_button.clone();
    let st = ctx.start_time.clone();
    let sr = ctx.timer_source.clone();
    let tl = ctx.timer_label.clone();

    // Left click: reveal or chord
    {
        let (da_c, game_c) = (da.clone(), game.clone());
        let (ml, fb, st, sr, tl) = (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());
        let click = GestureClick::new();
        click.set_button(1);
        click.connect_pressed(move |gesture, _, sx, sy| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
            let (w, h) = (da_c.width() as f64, da_c.height() as f64);
            let (ux, uy) = screen_to_unit(sx, sy, w, h);
            let ti = nearest_site(&game_c.borrow().sites, ux, uy);
            let was_ready = game_c.borrow().state == GameState::Ready;
            {
                let mut g = game_c.borrow_mut();
                if g.cells[ti].state == CellState::Revealed { g.chord(ti); }
                else { g.reveal(ti); }
            }
            if was_ready && game_c.borrow().state == GameState::Playing {
                start_timer(&st, &sr, &tl);
            }
            set_mine_lbl(&game_c.borrow(), &ml);
            set_face(game_c.borrow().state, &fb, &sr);
            da_c.queue_draw();
        });
        da.add_controller(click);
    }

    // Right click: flag
    {
        let (da_c, game_c) = (da.clone(), game.clone());
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let right = GestureClick::new();
        right.set_button(3);
        right.connect_pressed(move |gesture, _, sx, sy| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
            let (w, h) = (da_c.width() as f64, da_c.height() as f64);
            let (ux, uy) = screen_to_unit(sx, sy, w, h);
            let ti = nearest_site(&game_c.borrow().sites, ux, uy);
            game_c.borrow_mut().toggle_flag(ti);
            set_mine_lbl(&game_c.borrow(), &ml);
            set_face(game_c.borrow().state, &fb, &sr);
            da_c.queue_draw();
        });
        da.add_controller(right);
    }

    // Middle click: chord
    {
        let (da_c, game_c) = (da.clone(), game.clone());
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let middle = GestureClick::new();
        middle.set_button(2);
        middle.connect_pressed(move |gesture, _, sx, sy| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Playing) { return; }
            let (w, h) = (da_c.width() as f64, da_c.height() as f64);
            let (ux, uy) = screen_to_unit(sx, sy, w, h);
            let ti = nearest_site(&game_c.borrow().sites, ux, uy);
            if !game_c.borrow_mut().chord(ti) { return; }
            set_mine_lbl(&game_c.borrow(), &ml);
            set_face(game_c.borrow().state, &fb, &sr);
            da_c.queue_draw();
        });
        da.add_controller(middle);
    }

    da.upcast()
}

pub fn update_board(_game: &Rc<RefCell<crate::game::Game>>, board: &gtk4::Widget) {
    if let Some(da) = board.downcast_ref::<DrawingArea>() {
        da.queue_draw();
    }
}
