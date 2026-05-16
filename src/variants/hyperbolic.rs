use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::f64::consts::PI;
use std::rc::Rc;

use gtk4::cairo;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{DrawingArea, GestureClick, GestureDrag};

use crate::game::{CellState, GameState};
use super::{BoardContext, start_timer, stop_timer};

// ── Complex arithmetic ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
struct C(f64, f64);

impl C {
    const ZERO: C = C(0.0, 0.0);
    const ONE:  C = C(1.0, 0.0);
    fn re(self) -> f64 { self.0 }
    fn im(self) -> f64 { self.1 }
    fn conj(self) -> Self { C(self.0, -self.1) }
    fn norm_sq(self) -> f64 { self.0*self.0 + self.1*self.1 }
    fn abs(self) -> f64 { self.norm_sq().sqrt() }
    fn from_polar(r: f64, a: f64) -> Self { C(r*a.cos(), r*a.sin()) }
}

impl std::ops::Add for C {
    type Output = C;
    fn add(self, o: C) -> C { C(self.0+o.0, self.1+o.1) }
}
impl std::ops::Sub for C {
    type Output = C;
    fn sub(self, o: C) -> C { C(self.0-o.0, self.1-o.1) }
}
impl std::ops::Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C { C(self.0*o.0-self.1*o.1, self.0*o.1+self.1*o.0) }
}
impl std::ops::Div for C {
    type Output = C;
    fn div(self, o: C) -> C {
        let d = o.norm_sq();
        C((self.0*o.0+self.1*o.1)/d, (self.1*o.0-self.0*o.1)/d)
    }
}
impl std::ops::Neg for C {
    type Output = C;
    fn neg(self) -> C { C(-self.0, -self.1) }
}
impl std::ops::Mul<f64> for C {
    type Output = C;
    fn mul(self, t: f64) -> C { C(self.0*t, self.1*t) }
}

// ── Möbius transforms: f(z) = (a*z + b) / (c*z + d) ─────────────────────────

#[derive(Clone, Copy)]
struct M { a: C, b: C, c: C, d: C }

impl M {
    fn identity() -> Self { M { a: C::ONE, b: C::ZERO, c: C::ZERO, d: C::ONE } }

    fn apply(self, z: C) -> C { (self.a * z + self.b) / (self.c * z + self.d) }

    /// Matrix product: self ∘ g  (apply g first, then self).
    fn compose(self, g: M) -> M {
        M {
            a: self.a*g.a + self.b*g.c,
            b: self.a*g.b + self.b*g.d,
            c: self.c*g.a + self.d*g.c,
            d: self.c*g.b + self.d*g.d,
        }
    }

    fn inverse(self) -> M { M { a: self.d, b: -self.b, c: -self.c, d: self.a } }

    /// Restore the isometry invariant |a|²-|b|² = 1 lost to FP drift.
    /// Disk isometries have structure {a, b, b̄, ā}; repeated composition lets
    /// the coefficients grow, eventually making apply() return NaN (∞/∞).
    fn normalize(self) -> Self {
        let det = (self.a.norm_sq() - self.b.norm_sq()).max(1e-30);
        let s = 1.0 / det.sqrt();
        M { a: self.a * s, b: self.b * s, c: self.c * s, d: self.d * s }
    }

    /// Poincaré disk isometry: move `p` → origin.
    /// T_p(z) = (z - p) / (1 - p̄z)
    fn to_origin(p: C) -> Self { M { a: C::ONE, b: -p, c: -p.conj(), d: C::ONE } }

    /// Poincaré disk isometry: move origin → `p`.
    fn from_origin(p: C) -> Self { M { a: C::ONE, b: p, c: p.conj(), d: C::ONE } }

    /// Poincaré isometry that moves display point `from` to display point `to`.
    fn translate(from: C, to: C) -> Self {
        M::from_origin(to).compose(M::to_origin(from))
    }
}

// ── {7, 3} hyperbolic tiling ──────────────────────────────────────────────────

const NSIDES: usize = 4;
const CLICK_PX: f64 = 5.0;

/// Circumradius (hyperbolic) of regular {NSIDES, 3} polygon.
/// Derived from fundamental right-triangle: cosh(R) = cosh(a)·cosh(b)
/// where cosh(a) = cos(π/p)/sin(π/q) (half-edge) and cosh(b) = cos(π/q)/sin(π/p) (inradius).
/// → cosh(R) = cos(π/p)·cos(π/q) / (sin(π/p)·sin(π/q))
fn circ_radius() -> f64 {
    let p = NSIDES as f64;
    let q = 5.0_f64;
    let num = (PI / p).cos() * (PI / q).cos();
    let den = (PI / p).sin() * (PI / q).sin();
    (num / den).acosh()
}

