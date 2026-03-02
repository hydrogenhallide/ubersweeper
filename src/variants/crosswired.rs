// Cross-wired: two boards side by side, each with their own mines.
//
// Board A's numbers count Board B's adjacent mines.
// Board B's numbers count Board A's adjacent mines.
//
// Each revealed cell tells you about the OTHER board, not its own. Neither
// board can be solved in isolation — you alternate: reveal A cells to constrain
// B's mines, reveal B cells to constrain A's mines, iterate. The two systems
// are mutually dependent; progress on one unlocks progress on the other.
//
// No cascade (a "0" on board A means no adjacent B-mines, not that A's
// neighbours are safe), no chord. Win by clearing all non-mine cells on both.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, GestureClick, Grid, Orientation, Separator};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::cell::RefCell;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum CwState { Hidden, Flagged, Revealed }

#[derive(Clone, Copy)]
struct CwCell {
    is_mine: bool,
    state:   CwState,
    adj:     u8,   // adjacent mines on the OTHER board
}

impl CwCell {
    fn new() -> Self { CwCell { is_mine: false, state: CwState::Hidden, adj: 0 } }
}

struct CwGame {
    a:             Vec<Vec<CwCell>>,
    b:             Vec<Vec<CwCell>>,
    width:         usize,
    height:        usize,
    mine_count:    usize,   // per board
    state:         GameState,
    flags_placed:  usize,
    safe_revealed: usize,
    total_safe:    usize,   // (width*height - mine_count) * 2
}

impl CwGame {
    fn new(width: usize, height: usize, mine_count: usize) -> Self {
        let mc = mine_count.min(width * height - 1);
        CwGame {
            a: vec![vec![CwCell::new(); width]; height],
            b: vec![vec![CwCell::new(); width]; height],
            width, height,
            mine_count: mc,
            state: GameState::Ready,
            flags_placed: 0,
            safe_revealed: 0,
            total_safe: (width * height - mc) * 2,
        }
    }

    fn place_mines(&mut self, first_x: usize, first_y: usize, first_board: usize) {
        let mut rng = thread_rng();
        for b in 0..2usize {
            let grid = if b == 0 { &mut self.a } else { &mut self.b };
            // Safety zone: 3×3 around first click on the clicked board;
            // just avoid the exact cell on the other board.
            let mut positions: Vec<(usize, usize)> = (0..self.height)
                .flat_map(|y| (0..self.width).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    if b == first_board {
                        (x as isize - first_x as isize).abs() > 1
                            || (y as isize - first_y as isize).abs() > 1
                    } else {
                        x != first_x || y != first_y
                    }
                })
                .collect();
            if positions.len() < self.mine_count {
                positions = (0..self.height)
                    .flat_map(|y| (0..self.width).map(move |x| (x, y)))
                    .filter(|&(x, y)| x != first_x || y != first_y)
                    .collect();
            }
            positions.shuffle(&mut rng);
            let n = self.mine_count.min(positions.len());
            for &(x, y) in &positions[..n] {
                grid[y][x].is_mine = true;
            }
        }
        self.compute_adj();
    }

    fn compute_adj(&mut self) {
        let w = self.width as isize;
        let h = self.height as isize;
        for y in 0..self.height {
            for x in 0..self.width {
                let mut from_a = 0u8;
                let mut from_b = 0u8;
                for dy in -1..=1isize {
                    for dx in -1..=1isize {
                        if dx == 0 && dy == 0 { continue; }
                        let nx = x as isize + dx;
                        let ny = y as isize + dy;
                        if nx >= 0 && nx < w && ny >= 0 && ny < h {
                            if self.a[ny as usize][nx as usize].is_mine { from_a += 1; }
                            if self.b[ny as usize][nx as usize].is_mine { from_b += 1; }
                        }
                    }
                }
                self.a[y][x].adj = from_b;  // A shows B's mines
                self.b[y][x].adj = from_a;  // B shows A's mines
            }
        }
    }

    fn reveal(&mut self, x: usize, y: usize, board: usize) -> bool {
        if self.state == GameState::Ready {
            self.place_mines(x, y, board);
            self.state = GameState::Playing;
        }
        if self.state != GameState::Playing { return false; }

        let cur_state = if board == 0 { self.a[y][x].state } else { self.b[y][x].state };
        if cur_state != CwState::Hidden { return false; }

        if board == 0 { self.a[y][x].state = CwState::Revealed; }
        else          { self.b[y][x].state = CwState::Revealed; }

        let is_mine = if board == 0 { self.a[y][x].is_mine } else { self.b[y][x].is_mine };
        if is_mine {
            self.state = GameState::Lost;
            for gy in 0..self.height {
                for gx in 0..self.width {
                    if self.a[gy][gx].is_mine { self.a[gy][gx].state = CwState::Revealed; }
                    if self.b[gy][gx].is_mine { self.b[gy][gx].state = CwState::Revealed; }
                }
            }
            return true;
        }

        self.safe_revealed += 1;
        if self.safe_revealed >= self.total_safe {
            self.state = GameState::Won;
        }
        true
    }

    fn toggle_flag(&mut self, x: usize, y: usize, board: usize) -> bool {
        if self.state != GameState::Playing { return false; }
        let cur = if board == 0 { self.a[y][x].state } else { self.b[y][x].state };
        match cur {
            CwState::Hidden => {
                if board == 0 { self.a[y][x].state = CwState::Flagged; }
                else          { self.b[y][x].state = CwState::Flagged; }
                self.flags_placed += 1;
                true
            }
            CwState::Flagged => {
                if board == 0 { self.a[y][x].state = CwState::Hidden; }
                else          { self.b[y][x].state = CwState::Hidden; }
                self.flags_placed -= 1;
                true
            }
            CwState::Revealed => false,
        }
    }

    fn sync_to(&self, game: &Rc<RefCell<Game>>) {
        let mut g = game.borrow_mut();
        g.state        = self.state;
        g.mine_count   = self.mine_count * 2;
        g.flags_placed = self.flags_placed;
    }
}

