use std::cell::RefCell;
use std::collections::VecDeque;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk4::cairo;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Button, DrawingArea, EventControllerScroll, GestureClick, GestureDrag, Label};

use crate::game::{CellState, GameState};
use super::{BoardContext, start_timer, stop_timer};

// ---------------------------------------------------------------------------
// World-space layout  (unit cube per cell)
// ---------------------------------------------------------------------------
const W_STEP:    f64   = 1.0;
const W_HALF:    f64   = 0.40;
const DISPLAY:   f64   = 280.0;
const INIT_AZ:   f64   = -0.55;
const INIT_EL:   f64   =  0.40;
const DRAG_SENS: f64   =  0.008;
const CLICK_PX:  f64   =  5.0;
const BEVEL:     f64   =  0.10;
const MAX_BEVEL: usize =  4;   // arc segments per edge/corner at full LOD

// ---------------------------------------------------------------------------
// 3-D game (self-contained, does not touch the 2-D Game)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Cell3D { is_mine: bool, state: CellState, adjacent: u8 }
impl Cell3D { fn new() -> Self { Cell3D { is_mine: false, state: CellState::Hidden, adjacent: 0 } } }

struct Game3D {
    sx:             usize,
    sy:             usize,
    sz:             usize,
    grid:           Vec<Vec<Vec<Cell3D>>>,   // [z][y][x]
    pub state:      GameState,
    mines_total:    u32,
    flags_placed:   u32,
    cells_revealed: u32,
    total_safe:     u32,
}

impl Game3D {
    fn new(sx: usize, sy: usize, sz: usize, mines: u32) -> Self {
        let total = (sx * sy * sz) as u32;
        let mines = mines.min(total.saturating_sub(28));
        Game3D {
            sx, sy, sz,
            grid: vec![vec![vec![Cell3D::new(); sx]; sy]; sz],
            state: GameState::Ready,
            mines_total: mines,
            flags_placed: 0,
            cells_revealed: 0,
            total_safe: total - mines,
        }
    }

    fn nb(&self, x: usize, y: usize, z: usize) -> Vec<(usize, usize, usize)> {
        let mut v = Vec::with_capacity(26);
        for dz in -1i32..=1 { for dy in -1i32..=1 { for dx in -1i32..=1 {
            if dx == 0 && dy == 0 && dz == 0 { continue; }
            let (nx, ny, nz) = (x as i32 + dx, y as i32 + dy, z as i32 + dz);
            if nx >= 0 && nx < self.sx as i32
            && ny >= 0 && ny < self.sy as i32
            && nz >= 0 && nz < self.sz as i32 {
                v.push((nx as usize, ny as usize, nz as usize));
            }
        }}}
        v
    }

    fn place_mines(&mut self, ax: usize, ay: usize, az: usize) {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let mut placed = 0u32;
        while placed < self.mines_total {
            let x = rng.gen_range(0..self.sx);
            let y = rng.gen_range(0..self.sy);
            let z = rng.gen_range(0..self.sz);
            if self.grid[z][y][x].is_mine { continue; }
            if (x as i32 - ax as i32).abs() <= 1
            && (y as i32 - ay as i32).abs() <= 1
            && (z as i32 - az as i32).abs() <= 1 { continue; }
            self.grid[z][y][x].is_mine = true;
            placed += 1;
        }
        for gz in 0..self.sz { for gy in 0..self.sy { for gx in 0..self.sx {
            if self.grid[gz][gy][gx].is_mine { continue; }
            self.grid[gz][gy][gx].adjacent = self.nb(gx, gy, gz).iter()
                .filter(|&&(nx, ny, nz)| self.grid[nz][ny][nx].is_mine).count() as u8;
        }}}
    }

    fn reveal(&mut self, x: usize, y: usize, z: usize) {
        if self.state == GameState::Ready {
            self.place_mines(x, y, z);
            self.state = GameState::Playing;
        }
        if !matches!(self.state, GameState::Playing) { return; }
        if self.grid[z][y][x].state != CellState::Hidden { return; }
        if self.grid[z][y][x].is_mine {
            self.state = GameState::Lost;
            for gz in 0..self.sz { for gy in 0..self.sy { for gx in 0..self.sx {
                if self.grid[gz][gy][gx].is_mine {
                    self.grid[gz][gy][gx].state = CellState::Revealed;
                }
            }}}
            return;
        }
        let mut q: VecDeque<(usize, usize, usize)> = VecDeque::new();
        q.push_back((x, y, z));
        while let Some((cx, cy, cz)) = q.pop_front() {
            if self.grid[cz][cy][cx].state != CellState::Hidden { continue; }
            self.grid[cz][cy][cx].state = CellState::Revealed;
            self.cells_revealed += 1;
            if self.grid[cz][cy][cx].adjacent == 0 {
                let nb = self.nb(cx, cy, cz);
                for (nx, ny, nz) in nb {
                    if self.grid[nz][ny][nx].state == CellState::Hidden {
                        q.push_back((nx, ny, nz));
                    }
                }
            }
        }
        if self.cells_revealed >= self.total_safe { self.state = GameState::Won; }
    }

    fn toggle_flag(&mut self, x: usize, y: usize, z: usize) {
        match self.grid[z][y][x].state {
            CellState::Hidden  => { self.grid[z][y][x].state = CellState::Flagged; self.flags_placed += 1; }
            CellState::Flagged => { self.grid[z][y][x].state = CellState::Hidden;  if self.flags_placed > 0 { self.flags_placed -= 1; } }
            _ => {}
        }
    }