fn central_verts() -> [C; NSIDES] {
    let r0 = (circ_radius() / 2.0).tanh();
    let mut v = [C::ZERO; NSIDES];
    for k in 0..NSIDES {
        let angle = 2.0 * PI * k as f64 / NSIDES as f64 + PI / 2.0;
        v[k] = C::from_polar(r0, angle);
    }
    v
}

/// Reflect `z` through the hyperbolic geodesic from `p1` to `p2`.
fn reflect_geo(z: C, p1: C, p2: C) -> C {
    // In p1-centred frame the geodesic is a diameter toward f(p2).
    // Reflection through a diameter at angle θ: z ↦ e^{2iθ} · z̄
    let to   = |w: C| (w - p1) / (C::ONE - p1.conj() * w);
    let from = |w: C| (w + p1) / (C::ONE + p1.conj() * w);
    let q = to(p2);
    let w = to(z);
    // e^{2i·arg(q)} = q² / |q|²  = q / q̄
    let rot = q / q.conj();
    from(rot * w.conj())
}

/// Signed area of a polygon (positive ⟹ CCW in maths coords).
fn signed_area(verts: &[C]) -> f64 {
    let n = verts.len();
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += verts[i].re() * verts[j].im() - verts[j].re() * verts[i].im();
    }
    a * 0.5
}

fn center_key(c: C) -> (i64, i64) {
    ((c.re() * 1e5).round() as i64, (c.im() * 1e5).round() as i64)
}

/// BFS to generate the {7,3} tiling up to `max_rings` layers deep.
/// Ring 0 = center tile; ring N = tiles reachable in exactly N reflections.
/// Returns (verts_per_tile, centers, adjacency_lists).
fn generate_tiling(max_rings: u32) -> (Vec<[C; NSIDES]>, Vec<C>, Vec<Vec<usize>>) {
    let mut verts:   Vec<[C; NSIDES]>  = Vec::new();
    let mut centers: Vec<C>            = Vec::new();
    let mut adj:     Vec<Vec<usize>>   = Vec::new();
    let mut seen:    HashMap<(i64,i64), usize> = HashMap::new();
    let mut depth:   Vec<u32>          = Vec::new();

    let cv = central_verts();
    verts.push(cv);
    centers.push(C::ZERO);
    adj.push(Vec::new());
    depth.push(0);
    seen.insert(center_key(C::ZERO), 0);

    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(0);

    while let Some(ti) = queue.pop_front() {
        if depth[ti] >= max_rings { continue; }
        let tv = verts[ti];
        let tc_pos = centers[ti];
        for k in 0..NSIDES {
            let v0 = tv[k];
            let v1 = tv[(k + 1) % NSIDES];
            let new_center = reflect_geo(tc_pos, v0, v1);
            // Safety: discard numerically degenerate tiles near boundary
            if new_center.abs() >= 0.9999 { continue; }
            let ck = center_key(new_center);
            if let Some(&ni) = seen.get(&ck) {
                if !adj[ti].contains(&ni) {
                    adj[ti].push(ni);
                    adj[ni].push(ti);
                }
                continue;
            }
            let mut nv = [C::ZERO; NSIDES];
            for i in 0..NSIDES { nv[i] = reflect_geo(tv[i], v0, v1); }
            if signed_area(&nv) < 0.0 { nv.reverse(); }
            let ni = verts.len();
            verts.push(nv);
            centers.push(new_center);
            adj.push(Vec::new());
            depth.push(depth[ti] + 1);
            adj[ti].push(ni);
            adj[ni].push(ti);
            seen.insert(ck, ni);
            queue.push_back(ni);
        }
    }
    // Second pass: add corner-sharing (vertex-only) neighbours.
    // In {4,5} each vertex has 5 tiles; edge-adjacent tiles already share 2
    // consecutive vertices.  Any tile sharing just 1 vertex is a corner neighbour.
    let n = verts.len();
    let mut vert_map: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for ti in 0..n {
        for k in 0..NSIDES {
            let key = center_key(verts[ti][k]);
            vert_map.entry(key).or_default().push(ti);
        }
    }
    for ti in 0..n {
        for k in 0..NSIDES {
            let key = center_key(verts[ti][k]);
            if let Some(neighbours) = vert_map.get(&key) {
                let new_neighbours: Vec<usize> = neighbours.iter()
                    .copied()
                    .filter(|&ni| ni != ti && !adj[ti].contains(&ni))
                    .collect();
                for ni in new_neighbours {
                    adj[ti].push(ni);
                    adj[ni].push(ti);
                }
            }
        }
    }

    (verts, centers, adj)
}

