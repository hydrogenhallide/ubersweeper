use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Button, DrawingArea, GestureClick, Grid, Overlay};
use pangocairo::cairo;
use rand::Rng;
use rand::seq::SliceRandom;

use crate::constants::CELL_SIZE;
use super::{BoardContext, start_timer, stop_timer};

// ── AI difficulty ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum AiLevel { Easy, Medium, Hard }

impl AiLevel {
    fn from_board(total: usize) -> Self {
        match total {
            0..=100  => AiLevel::Easy,
            101..=300 => AiLevel::Medium,
            _         => AiLevel::Hard,
        }
    }
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Owner { Player, Ai }

#[derive(Clone, Copy, PartialEq)]
enum PvAiState { Ready, Playing, PlayerWon, AiWon }

#[derive(Clone, Copy, Default)]
struct Cell {
    is_mine:     bool,
    adj:         u8,
    revealed:    bool,
    player_flag: bool,
}

struct Constraint {
    cells: Vec<(usize, usize)>,
    mines: usize,
}

// ── Game ──────────────────────────────────────────────────────────────────────

struct PvAiGame {
    grid:         Vec<Vec<Cell>>,
    width:        usize,
    height:       usize,
    mine_count:   usize,
    state:        PvAiState,
    ownership:    HashMap<(usize, usize), Owner>,
    ai_flags:     HashSet<(usize, usize)>,
    player_score: usize,
    ai_score:     usize,
    ai_corner:    (usize, usize),
    level:        AiLevel,
    ai_pending:   bool,
}

impl PvAiGame {
    fn new(width: usize, height: usize, mine_count: usize) -> Self {
        PvAiGame {
            grid:         vec![vec![Cell::default(); width]; height],
            width, height, mine_count,
            state:        PvAiState::Ready,
            ownership:    HashMap::new(),
            ai_flags:     HashSet::new(),
            player_score: 0,
            ai_score:     0,
            ai_corner:    (width.saturating_sub(1), height.saturating_sub(1)),
            level:        AiLevel::from_board(width * height),
            ai_pending:   false,
        }
    }

    fn place_mines(&mut self, px: usize, py: usize) {
        let corners = [
            (0usize, 0usize),
            (self.width.saturating_sub(1), 0),
            (0, self.height.saturating_sub(1)),
            (self.width.saturating_sub(1), self.height.saturating_sub(1)),
        ];
        self.ai_corner = *corners.iter()
            .max_by_key(|&&(cx, cy)| px.abs_diff(cx) + py.abs_diff(cy))
            .unwrap();
        let (ax, ay) = self.ai_corner;

        let mut safe: HashSet<(usize, usize)> = HashSet::new();
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                for &(bx, by) in &[(px, py), (ax, ay)] {
                    let nx = bx as i32 + dx;
                    let ny = by as i32 + dy;
                    if nx >= 0 && ny >= 0 && (nx as usize) < self.width && (ny as usize) < self.height {
                        safe.insert((nx as usize, ny as usize));
                    }
                }
            }
        }

