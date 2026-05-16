use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Button, EventControllerScroll, GestureClick, Grid};

use crate::constants::CELL_SIZE;
use super::{BoardContext, start_timer, stop_timer};

const COLS: i32 = 18;
const ROWS: i32 = 14;
const CASCADE_BARRIER: usize = 8_000;

// ── Deterministic mine placement ───────────────────────────────────────────────

fn cell_hash(seed: u64, x: i64, y: i64) -> u64 {
    let mut h = seed
        .wrapping_add((x as u64).wrapping_mul(0x9e3779b97f4a7c15))
        .wrapping_add((y as u64).wrapping_mul(0x6c62272e07bb0142));
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94d049bb133111eb);
    h ^= h >> 31;
    h
}

// ── Cell state ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum IState { Hidden, Revealed, Flagged }

// ── Game ───────────────────────────────────────────────────────────────────────

struct InfGame {
    seed:          u64,
    mine_prob:     f64,
    states:        HashMap<(i64, i64), IState>,
    safe_set:      HashSet<(i64, i64)>,
    barrier_mines: HashSet<(i64, i64)>,
    score:         i64,
    started:       bool,
    game_over:     bool,
}

impl InfGame {
    fn new(safe_per_mine: u32) -> Self {
        use rand::Rng;
        InfGame {
            seed:          rand::thread_rng().gen::<u64>(),
            mine_prob:     1.0 / (safe_per_mine as f64 + 1.0),
            states:        HashMap::new(),
            safe_set:      HashSet::new(),
            barrier_mines: HashSet::new(),
            score:         0,
            started:       false,
            game_over:     false,
        }
    }

    fn is_mine(&self, x: i64, y: i64) -> bool {
        if self.safe_set.contains(&(x, y)) { return false; }
        if self.barrier_mines.contains(&(x, y)) { return true; }
        let h = cell_hash(self.seed, x, y);
        (h >> 11) as f64 * (1.0 / (1u64 << 53) as f64) < self.mine_prob
    }