// ── Game logic ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct HCell {
    is_mine:  bool,
    state:    CellState,
    adjacent: u8,
}

impl HCell {
    fn new() -> Self { HCell { is_mine: false, state: CellState::Hidden, adjacent: 0 } }
}

struct GameHyp {
    cells:          Vec<HCell>,
    adj:            Vec<Vec<usize>>,
    verts:          Vec<[C; NSIDES]>,
    centers:        Vec<C>,
    pub state:      GameState,
    mines_total:    u32,
    flags_placed:   u32,
    cells_revealed: u32,
    total_safe:     u32,
}

impl GameHyp {
    fn new(verts: Vec<[C; NSIDES]>, centers: Vec<C>, adj: Vec<Vec<usize>>, density: f64) -> Self {
        let n = verts.len();
        let mines = ((n as f64 * density) as u32).clamp(5, (n as u32).saturating_sub(10));
        Self::new_with_mines(verts, centers, adj, mines)
    }

    fn new_with_mines(verts: Vec<[C; NSIDES]>, centers: Vec<C>, adj: Vec<Vec<usize>>, mines: u32) -> Self {
        let n = verts.len();
        GameHyp {
            cells:          vec![HCell::new(); n],
            adj, verts, centers,
            state:          GameState::Ready,
            mines_total:    mines,
            flags_placed:   0,
            cells_revealed: 0,
            total_safe:     n as u32 - mines,
        }
    }

    fn remaining_mines(&self) -> i32 { self.mines_total as i32 - self.flags_placed as i32 }

