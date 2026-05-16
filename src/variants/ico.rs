use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::f64::consts::PI;
use std::rc::Rc;

use gtk4::cairo;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{DrawingArea, EventControllerScroll, GestureClick, GestureDrag};

use crate::game::{CellState, GameState};
use super::{BoardContext, start_timer, stop_timer};

// ── Constants ────────────────────────────────────────────────────────────────
const SPHERE_R:  f64 = 140.0;
const INIT_AZ:   f64 = 0.4;
const INIT_EL:   f64 = 0.5;
const DRAG_SENS: f64 = 0.007;
const CLICK_PX:  f64 = 5.0;

// ── Cell & game ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct CellSphere {
    is_mine:  bool,
    state:    CellState,
    adjacent: u8,   // 0–3 (exactly 3 neighbours on icosphere)
}

impl CellSphere {
    fn new() -> Self { CellSphere { is_mine: false, state: CellState::Hidden, adjacent: 0 } }
}

struct GameSphere {
    cells:          Vec<CellSphere>,
    pub state:      GameState,
    mines_total:    u32,
    flags_placed:   u32,
    cells_revealed: u32,
    total_safe:     u32,
}

impl GameSphere {
    fn new(num_faces: usize, mines: u32) -> Self {
        let mines = mines.min((num_faces as u32).saturating_sub(10));
        GameSphere {
            cells:          vec![CellSphere::new(); num_faces],
            state:          GameState::Ready,
            mines_total:    mines,
            flags_placed:   0,
            cells_revealed: 0,
            total_safe:     num_faces as u32 - mines,
        }
    }

    fn place_mines(&mut self, adj: &[Vec<usize>], first: usize) {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let n = self.cells.len();
        let safe: HashSet<usize> =
            std::iter::once(first).chain(adj[first].iter().copied()).collect();
        let mut placed = 0u32;
        while placed < self.mines_total {
            let i = rng.gen_range(0..n);
            if self.cells[i].is_mine || safe.contains(&i) { continue; }
            self.cells[i].is_mine = true;
            placed += 1;
        }
        for i in 0..n {
            if self.cells[i].is_mine { continue; }
            self.cells[i].adjacent = adj[i].iter()
                .filter(|&&j| self.cells[j].is_mine)
                .count() as u8;
        }
    }