        let mut rng = rand::thread_rng();
        let mut cands: Vec<(usize, usize)> = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|p| !safe.contains(p))
            .collect();
        cands.shuffle(&mut rng);

        for &(x, y) in cands.iter().take(self.mine_count.min(cands.len())) {
            self.grid[y][x].is_mine = true;
        }
        for y in 0..self.height {
            for x in 0..self.width {
                self.grid[y][x].adj = self.count_adj(x, y);
            }
        }
    }

    fn count_adj(&self, x: usize, y: usize) -> u8 {
        let mut n = 0u8;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && ny >= 0 && (nx as usize) < self.width && (ny as usize) < self.height
                    && self.grid[ny as usize][nx as usize].is_mine { n += 1; }
            }
        }
        n
    }

    fn reveal_cell(&mut self, x: usize, y: usize, owner: Owner) -> bool {
        let cell = &self.grid[y][x];
        if cell.revealed || cell.player_flag || self.ai_flags.contains(&(x, y)) { return false; }

        self.grid[y][x].revealed = true;
        self.ownership.insert((x, y), owner);
        match owner { Owner::Player => self.player_score += 1, Owner::Ai => self.ai_score += 1 }

        if self.grid[y][x].is_mine {
            for row in &mut self.grid { for c in row { if c.is_mine { c.revealed = true; } } }
            self.state = match owner { Owner::Player => PvAiState::AiWon, Owner::Ai => PvAiState::PlayerWon };
            return true;
        }
        if self.grid[y][x].adj == 0 { self.cascade(x, y, owner); }
        true
    }

    fn cascade(&mut self, sx: usize, sy: usize, owner: Owner) {
        let mut q: VecDeque<(usize, usize)> = VecDeque::new();
        q.push_back((sx, sy));
        while let Some((cx, cy)) = q.pop_front() {
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = cx as i32 + dx;
                    let ny = cy as i32 + dy;
                    if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height { continue; }
                    let (nx, ny) = (nx as usize, ny as usize);
                    let c = &self.grid[ny][nx];
                    if c.revealed || c.is_mine || c.player_flag || self.ai_flags.contains(&(nx, ny)) { continue; }
                    self.grid[ny][nx].revealed = true;
                    self.ownership.insert((nx, ny), owner);
                    match owner { Owner::Player => self.player_score += 1, Owner::Ai => self.ai_score += 1 }
                    if self.grid[ny][nx].adj == 0 { q.push_back((nx, ny)); }
                }
            }
        }
    }

    // Called on the player's first click; also gives AI its corner reveal.
    fn player_first(&mut self, x: usize, y: usize) {
        self.place_mines(x, y);
        self.state = PvAiState::Playing;
        // Player cascades first — AI never steals player cells on the opening
        self.reveal_cell(x, y, Owner::Player);
        if self.state != PvAiState::Playing { return; }
        self.check_win();
        if self.state != PvAiState::Playing { return; }
        // AI corner is its "turn 1"; runs after player's cascade is fully counted
        let (ax, ay) = self.ai_corner;
        self.reveal_cell(ax, ay, Owner::Ai);
        self.check_win();
    }

    fn player_reveal(&mut self, x: usize, y: usize) {
        if self.state != PvAiState::Playing { return; }
        self.reveal_cell(x, y, Owner::Player);
        self.check_win();
    }

    fn player_flag(&mut self, x: usize, y: usize) {
        if self.state != PvAiState::Playing { return; }
        let c = &self.grid[y][x];
        if c.revealed || self.ai_flags.contains(&(x, y)) { return; }
        self.grid[y][x].player_flag = !c.player_flag;
    }

    fn player_chord(&mut self, x: usize, y: usize) {
        if self.state != PvAiState::Playing { return; }
        let c = &self.grid[y][x];
        if !c.revealed || c.is_mine || c.adj == 0 { return; }
        if self.flags_around(x, y) != c.adj { return; }
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height { continue; }
                let (nx, ny) = (nx as usize, ny as usize);
                let c = &self.grid[ny][nx];
                if !c.revealed && !c.player_flag && !self.ai_flags.contains(&(nx, ny)) {
                    self.reveal_cell(nx, ny, Owner::Player);
                    if self.state != PvAiState::Playing { return; }
                }
            }
        }
        self.check_win();
    }

    fn flags_around(&self, x: usize, y: usize) -> u8 {
        let mut n = 0u8;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height { continue; }
                let (nx, ny) = (nx as usize, ny as usize);
                if self.grid[ny][nx].player_flag || self.ai_flags.contains(&(nx, ny)) { n += 1; }
            }
        }
        n
    }

    fn check_win(&mut self) {
        if self.state != PvAiState::Playing { return; }
        if self.grid.iter().flatten().all(|c| c.revealed || c.is_mine) {
            self.state = if self.player_score >= self.ai_score {
                PvAiState::PlayerWon
            } else {
                PvAiState::AiWon
            };
        }
    }

    // ── AI solver ─────────────────────────────────────────────────────────────

    fn build_constraints_with(&self, virtual_mines: &HashSet<(usize,usize)>) -> Vec<Constraint> {
        let mut out = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let cell = &self.grid[y][x];
                if !cell.revealed || cell.is_mine || cell.adj == 0 { continue; }
                let mut flagged = 0usize;
                let mut hidden = Vec::new();
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 { continue; }
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height { continue; }
                        let (nx, ny) = (nx as usize, ny as usize);
                        if self.grid[ny][nx].player_flag
                            || self.ai_flags.contains(&(nx, ny))
                            || virtual_mines.contains(&(nx, ny))
                        {
                            flagged += 1;
                        } else if !self.grid[ny][nx].revealed {
                            hidden.push((nx, ny));
                        }
                    }
                }
                let mines = (cell.adj as usize).saturating_sub(flagged);
                if !hidden.is_empty() {
                    out.push(Constraint { cells: hidden, mines });
                }
            }
        }
        out
    }

    fn local_pass(cs: &[Constraint], safe: &mut HashSet<(usize,usize)>, mines: &mut HashSet<(usize,usize)>) {
        for c in cs {
            if c.cells.is_empty() { continue; }
            if c.mines == 0 { safe.extend(&c.cells); }
            else if c.mines == c.cells.len() { mines.extend(&c.cells); }
        }
    }

    fn subset_pass(cs: &[Constraint]) -> Vec<Constraint> {
        let mut derived = Vec::new();
        for (i, c1) in cs.iter().enumerate() {
            let s1: HashSet<_> = c1.cells.iter().copied().collect();
            for c2 in &cs[i+1..] {
                let s2: HashSet<_> = c2.cells.iter().copied().collect();
                if s1.is_subset(&s2) && c1.mines <= c2.mines {
                    let diff: Vec<_> = s2.difference(&s1).copied().collect();
                    if !diff.is_empty() { derived.push(Constraint { cells: diff, mines: c2.mines - c1.mines }); }
                } else if s2.is_subset(&s1) && c2.mines <= c1.mines {
                    let diff: Vec<_> = s1.difference(&s2).copied().collect();
                    if !diff.is_empty() { derived.push(Constraint { cells: diff, mines: c1.mines - c2.mines }); }
                }
            }
        }
        derived
    }

    // Iterative solver: deduced mines are fed back as virtual flags so downstream
    // constraints can resolve (handles patterns like 1-2-1 on a flat edge).
    fn solve(&self) -> (Vec<(usize,usize)>, Vec<(usize,usize)>) {
        let mut safe_s: HashSet<(usize,usize)> = HashSet::new();
        let mut mine_s: HashSet<(usize,usize)> = HashSet::new();

        for _ in 0..10 {
            let prev = (safe_s.len(), mine_s.len());

            let mut cs = self.build_constraints_with(&mine_s);
            Self::local_pass(&cs, &mut safe_s, &mut mine_s);

            if self.level >= AiLevel::Medium {
                let d = Self::subset_pass(&cs);
                Self::local_pass(&d, &mut safe_s, &mut mine_s);
                cs.extend(d);
            }

            if self.level >= AiLevel::Hard {
                for _ in 0..3 {
                    let inner_prev = safe_s.len() + mine_s.len();
                    let d = Self::subset_pass(&cs);
                    Self::local_pass(&d, &mut safe_s, &mut mine_s);
                    cs.extend(d);
                    if safe_s.len() + mine_s.len() == inner_prev { break; }
                }
            }

            if (safe_s.len(), mine_s.len()) == prev { break; }
        }

        // Remove cells already known to the game state
        safe_s.retain(|&(x, y)| !self.grid[y][x].revealed && !self.grid[y][x].player_flag && !self.ai_flags.contains(&(x, y)));
        mine_s.retain(|p| !self.ai_flags.contains(p) && !self.grid[p.1][p.0].player_flag);
        (safe_s.into_iter().collect(), mine_s.into_iter().collect())
    }

    fn pick_safe(&self, safe: &[(usize,usize)]) -> (usize, usize) {
        match self.level {
            AiLevel::Easy   => safe[rand::thread_rng().gen_range(0..safe.len())],
            AiLevel::Medium => *safe.iter().max_by_key(|&&(x,y)| self.neighbors(x, y, |c, _| c.revealed)).unwrap(),
            AiLevel::Hard   => *safe.iter().max_by_key(|&&(x,y)| self.neighbors(x, y, |c, _| !c.revealed)).unwrap(),
        }
    }

    fn pick_guess(&self) -> Option<(usize, usize)> {
        let cands: Vec<_> = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let c = &self.grid[y][x];
                !c.revealed && !c.player_flag && !self.ai_flags.contains(&(x, y))
            })
            .collect();
        if cands.is_empty() { return None; }
        Some(match self.level {
            AiLevel::Easy   => cands[rand::thread_rng().gen_range(0..cands.len())],
            AiLevel::Medium => *cands.iter().max_by_key(|&&(x,y)| self.neighbors(x, y, |c, _| c.revealed)).unwrap(),
            AiLevel::Hard   => {
                let cs = self.build_constraints_with(&HashSet::new());
                *cands.iter().min_by(|&&(ax,ay), &&(bx,by)| {
                    self.mine_prob(ax, ay, &cs).partial_cmp(&self.mine_prob(bx, by, &cs)).unwrap()
                }).unwrap()
            }
        })
    }

    fn mine_prob(&self, x: usize, y: usize, cs: &[Constraint]) -> f64 {
        let rel: Vec<_> = cs.iter().filter(|c| c.cells.contains(&(x,y)) && !c.cells.is_empty()).collect();
        if rel.is_empty() {
            let hidden = self.grid.iter().flatten().filter(|c| !c.revealed && !c.player_flag).count();
            let flagged = self.ai_flags.len() + self.grid.iter().flatten().filter(|c| c.player_flag).count();
            return self.mine_count.saturating_sub(flagged) as f64 / hidden.max(1) as f64;
        }
        rel.iter().map(|c| c.mines as f64 / c.cells.len() as f64).sum::<f64>() / rel.len() as f64
    }

    fn neighbors(&self, x: usize, y: usize, pred: impl Fn(&Cell, (usize,usize)) -> bool) -> usize {
        let mut n = 0;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && ny >= 0 && (nx as usize) < self.width && (ny as usize) < self.height {
                    let (nx, ny) = (nx as usize, ny as usize);
                    if pred(&self.grid[ny][nx], (nx, ny)) { n += 1; }
                }
            }
        }
        n
    }

    /// Find a cell the AI can chord using placed flags + deduced mine positions.
    fn find_chord(&self, deduced: &HashSet<(usize, usize)>) -> Option<(usize, usize)> {
        for y in 0..self.height {
            for x in 0..self.width {
                let c = &self.grid[y][x];
                if !c.revealed || c.is_mine || c.adj == 0 { continue; }
                let covered = self.neighbors(x, y, |_, p| {
                    self.ai_flags.contains(&p) || deduced.contains(&p)
                }) as u8;
                let has_safe = self.neighbors(x, y, |c, p| {
                    !c.revealed && !c.player_flag
                        && !self.ai_flags.contains(&p)
                        && !deduced.contains(&p)
                }) > 0;
                if covered == c.adj && has_safe { return Some((x, y)); }
            }
        }
        None
    }

    fn do_chord(&mut self, x: usize, y: usize, deduced: &HashSet<(usize, usize)>) {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height { continue; }
                let (nx, ny) = (nx as usize, ny as usize);
                let c = &self.grid[ny][nx];
                if !c.revealed && !c.player_flag
                    && !self.ai_flags.contains(&(nx, ny))
                    && !deduced.contains(&(nx, ny))
                {
                    self.reveal_cell(nx, ny, Owner::Ai);
                    if self.state != PvAiState::Playing { return; }
                }
            }
        }
    }

    fn ai_turn(&mut self) {
        if self.state != PvAiState::Playing { return; }
        let (safe, mine_cells) = self.solve();
        let deduced: HashSet<(usize, usize)> = mine_cells.into_iter().collect();

        // Priority 1: chord using deduced mine knowledge (no flag placement needed)
        if let Some((cx, cy)) = self.find_chord(&deduced) {
            self.do_chord(cx, cy, &deduced);
            self.check_win();
            return;
        }

        // Priority 2: reveal a known-safe cell
        if !safe.is_empty() {
            let (x, y) = self.pick_safe(&safe);
            self.reveal_cell(x, y, Owner::Ai);
            self.check_win();
            return;
        }

        // Priority 3: guess
        if let Some((x, y)) = self.pick_guess() {
            self.reveal_cell(x, y, Owner::Ai);
            self.check_win();
        }
    }
}