    fn adj_mines(&self, x: i64, y: i64) -> u8 {
        let mut n = 0u8;
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                if dx == 0 && dy == 0 { continue; }
                if self.is_mine(x + dx, y + dy) { n += 1; }
            }
        }
        n
    }

    fn state_of(&self, x: i64, y: i64) -> IState {
        self.states.get(&(x, y)).copied().unwrap_or(IState::Hidden)
    }

    fn reveal(&mut self, x: i64, y: i64) {
        if self.game_over { return; }
        if !self.started {
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    self.safe_set.insert((x + dx, y + dy));
                }
            }
            self.started = true;
        }
        if self.is_mine(x, y) {
            self.states.insert((x, y), IState::Revealed);
            self.game_over = true;
            return;
        }
        let mut q: VecDeque<(i64, i64)> = VecDeque::new();
        q.push_back((x, y));
        let mut count = 0usize;
        while let Some((cx, cy)) = q.pop_front() {
            if self.state_of(cx, cy) != IState::Hidden { continue; }
            if count >= CASCADE_BARRIER {
                // Plant this cell and everything remaining in the queue as
                // barrier mines so the already-revealed frontier shows numbers
                // and the cascade looks like it terminated naturally.
                // Sparse barrier pattern: mine every 3rd cell by diagonal sum.
                // Every non-mine cell is guaranteed ≥1 mine neighbour, so the
                // cascade can't re-expand through the barrier zone.
                if (cx + cy).rem_euclid(3) == 0 { self.barrier_mines.insert((cx, cy)); }
                while let Some((bx, by)) = q.pop_front() {
                    if self.state_of(bx, by) == IState::Hidden
                        && (bx + by).rem_euclid(3) == 0
                    {
                        self.barrier_mines.insert((bx, by));
                    }
                }
                break;
            }
            self.states.insert((cx, cy), IState::Revealed);
            count += 1;
            if self.adj_mines(cx, cy) == 0 {
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        if dx == 0 && dy == 0 { continue; }
                        if self.state_of(cx + dx, cy + dy) == IState::Hidden {
                            q.push_back((cx + dx, cy + dy));
                        }
                    }
                }
            }
        }
    }

    fn toggle_flag(&mut self, x: i64, y: i64) {
        if self.game_over { return; }
        match self.state_of(x, y) {
            IState::Hidden  => {
                self.states.insert((x, y), IState::Flagged);
                self.score += if self.is_mine(x, y) { 1 } else { -1 };
            }
            IState::Flagged => {
                self.states.remove(&(x, y));
                self.score -= if self.is_mine(x, y) { 1 } else { -1 };
            }
            IState::Revealed => {}
        }
    }

    fn chord(&mut self, x: i64, y: i64) {
        if self.game_over { return; }
        if self.state_of(x, y) != IState::Revealed { return; }
        let adj = self.adj_mines(x, y);
        if adj == 0 { return; }
        let flags = (-1i64..=1).flat_map(|dy| (-1i64..=1).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| dx != 0 || dy != 0)
            .filter(|&(dx, dy)| self.state_of(x + dx, y + dy) == IState::Flagged)
            .count() as u8;
        if flags != adj { return; }
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                if dx == 0 && dy == 0 { continue; }
                if self.state_of(x + dx, y + dy) == IState::Hidden {
                    self.reveal(x + dx, y + dy);
                }
            }
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_cell(btn: &Button, game: &InfGame, x: i64, y: i64) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{}", i)); }

    match game.state_of(x, y) {
        IState::Hidden  => {
            if game.game_over && game.is_mine(x, y) {
                btn.add_css_class("cell-revealed");
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else {
                btn.set_label(" ");
            }
        }
        IState::Flagged => btn.set_label("\u{1F6A9}"),
        IState::Revealed => {
            btn.add_css_class("cell-revealed");
            if game.is_mine(x, y) {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else {
                let n = game.adj_mines(x, y);
                if n > 0 {
                    btn.add_css_class(&format!("cell-{}", n));
                    btn.set_label(&n.to_string());
                } else {
                    btn.set_label(" ");
                }
            }
        }
    }
}

fn refresh(grid: &Grid, game: &InfGame, offset: (i32, i32)) {
    for row in 0..ROWS {
        for col in 0..COLS {
            let wx = (offset.0 + col) as i64;
            let wy = (offset.1 + row) as i64;
            if let Some(w) = grid.child_at(col, row) {
                if let Some(btn) = w.downcast_ref::<Button>() {
                    render_cell(btn, game, wx, wy);
                }
            }
        }
    }
}

// ── UI helpers ────────────────────────────────────────────────────────────────

fn update_score(game: &InfGame, ml: &Rc<RefCell<Option<gtk4::Label>>>) {
    if let Some(lbl) = ml.borrow().as_ref() {
        lbl.set_text(&game.score.to_string());
    }
}

fn on_game_over(
    fb: &Rc<RefCell<Option<Button>>>,
    sr: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    if let Some(btn) = fb.borrow().as_ref() { btn.set_label("\u{1F635}"); }
    stop_timer(sr);
}

// ── Button factory ────────────────────────────────────────────────────────────

fn make_button(
    col: i32,
    row: i32,
    game: &Rc<RefCell<InfGame>>,
    offset: &Rc<RefCell<(i32, i32)>>,
    grid: &Grid,
    ctx: &BoardContext,
) -> Button {
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click: reveal or chord
    {
        let (game_c, off_c, grid_c) = (game.clone(), offset.clone(), grid.clone());
        let (ml, fb, st, sr, tl) = (
            ctx.mine_label.clone(), ctx.face_button.clone(),
            ctx.start_time.clone(), ctx.timer_source.clone(), ctx.timer_label.clone(),
        );
        btn.connect_clicked(move |_| {
            if game_c.borrow().game_over { return; }
            let (ox, oy) = *off_c.borrow();
            let (wx, wy) = ((ox + col) as i64, (oy + row) as i64);
            let was_started = game_c.borrow().started;
            {
                let mut g = game_c.borrow_mut();
                match g.state_of(wx, wy) {
                    IState::Revealed => g.chord(wx, wy),
                    IState::Hidden   => g.reveal(wx, wy),
                    _                => {}
                }
            }
            if !was_started && game_c.borrow().started {
                start_timer(&st, &sr, &tl);
            }
            refresh(&grid_c, &game_c.borrow(), *off_c.borrow());
            update_score(&game_c.borrow(), &ml);
            if game_c.borrow().game_over { on_game_over(&fb, &sr); }
        });
    }

    // Right click: flag
    {
        let (game_c, off_c, grid_c) = (game.clone(), offset.clone(), grid.clone());
        let (ml, fb, sr) = (ctx.mine_label.clone(), ctx.face_button.clone(), ctx.timer_source.clone());
        let right = GestureClick::new();
        right.set_button(3);
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if game_c.borrow().game_over { return; }
            let (ox, oy) = *off_c.borrow();
            let (wx, wy) = ((ox + col) as i64, (oy + row) as i64);
            game_c.borrow_mut().toggle_flag(wx, wy);
            refresh(&grid_c, &game_c.borrow(), *off_c.borrow());
            update_score(&game_c.borrow(), &ml);
            if game_c.borrow().game_over { on_game_over(&fb, &sr); }
        });
        btn.add_controller(right);
    }

    // Middle click: chord
    {
        let (game_c, off_c, grid_c) = (game.clone(), offset.clone(), grid.clone());
        let (ml, fb, sr) = (ctx.mine_label.clone(), ctx.face_button.clone(), ctx.timer_source.clone());
        let middle = GestureClick::new();
        middle.set_button(2);
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if game_c.borrow().game_over { return; }
            let (ox, oy) = *off_c.borrow();
            let (wx, wy) = ((ox + col) as i64, (oy + row) as i64);
            game_c.borrow_mut().chord(wx, wy);
            refresh(&grid_c, &game_c.borrow(), *off_c.borrow());
            update_score(&game_c.borrow(), &ml);
            if game_c.borrow().game_over { on_game_over(&fb, &sr); }
        });
        btn.add_controller(middle);
    }

    btn
}