    fn reveal(&mut self, adj: &[Vec<usize>], idx: usize) {
        if self.state == GameState::Ready {
            self.place_mines(adj, idx);
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
                for &ni in &adj[ci] {
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

    fn chord(&mut self, adj: &[Vec<usize>], idx: usize) -> bool {
        if self.cells[idx].state != CellState::Revealed { return false; }
        let flags = adj[idx].iter()
            .filter(|&&j| self.cells[j].state == CellState::Flagged)
            .count() as u8;
        if flags != self.cells[idx].adjacent { return false; }
        let to: Vec<usize> = adj[idx].iter()
            .filter(|&&j| self.cells[j].state == CellState::Hidden)
            .copied().collect();
        for j in to { self.reveal(adj, j); }
        true
    }

    fn remaining_mines(&self) -> i32 { self.mines_total as i32 - self.flags_placed as i32 }
}

// ── Icosphere geometry ───────────────────────────────────────────────────────

struct SphereGeo {
    verts:     Vec<[f64; 3]>,
    faces:     Vec<[usize; 3]>,
    adj:       Vec<Vec<usize>>,   // vertex-neighbours: 9–12 per face
    centroids: Vec<[f64; 3]>,
}

impl SphereGeo {
    fn new(subdivisions: u32) -> Self {
        let (verts, faces) = make_icosphere(subdivisions);
        let adj       = build_adjacency(&faces);
        let centroids = build_centroids(&verts, &faces);
        SphereGeo { verts, faces, adj, centroids }
    }
}

fn norm3(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt().max(1e-12);
    [v[0]/len, v[1]/len, v[2]/len]
}

fn make_icosphere(subdivisions: u32) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    let t = (1.0 + 5.0_f64.sqrt()) / 2.0;
    let mut verts: Vec<[f64; 3]> = vec![
        norm3([-1.0,  t,  0.0]), norm3([ 1.0,  t,  0.0]),
        norm3([-1.0, -t,  0.0]), norm3([ 1.0, -t,  0.0]),
        norm3([ 0.0, -1.0,  t]), norm3([ 0.0,  1.0,  t]),
        norm3([ 0.0, -1.0, -t]), norm3([ 0.0,  1.0, -t]),
        norm3([ t,  0.0, -1.0]), norm3([ t,  0.0,  1.0]),
        norm3([-t,  0.0, -1.0]), norm3([-t,  0.0,  1.0]),
    ];
    let mut faces: Vec<[usize; 3]> = vec![
        [0,11,5],[0,5,1],[0,1,7],[0,7,10],[0,10,11],
        [1,5,9],[5,11,4],[11,10,2],[10,7,6],[7,1,8],
        [3,9,4],[3,4,2],[3,2,6],[3,6,8],[3,8,9],
        [4,9,5],[2,4,11],[6,2,10],[8,6,7],[9,8,1],
    ];
    for _ in 0..subdivisions {
        let mut cache: HashMap<(usize,usize),usize> = HashMap::new();
        let mut nf = Vec::with_capacity(faces.len() * 4);
        for &[a,b,c] in &faces {
            let ab = midpt(&mut verts, &mut cache, a, b);
            let bc = midpt(&mut verts, &mut cache, b, c);
            let ca = midpt(&mut verts, &mut cache, c, a);
            nf.push([a,ab,ca]); nf.push([b,bc,ab]);
            nf.push([c,ca,bc]); nf.push([ab,bc,ca]);
        }
        faces = nf;
    }
    (verts, faces)
}

fn midpt(verts: &mut Vec<[f64;3]>, cache: &mut HashMap<(usize,usize),usize>, a: usize, b: usize) -> usize {
    let key = (a.min(b), a.max(b));
    if let Some(&i) = cache.get(&key) { return i; }
    let va = verts[a]; let vb = verts[b];
    let m = norm3([(va[0]+vb[0])*0.5, (va[1]+vb[1])*0.5, (va[2]+vb[2])*0.5]);
    let i = verts.len(); verts.push(m); cache.insert(key, i); i
}

/// All faces sharing any vertex with `fi` (edge- AND corner-neighbours).
/// Typical icosphere: degree-6 vertex → 12 neighbours; degree-5 → 11 or fewer.
fn build_adjacency(faces: &[[usize; 3]]) -> Vec<Vec<usize>> {
    // vertex → all faces that contain it
    let mut vmap: HashMap<usize, Vec<usize>> = HashMap::new();
    for (fi, face) in faces.iter().enumerate() {
        for &v in face {
            vmap.entry(v).or_default().push(fi);
        }
    }
    let mut adj = vec![Vec::new(); faces.len()];
    for (fi, face) in faces.iter().enumerate() {
        let mut seen = HashSet::new();
        for &v in face {
            if let Some(lst) = vmap.get(&v) {
                for &nfi in lst {
                    if nfi != fi { seen.insert(nfi); }
                }
            }
        }
        let mut nb: Vec<usize> = seen.into_iter().collect();
        nb.sort_unstable();
        adj[fi] = nb;
    }
    adj
}

fn build_centroids(verts: &[[f64;3]], faces: &[[usize;3]]) -> Vec<[f64;3]> {
    faces.iter().map(|&[a,b,c]| {
        let (va,vb,vc) = (verts[a], verts[b], verts[c]);
        norm3([(va[0]+vb[0]+vc[0])/3.0, (va[1]+vb[1]+vc[1])/3.0, (va[2]+vb[2]+vc[2])/3.0])
    }).collect()
}

// ── Projection ───────────────────────────────────────────────────────────────

/// Rotate world-space point to view space: [screen_x, screen_y, depth].
/// depth > 0 → closer to viewer (viewer on +Z side).
fn rot(v: [f64;3], az: f64, el: f64) -> [f64;3] {
    let rx  =  v[0]*az.cos() + v[2]*az.sin();
    let rz  = -v[0]*az.sin() + v[2]*az.cos();
    let ry2 =  v[1]*el.cos() - rz*el.sin();
    let rz2 =  v[1]*el.sin() + rz*el.cos();
    [rx, -ry2, rz2]
}

fn screen(rv: [f64;3], sc: f64, cx: f64, cy: f64) -> (f64, f64) {
    (cx + rv[0]*sc, cy + rv[1]*sc)
}

/// View direction: camera looks FROM this direction INTO the scene.
fn view_dir(az: f64, el: f64) -> [f64;3] {
    [-az.sin()*el.cos(), el.sin(), az.cos()*el.cos()]
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn num_color(n: u8) -> (f64, f64, f64) {
    match n {
        1  => (0.25, 0.60, 1.00),
        2  => (0.10, 0.92, 0.20),
        3  => (1.00, 0.25, 0.25),
        4  => (0.20, 0.20, 0.95),
        5  => (0.90, 0.40, 0.00),
        6  => (0.00, 0.80, 0.80),
        7  => (0.85, 0.85, 0.85),
        8  => (0.50, 0.50, 0.50),
        9  => (0.00, 0.65, 1.00),
        10 => (0.00, 0.95, 0.00),
        11 => (1.00, 0.20, 0.80),
        12 => (1.00, 1.00, 0.20),
        _  => (1.00, 0.60, 1.00),
    }
}

fn draw_text(cr: &cairo::Context, cx: f64, cy: f64, text: &str,
             r: f64, g: f64, b: f64, a: f64, font_px: f64) {
    if font_px < 3.0 { return; }
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_text(text);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Sans");
    fd.set_weight(pango::Weight::Bold);
    let sz = ((font_px * 0.72 * pango::SCALE as f64) as i32).max(1);
    fd.set_size(sz);
    layout.set_font_description(Some(&fd));
    let (lw, lh) = layout.pixel_size();
    cr.move_to(cx - lw as f64 / 2.0, cy - lh as f64 / 2.0);
    cr.set_source_rgba(r, g, b, a);
    pangocairo::functions::show_layout(cr, &layout);
}

fn in_tri(p: (f64,f64), a: (f64,f64), b: (f64,f64), c: (f64,f64)) -> bool {
    let cross = |o: (f64,f64), u: (f64,f64), v: (f64,f64)|
        (v.0-u.0)*(o.1-u.1) - (v.1-u.1)*(o.0-u.0);
    let (d1,d2,d3) = (cross(p,a,b), cross(p,b,c), cross(p,c,a));
    !((d1<0.0 || d2<0.0 || d3<0.0) && (d1>0.0 || d2>0.0 || d3>0.0))
}

fn draw_sphere(
    cr: &cairo::Context, game: &GameSphere, geo: &SphereGeo,
    az: f64, el: f64, w: f64, h: f64, zoom: f64,
    tc: &super::ThemeColors,
) {
    let [bgr, bgg, bgb] = tc.bg;
    cr.set_source_rgb(bgr, bgg, bgb);
    let _ = cr.paint();

    let sc  = SPHERE_R * zoom;
    let cx  = w / 2.0;
    let cy  = h / 2.0;
    let vd  = view_dir(az, el);
    // Font size scales with triangle edge length ≈ sc * 2 / sqrt(num_faces).
    let font_px = (sc * 2.0 / (geo.faces.len() as f64).sqrt()).clamp(4.0, 22.0);

    // Collect front-facing faces with depth.
    let mut visible: Vec<(f64, usize)> = Vec::with_capacity(geo.faces.len() / 2 + 1);
    for (fi, cent) in geo.centroids.iter().enumerate() {
        let dot = cent[0]*vd[0] + cent[1]*vd[1] + cent[2]*vd[2];
        if dot <= 0.02 { continue; }
        let rv = rot(*cent, az, el);
        visible.push((rv[2], fi));  // depth: larger = closer to viewer
    }

    // Painter's algorithm: ascending = farthest first (viewer on +Z side).
    visible.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    for &(_, fi) in &visible {
        let cell = &game.cells[fi];
        let [ia,ib,ic] = geo.faces[fi];

        let ra = rot(geo.verts[ia], az, el);
        let rb = rot(geo.verts[ib], az, el);
        let rc = rot(geo.verts[ic], az, el);

        let pa = screen(ra, sc, cx, cy);
        let pb = screen(rb, sc, cx, cy);
        let pc = screen(rc, sc, cx, cy);

        // Lighting from rotated face normal (= rotated centroid on unit sphere).
        let rn = rot(geo.centroids[fi], az, el);
        // Shading: brighter when facing viewer (+rn[2]) + slight top light (-rn[1]).
        let shade = (0.35 + 0.50 * rn[2].max(0.0) + 0.15 * (-rn[1]).max(0.0)).clamp(0.18, 1.0);

        let hidden_rgb = super::mix3(tc.bg, tc.fg, 0.25);
        let revd_rgb   = super::mix3(tc.bg, tc.fg, 0.10);
        let (fr, fg, fb, fa) = match cell.state {
            CellState::Hidden | CellState::Flagged =>
                (hidden_rgb[0]*shade, hidden_rgb[1]*shade, hidden_rgb[2]*shade, 1.0_f64),
            CellState::Revealed if cell.is_mine =>
                (0.65*shade, 0.18*shade, 0.18*shade, 1.0),
            CellState::Revealed =>
                (revd_rgb[0]*shade, revd_rgb[1]*shade, revd_rgb[2]*shade, 0.92),
            _ => (hidden_rgb[0]*shade, hidden_rgb[1]*shade, hidden_rgb[2]*shade, 1.0),
        };

        cr.set_source_rgba(fr, fg, fb, fa);
        cr.move_to(pa.0, pa.1);
        cr.line_to(pb.0, pb.1);
        cr.line_to(pc.0, pc.1);
        cr.close_path();
        let _ = cr.fill();

        // Wireframe edge.
        let [wr, wg, wb] = super::mix3(tc.bg, tc.fg, 0.50);
        cr.set_source_rgba(wr, wg, wb, 0.22);
        cr.set_line_width(0.5);
        cr.move_to(pa.0, pa.1);
        cr.line_to(pb.0, pb.1);
        cr.line_to(pc.0, pc.1);
        cr.close_path();
        let _ = cr.stroke();

        // Content at centroid.
        let fcx = (pa.0 + pb.0 + pc.0) / 3.0;
        let fcy = (pa.1 + pb.1 + pc.1) / 3.0;
        match cell.state {
            CellState::Flagged =>
                draw_text(cr, fcx, fcy, "🚩", 1.0, 0.6, 0.0, 1.0, font_px),
            CellState::Revealed if cell.is_mine =>
                draw_text(cr, fcx, fcy, "💣", 0.9, 0.9, 0.9, 1.0, font_px),
            CellState::Revealed if cell.adjacent > 0 => {
                let (nr, ng, nb_) = num_color(cell.adjacent);
                draw_text(cr, fcx, fcy, &cell.adjacent.to_string(), nr, ng, nb_, 1.0, font_px);
            }
            _ => {}
        }
    }

    if game.state == GameState::Ready {
        let layout = pangocairo::functions::create_layout(cr);
        layout.set_text("drag to rotate  •  click to reveal");
        let mut fd = pango::FontDescription::new();
        fd.set_family("Sans");
        fd.set_size(8 * pango::SCALE);
        layout.set_font_description(Some(&fd));
        let (lw, _) = layout.pixel_size();
        cr.move_to(w / 2.0 - lw as f64 / 2.0, h - 16.0);
        let [fgr, fgg, fgb] = tc.fg;
        cr.set_source_rgba(fgr, fgg, fgb, 0.5);
        pangocairo::functions::show_layout(cr, &layout);
    }
}

fn hit_test(
    mx: f64, my: f64,
    geo: &SphereGeo,
    az: f64, el: f64, w: f64, h: f64, zoom: f64,
) -> Option<usize> {
    let sc = SPHERE_R * zoom;
    let cx = w / 2.0; let cy = h / 2.0;
    let vd = view_dir(az, el);
    let mut best: Option<(f64, usize)> = None;

    for (fi, cent) in geo.centroids.iter().enumerate() {
        let dot = cent[0]*vd[0] + cent[1]*vd[1] + cent[2]*vd[2];
        if dot <= 0.0 { continue; }

        let [ia,ib,ic] = geo.faces[fi];
        let pa = screen(rot(geo.verts[ia], az, el), sc, cx, cy);
        let pb = screen(rot(geo.verts[ib], az, el), sc, cx, cy);
        let pc = screen(rot(geo.verts[ic], az, el), sc, cx, cy);

        if !in_tri((mx, my), pa, pb, pc) { continue; }

        let depth = rot(*cent, az, el)[2];
        if best.map_or(true, |(d,_)| depth > d) { best = Some((depth, fi)); }
    }
    best.map(|(_,fi)| fi)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn set_mine_lbl(game: &GameSphere, ml: &Rc<RefCell<Option<gtk4::Label>>>) {
    if let Some(lbl) = ml.borrow().as_ref() {
        lbl.set_text(&format!("{:03}", game.remaining_mines()));
    }
}

fn set_face(state: GameState, fb: &Rc<RefCell<Option<gtk4::Button>>>, sr: &Rc<RefCell<Option<glib::SourceId>>>) {
    if let Some(btn) = fb.borrow().as_ref() {
        match state {
            GameState::Won  => { btn.set_label("\u{1F60E}"); stop_timer(sr); }
            GameState::Lost => { btn.set_label("\u{1F635}"); stop_timer(sr); }
            _               => {}
        }
    }
}

// ── Board creation ────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    // Custom(subs, 9999, mines) sentinel: height==9999 means user set subdivisions directly.
    let (subdivisions, mines) = if height == 9999 {
        (width as u32, mine_count as u32)
    } else {
        let subs: u32 = if width * height <= 81 { 2 } else { 3 };
        let tmp_geo   = SphereGeo::new(subs);
        let nf        = tmp_geo.faces.len();
        let density   = mine_count as f64 / (width * height) as f64;
        let m         = ((nf as f64 * density) as u32).clamp(5, nf as u32 / 3);
        (subs, m)
    };
    let geo    = Rc::new(SphereGeo::new(subdivisions));
    let nfaces = geo.faces.len();

    let game     = Rc::new(RefCell::new(GameSphere::new(nfaces, mines)));
    let az       = Rc::new(RefCell::new(INIT_AZ));
    let el       = Rc::new(RefCell::new(INIT_EL));
    let az_start = Rc::new(RefCell::new(INIT_AZ));
    let el_start = Rc::new(RefCell::new(INIT_EL));
    let press_x  = Rc::new(RefCell::new(0.0_f64));
    let press_y  = Rc::new(RefCell::new(0.0_f64));
    let zoom     = Rc::new(RefCell::new(1.0_f64));

    let px = (SPHERE_R * 2.0 + 40.0) as i32;
    let da = DrawingArea::new();
    da.set_size_request(px, px);
    da.set_halign(gtk4::Align::Center);

    // Draw.
    {
        let (g, geo, az, el, zm) = (game.clone(), geo.clone(), az.clone(), el.clone(), zoom.clone());
        da.set_draw_func(move |widget, cr, w, h| {
            let tc = super::get_theme_colors(widget);
            draw_sphere(cr, &g.borrow(), &geo, *az.borrow(), *el.borrow(), w as f64, h as f64, *zm.borrow(), &tc);
        });
    }

    let ml = ctx.mine_label.clone();
    let fb = ctx.face_button.clone();
    let st = ctx.start_time.clone();
    let sr = ctx.timer_source.clone();
    let tl = ctx.timer_label.clone();

    set_mine_lbl(&game.borrow(), &ml);

    // ── Left drag: rotate + click-to-reveal ─────────────────────────────────
    {
        let (da, az, el, az_s, el_s, px_r, py_r) = (
            da.clone(), az.clone(), el.clone(),
            az_start.clone(), el_start.clone(),
            press_x.clone(), press_y.clone(),
        );
        let (ml, fb, st, sr, tl) = (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());
        let (geo, game, zm) = (geo.clone(), game.clone(), zoom.clone());
        let drag = GestureDrag::new();

        drag.connect_drag_begin({
            let (az, el, az_s, el_s, px_r, py_r) = (az.clone(), el.clone(), az_s.clone(), el_s.clone(), px_r.clone(), py_r.clone());
            move |gesture, _, _| {
                if let Some((x,y)) = gesture.start_point() {
                    *px_r.borrow_mut() = x; *py_r.borrow_mut() = y;
                }
                *az_s.borrow_mut() = *az.borrow();
                *el_s.borrow_mut() = *el.borrow();
            }
        });

        drag.connect_drag_update({
            let (az, el, az_s, el_s, da) = (az.clone(), el.clone(), az_s.clone(), el_s.clone(), da.clone());
            move |_, dx, dy| {
                if (dx*dx + dy*dy).sqrt() < CLICK_PX { return; }
                *az.borrow_mut() = *az_s.borrow() + dx * DRAG_SENS;
                *el.borrow_mut() = (*el_s.borrow() + dy * DRAG_SENS).clamp(-PI/2.1, PI/2.1);
                da.queue_draw();
            }
        });

        drag.connect_drag_end({
            let (da, az, el, px_r, py_r, geo, game, zm) = (
                da.clone(), az.clone(), el.clone(), px_r.clone(), py_r.clone(),
                geo.clone(), game.clone(), zm.clone(),
            );
            let (ml, fb, st, sr, tl) = (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());
            move |_, dx, dy| {
                if (dx*dx + dy*dy).sqrt() >= CLICK_PX { return; }
                if !matches!(game.borrow().state, GameState::Ready | GameState::Playing) { return; }
                let mx = *px_r.borrow() + dx;
                let my = *py_r.borrow() + dy;
                let (az_v, el_v, zm_v) = (*az.borrow(), *el.borrow(), *zm.borrow());
                let fi = hit_test(mx, my, &geo,
                                  az_v, el_v, da.width() as f64, da.height() as f64, zm_v);
                let Some(fi) = fi else { return };

                let was_ready   = game.borrow().state == GameState::Ready;
                let is_revealed = game.borrow().cells[fi].state == CellState::Revealed;
                if is_revealed {
                    if !game.borrow_mut().chord(&geo.adj, fi) { return; }
                } else {
                    game.borrow_mut().reveal(&geo.adj, fi);
                }
                if was_ready && game.borrow().state == GameState::Playing {
                    start_timer(&st, &sr, &tl);
                }
                da.queue_draw();
                let state = game.borrow().state;
                set_mine_lbl(&game.borrow(), &ml);
                set_face(state, &fb, &sr);
            }
        });

        da.add_controller(drag);
    }

    // ── Right click: flag ────────────────────────────────────────────────────
    {
        let (da_c, az, el, zm, geo, game) = (
            da.clone(), az.clone(), el.clone(), zoom.clone(), geo.clone(), game.clone(),
        );
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let right = GestureClick::new();
        right.set_button(3);
        right.connect_pressed(move |gesture, _, mx, my| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game.borrow().state, GameState::Ready | GameState::Playing) { return; }
            let (az_v, el_v, zm_v) = (*az.borrow(), *el.borrow(), *zm.borrow());
            let fi = hit_test(mx, my, &geo,
                              az_v, el_v, da_c.width() as f64, da_c.height() as f64, zm_v);
            let Some(fi) = fi else { return };
            game.borrow_mut().toggle_flag(fi);
            da_c.queue_draw();
            let state = game.borrow().state;
            set_mine_lbl(&game.borrow(), &ml);
            set_face(state, &fb, &sr);
        });
        da.add_controller(right);
    }

    // ── Middle click: chord ──────────────────────────────────────────────────
    {
        let (da_c, az, el, zm, geo, game) = (
            da.clone(), az.clone(), el.clone(), zoom.clone(), geo.clone(), game.clone(),
        );
        let (ml, fb, sr) = (ml.clone(), fb.clone(), sr.clone());
        let middle = GestureClick::new();
        middle.set_button(2);
        middle.connect_pressed(move |gesture, _, mx, my| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game.borrow().state, GameState::Playing) { return; }
            let (az_v, el_v, zm_v) = (*az.borrow(), *el.borrow(), *zm.borrow());
            let fi = hit_test(mx, my, &geo,
                              az_v, el_v, da_c.width() as f64, da_c.height() as f64, zm_v);
            let Some(fi) = fi else { return };
            if !game.borrow_mut().chord(&geo.adj, fi) { return; }
            da_c.queue_draw();
            let state = game.borrow().state;
            set_mine_lbl(&game.borrow(), &ml);
            set_face(state, &fb, &sr);
        });
        da.add_controller(middle);
    }

    // ── Scroll: zoom ─────────────────────────────────────────────────────────
    {
        let (zm, da_c) = (zoom.clone(), da.clone());
        let scroll = EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _dx, dy| {
            let factor = if dy < 0.0 { 1.1 } else { 1.0 / 1.1 };
            let new_zoom = (*zm.borrow() * factor).clamp(0.3, 4.0);
            *zm.borrow_mut() = new_zoom;
            da_c.queue_draw();
            glib::Propagation::Stop
        });
        da.add_controller(scroll);
    }

    da.upcast()
}

pub fn update_board(_game: &Rc<RefCell<crate::game::Game>>, board: &gtk4::Widget) {
    if let Some(da) = board.downcast_ref::<DrawingArea>() {
        da.queue_draw();
    }
}