    fn chord(&mut self, x: usize, y: usize, z: usize) -> bool {
        if self.grid[z][y][x].state != CellState::Revealed { return false; }
        let nb = self.nb(x, y, z);
        let flags = nb.iter()
            .filter(|&&(nx, ny, nz)| self.grid[nz][ny][nx].state == CellState::Flagged)
            .count() as u8;
        if flags != self.grid[z][y][x].adjacent { return false; }
        let to: Vec<_> = nb.into_iter()
            .filter(|&(nx, ny, nz)| self.grid[nz][ny][nx].state == CellState::Hidden)
            .collect();
        for (nx, ny, nz) in to { self.reveal(nx, ny, nz); }
        true
    }

    fn remaining_mines(&self) -> i32 { self.mines_total as i32 - self.flags_placed as i32 }
}

// ---------------------------------------------------------------------------
// Projection math
// ---------------------------------------------------------------------------

fn proj(wx: f64, wy: f64, wz: f64, az: f64, el: f64) -> (f64, f64, f64) {
    let rx  =  wx * az.cos() + wz * az.sin();
    let rz  = -wx * az.sin() + wz * az.cos();
    let ry2 =  wy * el.cos() - rz * el.sin();
    let rz2 =  wy * el.sin() + rz * el.cos();
    (rx, -ry2, rz2)
}

fn view_dir(az: f64, el: f64) -> (f64, f64, f64) {
    (-az.sin() * el.cos(), el.sin(), az.cos() * el.cos())
}

/// Scale so the actual 3-D diagonal of the grid fits within DISPLAY.
fn scale_for(sx: usize, sy: usize, sz: usize) -> f64 {
    let dx = sx.saturating_sub(1) as f64;
    let dy = sy.saturating_sub(1) as f64;
    let dz = sz.saturating_sub(1) as f64;
    let diag = (dx*dx + dy*dy + dz*dz).sqrt();
    DISPLAY * 0.82 / diag.max(1.0)
}