// ── Board creation ────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let safe_per_mine: u32 = if height == 9997 {
        width as u32
    } else {
        let total = width * height;
        let safe  = total.saturating_sub(mine_count);
        if mine_count == 0 { 5 } else { (safe / mine_count).clamp(1, 20) as u32 }
    };

    let game   = Rc::new(RefCell::new(InfGame::new(safe_per_mine)));
    let offset: Rc<RefCell<(i32, i32)>> = Rc::new(RefCell::new((0, 0)));
    // Accumulated fractional scroll for smooth trackpad panning
    let scroll_accum: Rc<RefCell<(f64, f64)>> = Rc::new(RefCell::new((0.0, 0.0)));

    if let Some(lbl) = ctx.mine_label.borrow().as_ref() { lbl.set_text("0"); }

    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    let ml = ctx.mine_label.clone();

    for row in 0..ROWS {
        for col in 0..COLS {
            let btn = make_button(col, row, &game, &offset, &grid, ctx);
            grid.attach(&btn, col, row, 1, 1);
        }
    }

    // Scroll wheel / trackpad pans the viewport
    {
        let (game_c, off_c, grid_c, accum_c, ml_c) = (
            game.clone(), offset.clone(), grid.clone(), scroll_accum.clone(), ml.clone(),
        );
        let scroll = EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::BOTH_AXES
        );
        scroll.connect_scroll(move |ctrl, dx, dy| {
            let shift = ctrl.current_event_state()
                .contains(gtk4::gdk::ModifierType::SHIFT_MASK);
            // On most Linux compositors, shift+wheel still sends dy; treat it as dx.
            let (dx, dy) = if shift { (dy, dx) } else { (dx, dy) };
            let (tile_dx, tile_dy) = {
                let mut a = accum_c.borrow_mut();
                a.0 += dx;
                a.1 += dy;
                let tdx = a.0 as i32;
                let tdy = a.1 as i32;
                a.0 -= tdx as f64;
                a.1 -= tdy as f64;
                (tdx, tdy)
            };
            if tile_dx != 0 || tile_dy != 0 {
                {
                    let mut off = off_c.borrow_mut();
                    off.0 += tile_dx;
                    off.1 += tile_dy;
                }
                refresh(&grid_c, &game_c.borrow(), *off_c.borrow());
                update_score(&game_c.borrow(), &ml_c);
            }
            glib::Propagation::Stop
        });
        grid.add_controller(scroll);
    }

    refresh(&grid, &game.borrow(), (0, 0));
    let _ = ml;

    grid.upcast()
}

pub fn update_board(_game: &Rc<RefCell<crate::game::Game>>, board: &gtk4::Widget) {
    // Infinite board manages its own state; nothing to do on external update.
    let _ = board;
}