// ── Board creation ─────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let cw: Rc<RefCell<CwGame>> =
        Rc::new(RefCell::new(CwGame::new(width, height, mine_count)));

    // Show total mines (both boards) in the mine counter from the start.
    cw.borrow().sync_to(&ctx.game);
    update_mine_counter(&ctx.game, &ctx.mine_label);

    let outer = GtkBox::new(Orientation::Horizontal, 8);
    outer.set_halign(gtk4::Align::Center);

    let grid_a = make_grid();
    let grid_b = make_grid();

    for y in 0..height {
        for x in 0..width {
            grid_a.attach(&make_cell_button(x, y, 0, ctx, &cw), x as i32, y as i32, 1, 1);
            grid_b.attach(&make_cell_button(x, y, 1, ctx, &cw), x as i32, y as i32, 1, 1);
        }
    }

    let sep = Separator::new(Orientation::Vertical);
    sep.set_margin_start(4);
    sep.set_margin_end(4);

    outer.append(&grid_a);
    outer.append(&sep);
    outer.append(&grid_b);

    outer.upcast()
}

fn make_grid() -> Grid {
    let g = Grid::new();
    g.set_row_spacing(2);
    g.set_column_spacing(2);
    g.set_halign(gtk4::Align::Center);
    g.set_row_homogeneous(true);
    g.set_column_homogeneous(true);
    g
}

#[allow(dead_code)]
pub fn update_board(_game: &Rc<RefCell<Game>>, _board: &gtk4::Widget) {}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_board(cw: &CwGame, board: &gtk4::Widget) {
    let outer = match board.downcast_ref::<GtkBox>() {
        Some(b) => b,
        None => return,
    };
    // Structure: grid_a | separator | grid_b — first_child = grid_a, last_child = grid_b.
    let grid_a = match outer.first_child().and_then(|w| w.downcast::<Grid>().ok()) {
        Some(g) => g,
        None => return,
    };
    let grid_b = match outer.last_child().and_then(|w| w.downcast::<Grid>().ok()) {
        Some(g) => g,
        None => return,
    };
    for y in 0..cw.height {
        for x in 0..cw.width {
            if let Some(w) = grid_a.child_at(x as i32, y as i32) {
                if let Some(btn) = w.downcast_ref::<Button>() {
                    render_cell(btn, &cw.a[y][x]);
                }
            }
            if let Some(w) = grid_b.child_at(x as i32, y as i32) {
                if let Some(btn) = w.downcast_ref::<Button>() {
                    render_cell(btn, &cw.b[y][x]);
                }
            }
        }
    }
}

fn render_cell(btn: &Button, cell: &CwCell) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{}", i));
    }
    match cell.state {
        CwState::Hidden   => { btn.set_label(" "); }
        CwState::Flagged  => { btn.set_label("\u{1F6A9}"); }
        CwState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.is_mine {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else if cell.adj > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adj));
                btn.set_label(&cell.adj.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}

// ── Button factory ─────────────────────────────────────────────────────────────

fn make_cell_button(
    x: usize,
    y: usize,
    board: usize,   // 0 = board A, 1 = board B
    ctx: &BoardContext,
    cw: &Rc<RefCell<CwGame>>,
) -> Button {
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click – reveal.
    {
        let cw_c     = cw.clone();
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let timer_c  = ctx.timer_label.clone();
        let face_c   = ctx.face_button.clone();
        let start_c  = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();
        btn.connect_clicked(move |_| {
            {
                let s = cw_c.borrow();
                if !matches!(s.state, GameState::Ready | GameState::Playing) { return; }
                let st = if board == 0 { s.a[y][x].state } else { s.b[y][x].state };
                if st != CwState::Hidden { return; }
            }
            let was_ready = cw_c.borrow().state == GameState::Ready;
            cw_c.borrow_mut().reveal(x, y, board);
            cw_c.borrow().sync_to(&game_c);
            if was_ready && cw_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }
            if let Some(b) = board_c.borrow().as_ref() {
                render_board(&cw_c.borrow(), b);
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click – flag.
    {
        let right    = GestureClick::new();
        right.set_button(3);
        let cw_c     = cw.clone();
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !cw_c.borrow_mut().toggle_flag(x, y, board) { return; }
            cw_c.borrow().sync_to(&game_c);
            if let Some(b) = board_c.borrow().as_ref() {
                render_board(&cw_c.borrow(), b);
            }
            update_mine_counter(&game_c, &mine_c);
        });
        btn.add_controller(right);
    }

    btn
}