fn cell_world(gx: usize, gy: usize, gz: usize, hx: f64, hy: f64, hz: f64) -> (f64, f64, f64) {
    ((gx as f64 - hx) * W_STEP,
     (gy as f64 - hy) * W_STEP,
     (gz as f64 - hz) * W_STEP)
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

fn fill_quad(cr: &cairo::Context, q: [(f64, f64); 4]) {
    cr.move_to(q[0].0, q[0].1);
    cr.line_to(q[1].0, q[1].1);
    cr.line_to(q[2].0, q[2].1);
    cr.line_to(q[3].0, q[3].1);
    cr.close_path();
    let _ = cr.fill();
}

fn outline_quad(cr: &cairo::Context, q: [(f64, f64); 4]) {
    cr.set_line_width(0.7);
    cr.move_to(q[0].0, q[0].1);
    cr.line_to(q[1].0, q[1].1);
    cr.line_to(q[2].0, q[2].1);
    cr.line_to(q[3].0, q[3].1);
    cr.close_path();
    let _ = cr.stroke();
}

fn num_color(n: u8) -> (f64, f64, f64) {
    match n {
         1 => (0.20,0.50,1.00),  2 => (0.20,0.80,0.20),
         3 => (1.00,0.30,0.30),  4 => (0.20,0.20,0.90),
         5 => (1.00,0.50,0.00),  6 => (0.00,0.80,0.80),
         7 => (0.88,0.88,0.88),  8 => (0.50,0.50,0.50),
         9 => (0.00,0.60,1.00), 10 => (0.00,0.93,0.00),
        11 => (1.00,0.20,0.20), 12 => (0.20,0.20,1.00),
        13 => (1.00,0.60,0.00), 14 => (0.00,1.00,0.80),
        15 => (0.80,0.00,1.00), 16 => (0.67,0.67,0.67),
        17 => (0.20,0.73,1.00), 18 => (0.20,1.00,0.20),
        19 => (1.00,0.40,0.40), 20 => (0.40,0.40,1.00),
        21 => (1.00,0.73,0.00), 22 => (0.00,1.00,1.00),
        23 => (0.93,0.00,1.00), 24 => (1.00,1.00,1.00),
        _  => (1.00,0.60,1.00),
    }
}

fn draw_label(cr: &cairo::Context, cx: f64, cy: f64, text: &str,
              r: f64, g: f64, b: f64, a: f64, font_px: f64) {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_text(text);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Sans");
    fd.set_weight(pango::Weight::Bold);
    fd.set_size((font_px * 0.72 * pango::SCALE as f64) as i32);
    layout.set_font_description(Some(&fd));
    let (lw, lh) = layout.pixel_size();
    cr.move_to(cx - lw as f64 / 2.0, cy - lh as f64 / 2.0);
    cr.set_source_rgba(r, g, b, a);
    pangocairo::functions::show_layout(cr, &layout);
}

// ---------------------------------------------------------------------------
// Cube renderer with per-cell LOD
//
// bevel_n = 0 : flat box only (6 faces, no edge/corner geometry)
// bevel_n = 1…MAX_BEVEL : arc segments per rounded edge/corner
// ---------------------------------------------------------------------------
fn draw_cube(
    cr:      &cairo::Context,
    cell:    &Cell3D,
    wx: f64, wy: f64, wz: f64,
    az: f64, el: f64,
    scx: f64, scy: f64,
    scale: f64,
    bevel_n: usize,
) {
    let h  = W_HALF;
    let b  = BEVEL;
    // When there are no bevel strips, extend flat faces to full half-size so
    // there's no visible gap at the edges.
    let fh = if bevel_n > 0 { h - b } else { h };
    let vd = view_dir(az, el);

    let p = |dx: f64, dy: f64, dz: f64| -> (f64, f64) {
        let (px, py, _) = proj(wx + dx, wy + dy, wz + dz, az, el);
        (scx + px * scale, scy + py * scale)
    };

    let alpha: f64 = match cell.state {
        CellState::Hidden                    => 0.80,
        CellState::Revealed if !cell.is_mine => 0.18,
        _                                    => 1.0,
    };
    let (br, bg, bb): (f64, f64, f64) = match cell.state {
        CellState::Revealed if cell.is_mine => (0.65, 0.18, 0.18),
        CellState::Revealed                 => (0.14, 0.14, 0.14),
        _                                   => (0.24, 0.24, 0.24),
    };
    let lit = |_nx: f64, ny: f64, nz: f64| -> f64 {
        (0.65 + 0.65 * ny + 0.35 * nz).max(0.05)
    };
    let set_col = |cr: &cairo::Context, l: f64| {
        cr.set_source_rgba((br*l).min(1.0), (bg*l).min(1.0), (bb*l).min(1.0), alpha);
    };

    // ── 6 flat faces ──────────────────────────────────────────────────────────
    for ez in [1.0_f64, -1.0] {
        if ez * vd.2 < 0.02 { continue; }
        let q = [p(-fh,-fh,ez*h), p(fh,-fh,ez*h), p(fh,fh,ez*h), p(-fh,fh,ez*h)];
        set_col(cr, lit(0.0, 0.0, ez)); fill_quad(cr, q);
        cr.set_source_rgba(0.5, 0.5, 0.5, alpha * 0.4); outline_quad(cr, q);
    }
    for ey in [1.0_f64, -1.0] {
        if ey * vd.1 < 0.02 { continue; }
        let q = [p(-fh,ey*h,-fh), p(fh,ey*h,-fh), p(fh,ey*h,fh), p(-fh,ey*h,fh)];
        set_col(cr, lit(0.0, ey, 0.0)); fill_quad(cr, q);
        cr.set_source_rgba(0.5, 0.5, 0.5, alpha * 0.4); outline_quad(cr, q);
    }
    for ex in [1.0_f64, -1.0] {
        if ex * vd.0 < 0.02 { continue; }
        let q = [p(ex*h,-fh,-fh), p(ex*h,fh,-fh), p(ex*h,fh,fh), p(ex*h,-fh,fh)];
        set_col(cr, lit(ex, 0.0, 0.0)); fill_quad(cr, q);
        cr.set_source_rgba(0.5, 0.5, 0.5, alpha * 0.4); outline_quad(cr, q);
    }

    // ── 12 edge strips (skipped when bevel_n == 0) ────────────────────────────
    let n = bevel_n;
    // Z-Y edges
    for ez in [1.0_f64, -1.0] { for ey in [1.0_f64, -1.0] {
        for i in 0..n {
            let t0 = (i       as f64 / n as f64) * PI * 0.5;
            let t1 = ((i + 1) as f64 / n as f64) * PI * 0.5;
            let tm = (t0 + t1) * 0.5;
            let (ny, nz) = (ey * tm.sin(), ez * tm.cos());
            if ny * vd.1 + nz * vd.2 <= 0.0 { continue; }
            let (y0, z0) = (ey*(fh + b*t0.sin()), ez*(fh + b*t0.cos()));
            let (y1, z1) = (ey*(fh + b*t1.sin()), ez*(fh + b*t1.cos()));
            set_col(cr, lit(0.0, ny, nz));
            fill_quad(cr, [p(-fh,y0,z0), p(fh,y0,z0), p(fh,y1,z1), p(-fh,y1,z1)]);
        }
    }}
    // Z-X edges
    for ez in [1.0_f64, -1.0] { for ex in [1.0_f64, -1.0] {
        for i in 0..n {
            let t0 = (i       as f64 / n as f64) * PI * 0.5;
            let t1 = ((i + 1) as f64 / n as f64) * PI * 0.5;
            let tm = (t0 + t1) * 0.5;
            let (nx, nz) = (ex * tm.sin(), ez * tm.cos());
            if nx * vd.0 + nz * vd.2 <= 0.0 { continue; }
            let (x0, z0) = (ex*(fh + b*t0.sin()), ez*(fh + b*t0.cos()));
            let (x1, z1) = (ex*(fh + b*t1.sin()), ez*(fh + b*t1.cos()));
            set_col(cr, lit(nx, 0.0, nz));
            fill_quad(cr, [p(x0,-fh,z0), p(x0,fh,z0), p(x1,fh,z1), p(x1,-fh,z1)]);
        }
    }}
    // Y-X edges
    for ey in [1.0_f64, -1.0] { for ex in [1.0_f64, -1.0] {
        for i in 0..n {
            let t0 = (i       as f64 / n as f64) * PI * 0.5;
            let t1 = ((i + 1) as f64 / n as f64) * PI * 0.5;
            let tm = (t0 + t1) * 0.5;
            let (nx, ny) = (ex * tm.sin(), ey * tm.cos());
            if nx * vd.0 + ny * vd.1 <= 0.0 { continue; }
            let (x0, y0) = (ex*(fh + b*t0.sin()), ey*(fh + b*t0.cos()));
            let (x1, y1) = (ex*(fh + b*t1.sin()), ey*(fh + b*t1.cos()));
            set_col(cr, lit(nx, ny, 0.0));
            fill_quad(cr, [p(x0,y0,-fh), p(x0,y0,fh), p(x1,y1,fh), p(x1,y1,-fh)]);
        }
    }}

    // ── 8 spherical corner patches (skipped when bevel_n == 0) ────────────────
    let cn = bevel_n;
    for ez in [1.0_f64, -1.0] { for ey in [1.0_f64, -1.0] { for ex in [1.0_f64, -1.0] {
        let (cx, cy, cz) = (ex*fh, ey*fh, ez*fh);
        let spt = |u: f64, v: f64| -> (f64, f64) {
            let w = 1.0 - u - v;
            let (dx, dy, dz) = (v*ex, u*ey, w*ez);
            let len = (dx*dx + dy*dy + dz*dz).sqrt().max(1e-12);
            p(cx + b*dx/len, cy + b*dy/len, cz + b*dz/len)
        };
        for i in 0..cn { for j in 0..(cn - i) {
            let (u0, u1) = (i as f64/cn as f64, (i+1) as f64/cn as f64);
            let (v0, v1) = (j as f64/cn as f64, (j+1) as f64/cn as f64);
            {
                let (um, vm) = ((u0+u1+u0)/3.0, (v0+v0+v1)/3.0);
                let wm = 1.0 - um - vm;
                let (dx,dy,dz) = (vm*ex, um*ey, wm*ez);
                let len = (dx*dx+dy*dy+dz*dz).sqrt().max(1e-12);
                let (nx,ny,nz) = (dx/len, dy/len, dz/len);
                if nx*vd.0 + ny*vd.1 + nz*vd.2 > 0.0 {
                    let (pa,pb,pc) = (spt(u0,v0), spt(u1,v0), spt(u0,v1));
                    set_col(cr, lit(nx, ny, nz));
                    cr.move_to(pa.0,pa.1); cr.line_to(pb.0,pb.1);
                    cr.line_to(pc.0,pc.1); cr.close_path(); let _ = cr.fill();
                }
            }
            if u1 + v1 <= 1.0 {
                let (um, vm) = ((u1+u1+u0)/3.0, (v0+v1+v1)/3.0);
                let wm = 1.0 - um - vm;
                let (dx,dy,dz) = (vm*ex, um*ey, wm*ez);
                let len = (dx*dx+dy*dy+dz*dz).sqrt().max(1e-12);
                let (nx,ny,nz) = (dx/len, dy/len, dz/len);
                if nx*vd.0 + ny*vd.1 + nz*vd.2 > 0.0 {
                    let (pb,pd,pc) = (spt(u1,v0), spt(u1,v1), spt(u0,v1));
                    set_col(cr, lit(nx, ny, nz));
                    cr.move_to(pb.0,pb.1); cr.line_to(pd.0,pd.1);
                    cr.line_to(pc.0,pc.1); cr.close_path(); let _ = cr.fill();
                }
            }
        }}
    }}}

    // ── Content label (always faces camera) ───────────────────────────────────
    let (fcx, fcy) = p(0.0, 0.0, 0.0);
    let font_px = h * 2.0 * scale * 0.55;
    match cell.state {
        CellState::Flagged => draw_label(cr, fcx, fcy, "🚩", 1.0, 0.6, 0.0, 1.0, font_px),
        CellState::Revealed if cell.is_mine =>
            draw_label(cr, fcx, fcy, "💣", 0.9, 0.9, 0.9, 1.0, font_px),
        CellState::Revealed if cell.adjacent > 0 => {
            let (nr, ng, nb_) = num_color(cell.adjacent);
            draw_label(cr, fcx, fcy, &cell.adjacent.to_string(), nr, ng, nb_, alpha, font_px);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Interior culling: skip an opaque cell if all 6 face-neighbours are in-bounds,
// inside the current slice filter, and opaque (so they fully block each face).
// ---------------------------------------------------------------------------
fn is_occluded(
    game: &Game3D,
    x: usize, y: usize, z: usize,
    lmin: usize, lmax: usize,
    rmin: usize, rmax: usize,
    cmin: usize, cmax: usize,
) -> bool {
    if game.grid[z][y][x].state == CellState::Revealed { return false; }

    let face_covered = |nx: i32, ny: i32, nz: i32| -> bool {
        if nx < 0 || nx >= game.sx as i32 { return false; }
        if ny < 0 || ny >= game.sy as i32 { return false; }
        if nz < 0 || nz >= game.sz as i32 { return false; }
        let (nx, ny, nz) = (nx as usize, ny as usize, nz as usize);
        if nz < lmin || nz > lmax { return false; }
        if ny < rmin || ny > rmax { return false; }
        if nx < cmin || nx > cmax { return false; }
        game.grid[nz][ny][nx].state != CellState::Revealed
    };

    let (ix, iy, iz) = (x as i32, y as i32, z as i32);
    face_covered(ix-1, iy, iz) && face_covered(ix+1, iy, iz) &&
    face_covered(ix, iy-1, iz) && face_covered(ix, iy+1, iz) &&
    face_covered(ix, iy, iz-1) && face_covered(ix, iy, iz+1)
}

// ---------------------------------------------------------------------------
// Board draw
// ---------------------------------------------------------------------------

fn draw_board(cr: &cairo::Context, game: &Game3D, az: f64, el: f64, w: f64, h: f64,
              lmin: usize, lmax: usize,
              rmin: usize, rmax: usize,
              cmin: usize, cmax: usize,
              zoom: f64) {
    cr.set_source_rgb(0.09, 0.09, 0.09);
    let _ = cr.paint();

    let (sx, sy, sz) = (game.sx, game.sy, game.sz);
    let scx   = w / 2.0;
    let scy   = h / 2.0;
    let scale = scale_for(sx, sy, sz) * zoom;
    let hx    = (sx as f64 - 1.0) / 2.0;
    let hy    = (sy as f64 - 1.0) / 2.0;
    let hz    = (sz as f64 - 1.0) / 2.0;

    // Build cell list: apply slice filter and skip fully-interior opaque cells.
    let mut cells: Vec<(f64, usize, usize, usize)> = Vec::new();
    for gz in lmin..=lmax.min(sz.saturating_sub(1)) {
        for gy in rmin..=rmax.min(sy.saturating_sub(1)) {
            for gx in cmin..=cmax.min(sx.saturating_sub(1)) {
                if is_occluded(game, gx, gy, gz, lmin, lmax, rmin, rmax, cmin, cmax) {
                    continue;
                }
                let (wx, wy, wz) = cell_world(gx, gy, gz, hx, hy, hz);
                let (_, _, depth) = proj(wx, wy, wz, az, el);
                cells.push((depth, gx, gy, gz));
            }
        }
    }

    // Painter's algorithm: farthest first.
    cells.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Map depth range → LOD: farthest cells get bevel_n=0, nearest get MAX_BEVEL.
    let depth_min = cells.first().map(|c| c.0).unwrap_or(0.0);
    let depth_max = cells.last().map(|c| c.0).unwrap_or(1.0);
    let depth_range = (depth_max - depth_min).max(1e-6);

    for &(depth, gx, gy, gz) in &cells {
        let t = (depth - depth_min) / depth_range;   // 0 = farthest, 1 = nearest
        let bevel_n = ((t * (MAX_BEVEL + 1) as f64) as usize).min(MAX_BEVEL);
        let (wx, wy, wz) = cell_world(gx, gy, gz, hx, hy, hz);
        draw_cube(cr, &game.grid[gz][gy][gx], wx, wy, wz, az, el, scx, scy, scale, bevel_n);
    }

    if game.state == GameState::Ready {
        let layout = pangocairo::functions::create_layout(cr);
        layout.set_text("drag to rotate");
        let mut fd = pango::FontDescription::new();
        fd.set_family("Sans");
        fd.set_size(8 * pango::SCALE);
        layout.set_font_description(Some(&fd));
        let (lw, _) = layout.pixel_size();
        cr.move_to(w / 2.0 - lw as f64 / 2.0, h - 16.0);
        cr.set_source_rgba(0.6, 0.6, 0.6, 0.5);
        pangocairo::functions::show_layout(cr, &layout);
    }
}

// ---------------------------------------------------------------------------
// Hit-test
// ---------------------------------------------------------------------------

fn hit_test_3d(
    mx: f64, my: f64,
    game: &Game3D,
    az: f64, el: f64,
    w: f64, h: f64,
    lmin: usize, lmax: usize,
    rmin: usize, rmax: usize,
    cmin: usize, cmax: usize,
    zoom: f64,
) -> Option<(usize, usize, usize)> {
    let (sx, sy, sz) = (game.sx, game.sy, game.sz);
    let scx   = w / 2.0;
    let scy   = h / 2.0;
    let scale = scale_for(sx, sy, sz) * zoom;
    let hx    = (sx as f64 - 1.0) / 2.0;
    let hy    = (sy as f64 - 1.0) / 2.0;
    let hz    = (sz as f64 - 1.0) / 2.0;
    let r     = W_HALF * scale;

    let mut best_solid:    Option<(f64, usize, usize, usize)> = None;
    let mut best_revealed: Option<(f64, usize, usize, usize)> = None;

    for gz in lmin..=lmax.min(sz.saturating_sub(1)) {
        for gy in rmin..=rmax.min(sy.saturating_sub(1)) {
            for gx in cmin..=cmax.min(sx.saturating_sub(1)) {
                let (wx, wy, wz) = cell_world(gx, gy, gz, hx, hy, hz);
                let (px, py, depth) = proj(wx, wy, wz, az, el);
                let px = scx + px * scale;
                let py = scy + py * scale;
                if (mx - px).abs() < r && (my - py).abs() < r {
                    if game.grid[gz][gy][gx].state == CellState::Revealed {
                        if best_revealed.map_or(true, |(d, ..)| depth > d) {
                            best_revealed = Some((depth, gx, gy, gz));
                        }
                    } else if best_solid.map_or(true, |(d, ..)| depth > d) {
                        best_solid = Some((depth, gx, gy, gz));
                    }
                }
            }
        }
    }

    best_solid.or(best_revealed).map(|(_, x, y, z)| (x, y, z))
}

// ---------------------------------------------------------------------------
// Local UI helpers
// ---------------------------------------------------------------------------

fn set_mine_label(game: &Game3D, mine_label: &Rc<RefCell<Option<gtk4::Label>>>) {
    if let Some(lbl) = mine_label.borrow().as_ref() {
        lbl.set_text(&format!("{:03}", game.remaining_mines()));
    }
}

fn set_face(
    state:        GameState,
    face_button:  &Rc<RefCell<Option<Button>>>,
    timer_source: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    if let Some(btn) = face_button.borrow().as_ref() {
        match state {
            GameState::Won  => { btn.set_label("\u{1F60E}"); stop_timer(timer_source); }
            GameState::Lost => { btn.set_label("\u{1F635}"); stop_timer(timer_source); }
            _               => { btn.set_label("\u{1F642}"); }
        }
    }
}

// ---------------------------------------------------------------------------
// Two-handle range slider
// ---------------------------------------------------------------------------

fn make_range_slider(
    size:     usize,
    vmin:     Rc<RefCell<usize>>,
    vmax:     Rc<RefCell<usize>>,
    board_da: DrawingArea,
) -> DrawingArea {
    const PAD: f64 = 12.0;
    const HR:  f64 = 7.0;

    let da = DrawingArea::new();
    da.set_size_request(-1, 42);
    da.set_hexpand(true);

    {
        let vmin = vmin.clone(); let vmax = vmax.clone();
        da.set_draw_func(move |widget, cr, _w, _h| {
            let w   = widget.width() as f64;
            let h   = widget.height() as f64;
            let ty  = h * 0.45;
            let len = w - 2.0 * PAD;
            let n   = (size - 1) as f64;
            let v2x = |v: usize| PAD + v as f64 / n * len;

            let xmin = v2x(*vmin.borrow());
            let xmax = v2x(*vmax.borrow());

            cr.set_source_rgba(0.30, 0.30, 0.30, 1.0);
            cr.set_line_width(3.0);
            cr.move_to(PAD, ty); cr.line_to(w - PAD, ty);
            let _ = cr.stroke();

            let accent = widget.style_context().lookup_color("accent_color")
                .or_else(|| widget.style_context().lookup_color("theme_selected_bg_color"))
                .unwrap_or(gtk4::gdk::RGBA::new(0.35, 0.55, 0.95, 1.0));
            cr.set_source_rgba(accent.red() as f64, accent.green() as f64,
                               accent.blue() as f64, 0.9);
            cr.set_line_width(3.0);
            cr.move_to(xmin, ty); cr.line_to(xmax, ty);
            let _ = cr.stroke();

            for i in 0..size {
                let x = v2x(i);
                cr.set_source_rgba(0.55, 0.55, 0.55, 0.65);
                cr.set_line_width(1.0);
                cr.move_to(x, ty + 5.0); cr.line_to(x, ty + 9.0);
                let _ = cr.stroke();

                let layout = pangocairo::functions::create_layout(cr);
                layout.set_text(&i.to_string());
                let mut fd = pango::FontDescription::new();
                fd.set_family("Sans");
                fd.set_size((7.0 * pango::SCALE as f64) as i32);
                layout.set_font_description(Some(&fd));
                let (lw, _) = layout.pixel_size();
                cr.move_to(x - lw as f64 / 2.0, ty + 10.0);
                cr.set_source_rgba(0.55, 0.55, 0.55, 0.65);
                pangocairo::functions::show_layout(cr, &layout);
            }

            for &x in &[xmin, xmax] {
                cr.arc(x, ty, HR, 0.0, 2.0 * PI);
                cr.set_source_rgba(0.82, 0.82, 0.82, 1.0);
                let _ = cr.fill();
                cr.arc(x, ty, HR, 0.0, 2.0 * PI);
                cr.set_source_rgba(0.45, 0.45, 0.45, 0.7);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
        });
    }

    let active: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    {
        let drag = GestureDrag::new();

        drag.connect_drag_begin({
            let vmin = vmin.clone(); let vmax = vmax.clone();
            let active = active.clone();
            let da_c = da.clone();
            move |_, bx, _| {
                let w   = da_c.width() as f64;
                let len = w - 2.0 * PAD;
                let n   = (size - 1) as f64;
                let v2x = |v: usize| PAD + v as f64 / n * len;
                let dx_min = (bx - v2x(*vmin.borrow())).abs();
                let dx_max = (bx - v2x(*vmax.borrow())).abs();
                *active.borrow_mut() = Some(dx_max < dx_min);
            }
        });

        drag.connect_drag_update({
            let vmin = vmin.clone(); let vmax = vmax.clone();
            let active = active.clone();
            let da_c = da.clone();
            let board_da = board_da.clone();
            move |gesture, dx, _| {
                let Some(is_max) = *active.borrow() else { return; };
                let bx  = gesture.start_point().map(|(x, _)| x).unwrap_or(0.0);
                let w   = da_c.width() as f64;
                let len = (w - 2.0 * PAD).max(1.0);
                let n   = (size - 1) as f64;
                let v   = ((bx + dx - PAD) / len * n).round().clamp(0.0, n) as usize;
                if is_max { *vmax.borrow_mut() = v.max(*vmin.borrow()); }
                else      { *vmin.borrow_mut() = v.min(*vmax.borrow()); }
                da_c.queue_draw();
                board_da.queue_draw();
            }
        });

        drag.connect_drag_end({ move |_, _, _| { *active.borrow_mut() = None; } });

        da.add_controller(drag);
    }

    da
}

// ---------------------------------------------------------------------------
// Board creation
// ---------------------------------------------------------------------------

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    // Grid: x and z axes = game width (so top/bottom face = width²),
    //       y axis = game height, mine count taken directly from 2-D game.
    let (sx, sy, sz, gm) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.width, g.mine_count)
    };
    let game3d = Rc::new(RefCell::new(Game3D::new(sx, sy, sz, gm as u32)));

    let az       = Rc::new(RefCell::new(INIT_AZ));
    let el       = Rc::new(RefCell::new(INIT_EL));
    let az_start = Rc::new(RefCell::new(INIT_AZ));
    let el_start = Rc::new(RefCell::new(INIT_EL));
    let press_x  = Rc::new(RefCell::new(0.0f64));
    let press_y  = Rc::new(RefCell::new(0.0f64));

    // Slice filters — each axis has its own independent range.
    let layer_min = Rc::new(RefCell::new(0usize));
    let layer_max = Rc::new(RefCell::new(sz.saturating_sub(1)));
    let row_min   = Rc::new(RefCell::new(0usize));
    let row_max   = Rc::new(RefCell::new(sy.saturating_sub(1)));
    let col_min   = Rc::new(RefCell::new(0usize));
    let col_max   = Rc::new(RefCell::new(sx.saturating_sub(1)));

    let zoom = Rc::new(RefCell::new(1.0f64));

    let px = (DISPLAY + 20.0) as i32;
    let da = DrawingArea::new();
    da.set_size_request(px, px);
    da.set_halign(gtk4::Align::Center);

    {
        let g    = game3d.clone();
        let az   = az.clone(); let el = el.clone();
        let lmin = layer_min.clone(); let lmax = layer_max.clone();
        let rmin = row_min.clone();   let rmax = row_max.clone();
        let cmin = col_min.clone();   let cmax = col_max.clone();
        let zm   = zoom.clone();
        da.set_draw_func(move |_, cr, w, h| {
            draw_board(cr, &g.borrow(), *az.borrow(), *el.borrow(), w as f64, h as f64,
                       *lmin.borrow(), *lmax.borrow(),
                       *rmin.borrow(), *rmax.borrow(),
                       *cmin.borrow(), *cmax.borrow(),
                       *zm.borrow());
        });
    }

    set_mine_label(&game3d.borrow(), &ctx.mine_label);

    let ml = ctx.mine_label.clone();
    let fb = ctx.face_button.clone();
    let st = ctx.start_time.clone();
    let sr = ctx.timer_source.clone();
    let tl = ctx.timer_label.clone();

    // Left button: GestureDrag handles drag-to-rotate AND click-to-reveal.
    {
        let da = da.clone();
        let az = az.clone();  let el = el.clone();
        let az_start = az_start.clone(); let el_start = el_start.clone();
        let press_x = press_x.clone(); let press_y = press_y.clone();
        let (ml, fb, st, sr, tl) = (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());
        let drag = GestureDrag::new();

        drag.connect_drag_begin({
            let az = az.clone(); let el = el.clone();
            let az_start = az_start.clone(); let el_start = el_start.clone();
            let press_x = press_x.clone(); let press_y = press_y.clone();
            move |gesture, _, _| {
                if let Some((x, y)) = gesture.start_point() {
                    *press_x.borrow_mut() = x;
                    *press_y.borrow_mut() = y;
                }
                *az_start.borrow_mut() = *az.borrow();
                *el_start.borrow_mut() = *el.borrow();
            }
        });

        drag.connect_drag_update({
            let az = az.clone(); let el = el.clone();
            let az_start = az_start.clone(); let el_start = el_start.clone();
            let da = da.clone();
            move |_, dx, dy| {
                if (dx * dx + dy * dy).sqrt() < CLICK_PX { return; }
                *az.borrow_mut() = *az_start.borrow() + dx * DRAG_SENS;
                *el.borrow_mut() = (*el_start.borrow() + dy * DRAG_SENS).clamp(-PI / 2.1, PI / 2.1);
                da.queue_draw();
            }
        });

        drag.connect_drag_end({
            let g    = game3d.clone();
            let da   = da.clone();
            let press_x = press_x.clone(); let press_y = press_y.clone();
            let az   = az.clone(); let el = el.clone();
            let lmin = layer_min.clone(); let lmax = layer_max.clone();
            let rmin = row_min.clone();   let rmax = row_max.clone();
            let cmin = col_min.clone();   let cmax = col_max.clone();
            let zm   = zoom.clone();
            let (ml, fb, st, sr, tl) = (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());
            move |_, dx, dy| {
                if (dx * dx + dy * dy).sqrt() >= CLICK_PX { return; }
                if !matches!(g.borrow().state, GameState::Ready | GameState::Playing) { return; }
                let mx = *press_x.borrow() + dx;
                let my = *press_y.borrow() + dy;
                let (az_v, el_v) = (*az.borrow(), *el.borrow());
                let (lmn, lmx) = (*lmin.borrow(), *lmax.borrow());
                let (rmn, rmx) = (*rmin.borrow(), *rmax.borrow());
                let (cmn, cmx) = (*cmin.borrow(), *cmax.borrow());
                let Some((x, y, z)) = hit_test_3d(
                    mx, my, &g.borrow(), az_v, el_v,
                    da.width() as f64, da.height() as f64,
                    lmn, lmx, rmn, rmx, cmn, cmx, *zm.borrow())
                else { return };

                let was_ready   = g.borrow().state == GameState::Ready;
                let is_revealed = g.borrow().grid[z][y][x].state == CellState::Revealed;
                if is_revealed {
                    if !g.borrow_mut().chord(x, y, z) { return; }
                } else {
                    g.borrow_mut().reveal(x, y, z);
                }
                if was_ready && g.borrow().state == GameState::Playing {
                    start_timer(&st, &sr, &tl);
                }
                da.queue_draw();
                let state = g.borrow().state;
                set_mine_label(&g.borrow(), &ml);
                set_face(state, &fb, &sr);
            }
        });

        da.add_controller(drag);
    }

    // Right click: flag.
    {
        let g    = game3d.clone();
        let da_c = da.clone();
        let az   = az.clone(); let el = el.clone();
        let lmin = layer_min.clone(); let lmax = layer_max.clone();
        let rmin = row_min.clone();   let rmax = row_max.clone();
        let cmin = col_min.clone();   let cmax = col_max.clone();
        let zm   = zoom.clone();
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let right = GestureClick::new();
        right.set_button(3);
        right.connect_pressed(move |gesture, _, mx, my| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(g.borrow().state, GameState::Ready | GameState::Playing) { return; }
            let (az_v, el_v) = (*az.borrow(), *el.borrow());
            let (lmn, lmx) = (*lmin.borrow(), *lmax.borrow());
            let (rmn, rmx) = (*rmin.borrow(), *rmax.borrow());
            let (cmn, cmx) = (*cmin.borrow(), *cmax.borrow());
            let Some((x, y, z)) = hit_test_3d(
                mx, my, &g.borrow(), az_v, el_v,
                da_c.width() as f64, da_c.height() as f64,
                lmn, lmx, rmn, rmx, cmn, cmx, *zm.borrow())
            else { return };
            g.borrow_mut().toggle_flag(x, y, z);
            da_c.queue_draw();
            let state = g.borrow().state;
            set_mine_label(&g.borrow(), &ml);
            set_face(state, &fb, &sr);
        });
        da.add_controller(right);
    }

    // Middle click: chord.
    {
        let g    = game3d.clone();
        let da_c = da.clone();
        let az   = az.clone(); let el = el.clone();
        let lmin = layer_min.clone(); let lmax = layer_max.clone();
        let rmin = row_min.clone();   let rmax = row_max.clone();
        let cmin = col_min.clone();   let cmax = col_max.clone();
        let zm   = zoom.clone();
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let middle = GestureClick::new();
        middle.set_button(2);
        middle.connect_pressed(move |gesture, _, mx, my| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(g.borrow().state, GameState::Playing) { return; }
            let (az_v, el_v) = (*az.borrow(), *el.borrow());
            let (lmn, lmx) = (*lmin.borrow(), *lmax.borrow());
            let (rmn, rmx) = (*rmin.borrow(), *rmax.borrow());
            let (cmn, cmx) = (*cmin.borrow(), *cmax.borrow());
            let Some((x, y, z)) = hit_test_3d(
                mx, my, &g.borrow(), az_v, el_v,
                da_c.width() as f64, da_c.height() as f64,
                lmn, lmx, rmn, rmx, cmn, cmx, *zm.borrow())
            else { return };
            if !g.borrow_mut().chord(x, y, z) { return; }
            da_c.queue_draw();
            let state = g.borrow().state;
            set_mine_label(&g.borrow(), &ml);
            set_face(state, &fb, &sr);
        });
        da.add_controller(middle);
    }

    // Scroll to zoom.
    {
        let zm   = zoom.clone();
        let da_c = da.clone();
        let scroll = EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _dx, dy| {
            let factor = if dy < 0.0 { 1.1 } else { 1.0 / 1.1 };
            *zm.borrow_mut() = (*zm.borrow() * factor).clamp(0.2, 5.0);
            da_c.queue_draw();
            glib::Propagation::Stop
        });
        da.add_controller(scroll);
    }

    // Range sliders — each axis uses its own size.
    let layer_slider = make_range_slider(sz, layer_min.clone(), layer_max.clone(), da.clone());
    let row_slider   = make_range_slider(sy, row_min.clone(),   row_max.clone(),   da.clone());
    let col_slider   = make_range_slider(sx, col_min.clone(),   col_max.clone(),   da.clone());

    let make_row = |lbl: &str, widget: &DrawingArea| -> gtk4::Box {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        row.set_margin_start(4); row.set_margin_end(4);
        let l = Label::new(Some(lbl)); l.set_width_chars(5); l.set_xalign(1.0);
        row.append(&l); row.append(widget); row
    };

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.set_halign(gtk4::Align::Center);
    vbox.append(&da);
    vbox.append(&make_row("layers", &layer_slider));
    vbox.append(&make_row("rows",   &row_slider));
    vbox.append(&make_row("cols",   &col_slider));

    vbox.upcast()
}

pub fn update_board(_game: &Rc<RefCell<crate::game::Game>>, board: &gtk4::Widget) {
    if let Some(bx) = board.downcast_ref::<gtk4::Box>() {
        if let Some(da) = bx.first_child().and_then(|w| w.downcast::<DrawingArea>().ok()) {
            da.queue_draw();
        }
    }
}