    fn place_mines(&mut self, first: usize) {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let n = self.cells.len();
        let safe: std::collections::HashSet<usize> =
            std::iter::once(first).chain(self.adj[first].iter().copied()).collect();
        let mut placed = 0u32;
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

    /// Hit-test a display-space Poincaré point (after view transform).
    fn hit_test(&self, view: M, dx: f64, dy: f64) -> Option<usize> {
        let dp = C(dx, dy);
        if dp.abs() >= 1.0 { return None; }
        // Convert display → world coordinates.
        let wp = view.inverse().apply(dp);
        // Check tiles whose display center is closest first (fast reject).
        for (i, &wc) in self.centers.iter().enumerate() {
            if point_in_poly(wp, &self.verts[i]) {
                return Some(i);
            }
        }
        None
    }
}

/// Point-in-convex-polygon (straight-line edges in world/Poincaré coords).
/// Sufficient for hit testing since tiles are convex.
fn point_in_poly(p: C, verts: &[C]) -> bool {
    let n = verts.len();
    let mut sign = 0i8;
    for i in 0..n {
        let v0 = verts[i];
        let v1 = verts[(i + 1) % n];
        let cross = (v1.re()-v0.re()) * (p.im()-v0.im())
                  - (v1.im()-v0.im()) * (p.re()-v0.re());
        if cross.abs() < 1e-12 { continue; }
        let s = if cross > 0.0 { 1i8 } else { -1i8 };
        if sign == 0 { sign = s; } else if sign != s { return false; }
    }
    true
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn num_color(n: u8) -> (f64, f64, f64) {
    match n {
        1 => (0.20, 0.50, 1.00),  2 => (0.20, 0.80, 0.20),
        3 => (1.00, 0.30, 0.30),  4 => (0.20, 0.20, 0.90),
        5 => (1.00, 0.50, 0.00),  6 => (0.00, 0.80, 0.80),
        7 => (0.88, 0.88, 0.88),  _ => (0.55, 0.55, 0.55),
    }
}

/// Sample n+1 points along the geodesic from p1 to p2 (in Poincaré disk).
/// Uses the parametrisation: move p1→0, interpolate linearly to f(p2), invert.
fn geo_pts(p1: C, p2: C, n: usize) -> Vec<C> {
    let to   = |z: C| (z - p1) / (C::ONE - p1.conj() * z);
    let from = |w: C| (w + p1) / (C::ONE + p1.conj() * w);
    let q = to(p2);
    (0..=n).map(|i| from(q * (i as f64 / n as f64))).collect()
}

/// Adaptive segment count for one geodesic edge.
/// Measures how far the true geodesic midpoint deviates from the chord midpoint
/// in screen pixels; n segments reduce sagitta by n², so n = ceil(sqrt(dev)).
fn geo_segs(p1: C, p2: C, scale: f64) -> usize {
    let to   = |z: C| (z - p1) / (C::ONE - p1.conj() * z);
    let from = |w: C| (w + p1) / (C::ONE + p1.conj() * w);
    let q = to(p2);
    let mid_geo   = from(q * 0.5);
    let mid_chord = (p1 + p2) * 0.5;
    let dev_px = (mid_geo - mid_chord).abs() * scale;
    (dev_px.sqrt().ceil() as usize).clamp(1, 24)
}

fn draw_text(cr: &cairo::Context, cx: f64, cy: f64, text: &str,
             r: f64, g: f64, b: f64, font_px: f64) {
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

fn disk_scale(w: f64, h: f64) -> f64 { w.min(h) * 0.47 }

/// World-space point → screen (x, y).
fn to_screen(z: C, view: M, w: f64, h: f64) -> (f64, f64) {
    let d = view.apply(z);
    (w/2.0 + d.re() * disk_scale(w, h),
     h/2.0 - d.im() * disk_scale(w, h))
}

fn draw_board(cr: &cairo::Context, game: &GameHyp, view: M,
              w: f64, h: f64, tc: &super::ThemeColors) {
    let scale = disk_scale(w, h);
    let cx = w / 2.0;
    let cy = h / 2.0;

    // Background
    let [bgr, bgg, bgb] = tc.bg;
    cr.set_source_rgb(bgr, bgg, bgb);
    let _ = cr.paint();

    // Boundary circle
    let [fgr, fgg, fgb] = tc.fg;
    cr.set_source_rgba(fgr, fgg, fgb, 0.25);
    cr.set_line_width(1.0);
    cr.arc(cx, cy, scale, 0.0, 2.0 * PI);
    let _ = cr.stroke();

    // Precompute display-space centres once — avoids O(n log n) apply calls in sort.
    let dc_all: Vec<C> = game.centers.iter().map(|&wc| view.apply(wc)).collect();

    // Pre-filter to visible tiles only, then sort farthest-first.
    let mut order: Vec<usize> = (0..game.cells.len())
        .filter(|&i| dc_all[i].abs() <= 1.02)
        .collect();
    order.sort_by(|&a, &b| {
        dc_all[b].abs().partial_cmp(&dc_all[a].abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let hidden_rgb = super::mix3(tc.bg, tc.fg, 0.25);
    let revd_rgb   = super::mix3(tc.bg, tc.fg, 0.10);

    for &ti in &order {
        let cell = &game.cells[ti];
        let dc = dc_all[ti];

        // Display-space vertices
        let dv: Vec<C> = game.verts[ti].iter()
            .map(|&v| view.apply(v))
            .collect();

        // Tile "radius" in screen pixels — for font sizing and geodesic detail.
        let tile_r_px = dv.iter()
            .map(|&v| ((v - dc).abs() * scale))
            .fold(0.0_f64, f64::max);

        // Sub-pixel tiles are invisible; skip all path/fill/text work.
        if tile_r_px < 2.0 { continue; }

        // ── Build tile path (geodesic edges) ──────────────────────────────────
        let mut first = true;
        for k in 0..NSIDES {
            let segs = geo_segs(dv[k], dv[(k + 1) % NSIDES], scale);
            let pts = geo_pts(dv[k], dv[(k + 1) % NSIDES], segs);
            for (j, &pt) in pts.iter().enumerate() {
                let sx = cx + pt.re() * scale;
                let sy = cy - pt.im() * scale;
                if first && j == 0 { cr.move_to(sx, sy); first = false; }
                else { cr.line_to(sx, sy); }
            }
        }
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
        cr.set_source_rgba(wr, wg, wb, 0.28);
        cr.set_line_width((0.6_f64).min(tile_r_px * 0.04 + 0.3));
        let _ = cr.stroke();

        // Content label
        if tile_r_px >= 5.0 {
            let (scx, scy) = (cx + dc.re() * scale, cy - dc.im() * scale);
            if tile_r_px < 14.0 {
                // Too small for text — draw a coloured dot instead.
                let dot_r = (tile_r_px * 0.19).max(1.0);
                let dot_color = match cell.state {
                    CellState::Flagged =>
                        Some((0.95, 0.15, 0.15)),
                    CellState::Revealed if cell.is_mine =>
                        Some((0.05, 0.05, 0.05)),
                    CellState::Revealed if cell.adjacent > 0 =>
                        Some(num_color(cell.adjacent)),
                    _ => None,
                };
                if let Some((dr, dg, db)) = dot_color {
                    cr.arc(scx, scy, dot_r, 0.0, 2.0 * PI);
                    cr.set_source_rgb(dr, dg, db);
                    let _ = cr.fill();
                }
            } else {
                let font_px = (tile_r_px * 0.55).clamp(4.0, 18.0);
                match cell.state {
                    CellState::Flagged =>
                        draw_text(cr, scx, scy, "🚩", 1.0, 1.0, 1.0, font_px),
                    CellState::Revealed if cell.is_mine =>
                        draw_text(cr, scx, scy, "💣", 0.9, 0.9, 0.9, font_px),
                    CellState::Revealed if cell.adjacent > 0 => {
                        let (nr, ng, nb) = num_color(cell.adjacent);
                        draw_text(cr, scx, scy, &cell.adjacent.to_string(), nr, ng, nb, font_px);
                    }
                    _ => {}
                }
            }
        }
    }

    // "Ready" hint
    if game.state == GameState::Ready {
        let layout = pangocairo::functions::create_layout(cr);
        layout.set_text("drag to pan  •  click to reveal");
        let mut fd = pango::FontDescription::new();
        fd.set_family("Sans");
        fd.set_size(8 * pango::SCALE);
        layout.set_font_description(Some(&fd));
        let (lw, _) = layout.pixel_size();
        cr.move_to(w / 2.0 - lw as f64 / 2.0, h - 16.0);
        cr.set_source_rgba(fgr, fgg, fgb, 0.5);
        pangocairo::functions::show_layout(cr, &layout);
    }
}

// ── UI helpers ────────────────────────────────────────────────────────────────

fn set_mine_lbl(game: &GameHyp, ml: &Rc<RefCell<Option<gtk4::Label>>>) {
    if let Some(lbl) = ml.borrow().as_ref() {
        lbl.set_text(&format!("{:03}", game.remaining_mines()));
    }
}

fn set_face(state: GameState, fb: &Rc<RefCell<Option<gtk4::Button>>>,
            sr: &Rc<RefCell<Option<glib::SourceId>>>) {
    if let Some(btn) = fb.borrow().as_ref() {
        match state {
            GameState::Won  => { btn.set_label("\u{1F60E}"); stop_timer(sr); }
            GameState::Lost => { btn.set_label("\u{1F635}"); stop_timer(sr); }
            _               => {}
        }
    }
}

fn screen_to_disk(sx: f64, sy: f64, w: f64, h: f64) -> C {
    let s = disk_scale(w, h);
    C((sx - w/2.0) / s, -(sy - h/2.0) / s)
}

// ── Board creation ────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    // height==9998 is the sentinel: user set rings + mine count directly.
    let (rings, direct_mines): (u32, Option<u32>) = if height == 9998 {
        (width as u32, Some(mine_count as u32))
    } else {
        (3, None)
    };

    let (verts, centers, adj) = generate_tiling(rings);

    let game = if let Some(m) = direct_mines {
        let n = verts.len() as u32;
        let mines = m.clamp(1, n.saturating_sub(2));
        Rc::new(RefCell::new(GameHyp::new_with_mines(verts, centers, adj, mines)))
    } else {
        let density = (mine_count as f64 / (width * height) as f64).clamp(0.10, 0.25);
        Rc::new(RefCell::new(GameHyp::new(verts, centers, adj, density)))
    };

    set_mine_lbl(&game.borrow(), &ctx.mine_label);

    // View transform (world → display Poincaré).
    let view: Rc<RefCell<M>> = Rc::new(RefCell::new(M::identity()));

    // Drag state
    let drag_prev: Rc<RefCell<Option<C>>> = Rc::new(RefCell::new(None));
    let press_xy:  Rc<RefCell<(f64,f64)>> = Rc::new(RefCell::new((0.0, 0.0)));

    let da = DrawingArea::new();
    da.set_size_request(600, 600);
    da.set_halign(gtk4::Align::Center);

    // ── Draw ─────────────────────────────────────────────────────────────────
    {
        let g = game.clone();
        let v = view.clone();
        da.set_draw_func(move |widget, cr, w, h| {
            let tc = super::get_theme_colors(widget);
            draw_board(cr, &g.borrow(), *v.borrow(), w as f64, h as f64, &tc);
        });
    }

    let ml = ctx.mine_label.clone();
    let fb = ctx.face_button.clone();
    let st = ctx.start_time.clone();
    let sr = ctx.timer_source.clone();
    let tl = ctx.timer_label.clone();

    // ── Left button: GestureDrag (pan + click-to-reveal) ─────────────────────
    {
        let (da_c, game_c, view_c, dp, pxy) = (
            da.clone(), game.clone(), view.clone(),
            drag_prev.clone(), press_xy.clone(),
        );
        let (ml, fb, st, sr, tl) = (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());
        let drag = GestureDrag::new();

        drag.connect_drag_begin({
            let (da_c, dp, pxy) = (da_c.clone(), dp.clone(), pxy.clone());
            move |gesture, _, _| {
                *dp.borrow_mut() = None;
                if let Some((sx, sy)) = gesture.start_point() {
                    let w = da_c.width() as f64;
                    let h = da_c.height() as f64;
                    let disk = screen_to_disk(sx, sy, w, h);
                    if disk.abs() < 1.0 {
                        *dp.borrow_mut() = Some(disk);
                    }
                    *pxy.borrow_mut() = (sx, sy);
                }
            }
        });

        drag.connect_drag_update({
            let (da_c, view_c, dp) = (da_c.clone(), view_c.clone(), dp.clone());
            move |gesture, _, _| {
                let Some(prev) = *dp.borrow() else { return };
                let Some((ox, oy)) = gesture.start_point() else { return };
                let (dx, dy) = gesture.offset().unwrap_or((0.0, 0.0));
                if (dx*dx + dy*dy).sqrt() < CLICK_PX { return; }
                let w = da_c.width() as f64;
                let h = da_c.height() as f64;
                let curr = screen_to_disk(ox + dx, oy + dy, w, h);
                if curr.abs() >= 0.999 { return; }
                // Incremental Möbius: move prev → curr in display space.
                let m = M::translate(prev, curr);
                let new_view = m.compose(*view_c.borrow()).normalize();
                *view_c.borrow_mut() = new_view;
                *dp.borrow_mut() = Some(curr);
                da_c.queue_draw();
            }
        });

        drag.connect_drag_end({
            let (da_c, game_c, view_c, dp, pxy) = (
                da_c.clone(), game_c.clone(), view_c.clone(), dp.clone(), pxy.clone(),
            );
            let (ml, fb, st, sr, tl) = (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());
            move |_, dx, dy| {
                // Small movement → treat as click.
                if (dx*dx + dy*dy).sqrt() >= CLICK_PX { return; }
                if !matches!(game_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
                let (sx, sy) = *pxy.borrow();
                let w = da_c.width() as f64;
                let h = da_c.height() as f64;
                let disk = screen_to_disk(sx, sy, w, h);
                let v = *view_c.borrow();
                let Some(ti) = game_c.borrow().hit_test(v, disk.re(), disk.im()) else { return };
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
                *dp.borrow_mut() = None;
            }
        });

        da.add_controller(drag);
    }

    // ── Right click: flag ─────────────────────────────────────────────────────
    {
        let (da_c, game_c, view_c) = (da.clone(), game.clone(), view.clone());
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let right = GestureClick::new();
        right.set_button(3);
        right.connect_pressed(move |gesture, _, sx, sy| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
            let w = da_c.width() as f64;
            let h = da_c.height() as f64;
            let disk = screen_to_disk(sx, sy, w, h);
            let v = *view_c.borrow();
            let Some(ti) = game_c.borrow().hit_test(v, disk.re(), disk.im()) else { return };
            game_c.borrow_mut().toggle_flag(ti);
            set_mine_lbl(&game_c.borrow(), &ml);
            set_face(game_c.borrow().state, &fb, &sr);
            da_c.queue_draw();
        });
        da.add_controller(right);
    }

    // ── Middle click: chord ───────────────────────────────────────────────────
    {
        let (da_c, game_c, view_c) = (da.clone(), game.clone(), view.clone());
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let middle = GestureClick::new();
        middle.set_button(2);
        middle.connect_pressed(move |gesture, _, sx, sy| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Playing) { return; }
            let w = da_c.width() as f64;
            let h = da_c.height() as f64;
            let disk = screen_to_disk(sx, sy, w, h);
            let v = *view_c.borrow();
            let Some(ti) = game_c.borrow().hit_test(v, disk.re(), disk.im()) else { return };
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