// ── Score / face helpers ──────────────────────────────────────────────────────

fn update_score(game: &PvAiGame, ml: &Rc<RefCell<Option<gtk4::Label>>>) {
    if let Some(lbl) = ml.borrow().as_ref() {
        lbl.set_text(&format!("P:{} A:{}", game.player_score, game.ai_score));
    }
}

fn update_face(game: &PvAiGame, fb: &Rc<RefCell<Option<Button>>>, sr: &Rc<RefCell<Option<glib::SourceId>>>) {
    if let Some(btn) = fb.borrow().as_ref() {
        match game.state {
            PvAiState::PlayerWon => { btn.set_label("\u{1F60E}"); stop_timer(sr); }
            PvAiState::AiWon     => { btn.set_label("\u{1F635}"); stop_timer(sr); }
            _                    => btn.set_label("\u{1F642}"),
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_cell_button(btn: &Button, cell: &Cell, owner: Option<Owner>) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-revealed-player");
    btn.remove_css_class("cell-revealed-ai");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{}", i)); }

    if cell.revealed {
        btn.add_css_class("cell-revealed");
        match owner {
            Some(Owner::Player) => btn.add_css_class("cell-revealed-player"),
            Some(Owner::Ai)     => btn.add_css_class("cell-revealed-ai"),
            None => {}
        }
        if cell.is_mine {
            btn.add_css_class("cell-mine");
            btn.set_label("\u{1F4A3}");
        } else if cell.adj > 0 {
            btn.add_css_class(&format!("cell-{}", cell.adj));
            btn.set_label(&cell.adj.to_string());
        } else {
            btn.set_label(" ");
        }
    } else {
        btn.set_label(" ");
    }
}

fn render_board(game: &PvAiGame, grid: &Grid) {
    for y in 0..game.height {
        for x in 0..game.width {
            if let Some(w) = grid.child_at(x as i32, y as i32) {
                if let Some(ov) = w.downcast_ref::<Overlay>() {
                    let cell = &game.grid[y][x];
                    let owner = game.ownership.get(&(x, y)).copied();
                    if let Some(btn) = ov.first_child().and_then(|w| w.downcast::<Button>().ok()) {
                        render_cell_button(&btn, cell, owner);
                    }
                    if let Some(da) = ov.first_child()
                        .and_then(|w| w.next_sibling())
                        .and_then(|w| w.downcast::<DrawingArea>().ok())
                    {
                        da.queue_draw();
                    }
                }
            }
        }
    }
}

// ── Hue-rotated flag (copied from rgb.rs) ────────────────────────────────────

fn draw_hue_flag(cr: &cairo::Context, pctx: &pango::Context, cx: f64, cy: f64, degrees: f64) {
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
            for row in 0..sz as usize {
                for col in 0..sz as usize {
                    let i = row * stride + col * 4;
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
    (h2rgb(p, q, h+1.0/3.0), h2rgb(p, q, h), h2rgb(p, q, h-1.0/3.0))
}

// ── AI turn scheduler ─────────────────────────────────────────────────────────

fn schedule_ai_turn(
    game: &Rc<RefCell<PvAiGame>>,
    grid: &Grid,
    ml:   &Rc<RefCell<Option<gtk4::Label>>>,
    fb:   &Rc<RefCell<Option<Button>>>,
    sr:   &Rc<RefCell<Option<glib::SourceId>>>,
) {
    game.borrow_mut().ai_pending = true;
    let (gc, gridc, mlc, fbc, src) = (game.clone(), grid.clone(), ml.clone(), fb.clone(), sr.clone());
    glib::timeout_add_local(Duration::from_millis(800), move || {
        gc.borrow_mut().ai_pending = false;
        if gc.borrow().state == PvAiState::Playing {
            gc.borrow_mut().ai_turn();
            render_board(&gc.borrow(), &gridc);
            update_score(&gc.borrow(), &mlc);
            update_face(&gc.borrow(), &fbc, &src);
        }
        glib::ControlFlow::Break
    });
}

// ── Cell factory ──────────────────────────────────────────────────────────────

fn make_cell(
    x: usize, y: usize,
    game: &Rc<RefCell<PvAiGame>>,
    grid: &Grid,
    ctx: &BoardContext,
) -> Overlay {
    let ov = Overlay::new();

    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click: reveal or chord
    {
        let (game_c, grid_c) = (game.clone(), grid.clone());
        let (ml, fb, st, sr, tl) = (
            ctx.mine_label.clone(), ctx.face_button.clone(),
            ctx.start_time.clone(), ctx.timer_source.clone(), ctx.timer_label.clone(),
        );
        btn.connect_clicked(move |_| {
            let (state, is_revealed, ai_pending) = {
                let g = game_c.borrow();
                (g.state, g.grid[y][x].revealed, g.ai_pending)
            };
            if ai_pending { return; }
            if state != PvAiState::Playing && state != PvAiState::Ready { return; }
            let was_ready = state == PvAiState::Ready;

            if was_ready {
                game_c.borrow_mut().player_first(x, y);
                if game_c.borrow().state == PvAiState::Playing {
                    start_timer(&st, &sr, &tl);
                }
            } else if is_revealed {
                game_c.borrow_mut().player_chord(x, y);
            } else {
                game_c.borrow_mut().player_reveal(x, y);
            }

            render_board(&game_c.borrow(), &grid_c);
            update_score(&game_c.borrow(), &ml);
            update_face(&game_c.borrow(), &fb, &sr);

            if game_c.borrow().state == PvAiState::Playing {
                schedule_ai_turn(&game_c, &grid_c, &ml, &fb, &sr);
            }
        });
    }

    // Right click: player flag → also triggers AI turn
    {
        let (game_c, grid_c) = (game.clone(), grid.clone());
        let (ml, fb, sr) = (ctx.mine_label.clone(), ctx.face_button.clone(), ctx.timer_source.clone());
        let right = GestureClick::new();
        right.set_button(3);
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if game_c.borrow().ai_pending { return; }
            if game_c.borrow().state != PvAiState::Playing { return; }
            game_c.borrow_mut().player_flag(x, y);
            render_board(&game_c.borrow(), &grid_c);
            update_score(&game_c.borrow(), &ml);
            if game_c.borrow().state == PvAiState::Playing {
                schedule_ai_turn(&game_c, &grid_c, &ml, &fb, &sr);
            }
        });
        btn.add_controller(right);
    }

    // Middle click: chord → also triggers AI turn
    {
        let (game_c, grid_c) = (game.clone(), grid.clone());
        let (ml, fb, sr) = (ctx.mine_label.clone(), ctx.face_button.clone(), ctx.timer_source.clone());
        let middle = GestureClick::new();
        middle.set_button(2);
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if game_c.borrow().ai_pending { return; }
            if game_c.borrow().state != PvAiState::Playing { return; }
            game_c.borrow_mut().player_chord(x, y);
            render_board(&game_c.borrow(), &grid_c);
            update_score(&game_c.borrow(), &ml);
            update_face(&game_c.borrow(), &fb, &sr);
            if game_c.borrow().state == PvAiState::Playing {
                schedule_ai_turn(&game_c, &grid_c, &ml, &fb, &sr);
            }
        });
        btn.add_controller(middle);
    }

    ov.set_child(Some(&btn));

    // DrawingArea: renders hue-rotated flags
    let da = DrawingArea::new();
    da.set_can_target(false);
    {
        let game_c = game.clone();
        da.set_draw_func(move |da, cr, w, h| {
            let g = game_c.borrow();
            let cell = &g.grid[y][x];
            if !cell.revealed {
                let cx = w as f64 / 2.0;
                let cy = h as f64 / 2.0;
                let pctx = da.pango_context();
                if cell.player_flag {
                    draw_hue_flag(cr, &pctx, cx, cy, 0.0);
                } else if g.ai_flags.contains(&(x, y)) {
                    draw_hue_flag(cr, &pctx, cx, cy, 240.0);
                }
            }
        });
    }
    ov.add_overlay(&da);
    ov.set_measure_overlay(&da, false);

    ov
}

// ── Board creation ────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let game = Rc::new(RefCell::new(PvAiGame::new(width, height, mine_count)));
    update_score(&game.borrow(), &ctx.mine_label);

    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    for y in 0..height {
        for x in 0..width {
            let ov = make_cell(x, y, &game, &grid, ctx);
            grid.attach(&ov, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

pub fn update_board(_game: &Rc<RefCell<crate::game::Game>>, _board: &gtk4::Widget) {}
