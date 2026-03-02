use gtk4::prelude::*;
use gtk4::{Button, Grid};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use rand::Rng;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

// ---------------------------------------------------------------------------
// Kudzu state
// ---------------------------------------------------------------------------

struct KudzuState {
    kudzu: Vec<Vec<bool>>,
    width: usize,
    height: usize,
}

impl KudzuState {
    fn new(width: usize, height: usize) -> Self {
        KudzuState {
            kudzu: vec![vec![false; width]; height],
            width,
            height,
        }
    }

    fn init_random(&mut self) {
        let mut rng = rand::thread_rng();
        let x = rng.gen_range(0..self.width);
        let y = rng.gen_range(0..self.height);
        self.kudzu[y][x] = true;
    }

    /// Each existing kudzu tile independently spreads to one random adjacent
    /// hidden non-kudzu cell (8 directions). If a tile is caged by revealed
    /// cells with no adjacent hidden targets, it BFS-escapes to the nearest
    /// hidden tile on the board instead.
    fn spread(&mut self, game: &Game) {
        let mut rng = rand::thread_rng();
        let mut to_add: Vec<(usize, usize)> = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if self.kudzu[y][x] {
                    let candidates: Vec<(usize, usize)> =
                        all_neighbors(x, y, self.width, self.height)
                            .into_iter()
                            .filter(|&(nx, ny)| {
                                !self.kudzu[ny][nx]
                                    && game.grid[ny][nx].state == CellState::Hidden
                            })
                            .collect();
                    if !candidates.is_empty() {
                        to_add.push(candidates[rng.gen_range(0..candidates.len())]);
                    } else if let Some(target) = self.nearest_hidden(x, y, game) {
                        to_add.push(target);
                    }
                }
            }
        }
        for (nx, ny) in to_add {
            self.kudzu[ny][nx] = true;
        }
    }

    /// BFS from (sx, sy) through any cell to find the nearest hidden non-kudzu
    /// tile. Used when a vine is caged by revealed cells.
    fn nearest_hidden(&self, sx: usize, sy: usize, game: &Game) -> Option<(usize, usize)> {
        let mut visited = vec![vec![false; self.width]; self.height];
        let mut queue = VecDeque::new();
        visited[sy][sx] = true;
        queue.push_back((sx, sy));
        while let Some((x, y)) = queue.pop_front() {
            for (nx, ny) in all_neighbors(x, y, self.width, self.height) {
                if visited[ny][nx] { continue; }
                visited[ny][nx] = true;
                if !self.kudzu[ny][nx] && game.grid[ny][nx].state == CellState::Hidden {
                    return Some((nx, ny));
                }
                queue.push_back((nx, ny));
            }
        }
        None
    }

    fn is_empty(&self) -> bool {
        self.kudzu.iter().all(|row| row.iter().all(|&k| !k))
    }

    /// Set kudzu-covered Hidden cells to Flagged so cascade skips them.
    /// Returns the cells changed so they can be restored afterward.
    fn mask_for_cascade(&self, game: &mut Game) -> Vec<(usize, usize)> {
        let mut masked = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if self.kudzu[y][x] && game.grid[y][x].state == CellState::Hidden {
                    game.grid[y][x].state = CellState::Flagged;
                    masked.push((x, y));
                }
            }
        }
        masked
    }

    /// Restore cells masked by mask_for_cascade back to Hidden.
    fn unmask_cascade(masked: Vec<(usize, usize)>, game: &mut Game) {
        for (x, y) in masked {
            // Only restore if still Flagged; a mine explosion may have set it to Revealed.
            if game.grid[y][x].state == CellState::Flagged {
                game.grid[y][x].state = CellState::Hidden;
            }
        }
    }

    /// When a player uncovers cell (x, y): if that cell or any orthogonal
    /// neighbour has kudzu, remove kudzu from (x, y) and its 4 neighbours.
    fn remove_around_reveal(&mut self, x: usize, y: usize) {
        let neighbors = ortho_neighbors(x, y, self.width, self.height);
        let condition = self.kudzu[y][x]
            || neighbors.iter().any(|&(nx, ny)| self.kudzu[ny][nx]);
        if condition {
            self.kudzu[y][x] = false;
            for (nx, ny) in neighbors {
                self.kudzu[ny][nx] = false;
            }
        }
    }

    /// Clear kudzu from any cell that has been revealed (cascade clean-up).
    fn clean_revealed(&mut self, game: &Game) {
        for y in 0..self.height {
            for x in 0..self.width {
                if self.kudzu[y][x] && game.grid[y][x].state == CellState::Revealed {
                    self.kudzu[y][x] = false;
                }
            }
        }
    }
}

/// 4-directional — used for kudzu removal (above/below/sideways per spec).
fn ortho_neighbors(x: usize, y: usize, w: usize, h: usize) -> Vec<(usize, usize)> {
    let mut v = Vec::with_capacity(4);
    if y > 0     { v.push((x, y - 1)); }
    if y + 1 < h { v.push((x, y + 1)); }
    if x > 0     { v.push((x - 1, y)); }
    if x + 1 < w { v.push((x + 1, y)); }
    v
}

/// 8-directional — used for spreading.
fn all_neighbors(x: usize, y: usize, w: usize, h: usize) -> Vec<(usize, usize)> {
    let mut v = Vec::with_capacity(8);
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                v.push((nx as usize, ny as usize));
            }
        }
    }
    v
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height) = {
        let g = ctx.game.borrow();
        (g.width, g.height)
    };

    let kudzu = Rc::new(RefCell::new(KudzuState::new(width, height)));
    kudzu.borrow_mut().init_random();

    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx, kudzu.clone());
            // Paint initial kudzu before the first click.
            if kudzu.borrow().kudzu[y][x] {
                btn.add_css_class("cell-kudzu");
                btn.set_label("\u{1F331}");
            }
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

fn make_cell_button(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    kudzu: Rc<RefCell<KudzuState>>,
) -> Button {
    let button = Button::with_label(" ");
    button.set_size_request(CELL_SIZE, CELL_SIZE);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_halign(gtk4::Align::Center);
    button.set_valign(gtk4::Align::Center);
    button.add_css_class("cell");

    // Left click - reveal with kudzu interaction.
    {
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let timer_c  = ctx.timer_label.clone();
        let face_c   = ctx.face_button.clone();
        let start_c  = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();
        let kudzu_c  = kudzu.clone();

        button.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if !matches!(gr.state, GameState::Ready | GameState::Playing) { return; }
                // Chord on already-revealed cell — spread kudzu, no removal bonus.
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    let masked = {
                        let k = kudzu_c.borrow();
                        let mut g = game_c.borrow_mut();
                        k.mask_for_cascade(&mut g)
                    };
                    let chorded = game_c.borrow_mut().chord(x, y);
                    KudzuState::unmask_cascade(masked, &mut game_c.borrow_mut());
                    if !chorded { return; }
                    if matches!(game_c.borrow().state, GameState::Playing) {
                        let g = game_c.borrow();
                        let mut k = kudzu_c.borrow_mut();
                        k.clean_revealed(&g);
                        k.spread(&g);
                    }
                    kudzu_liveness_check(&game_c, &kudzu_c);
                    if let Some(b) = board_c.borrow().as_ref() {
                        update_board_kudzu(&game_c, b, &kudzu_c);
                    }
                    update_mine_counter(&game_c, &mine_c);
                    update_face(&game_c, &face_c, &source_c);
                    return;
                }
            }

            let was_ready = game_c.borrow().state == GameState::Ready;

            // Kudzu removal fires before the cell is revealed.
            kudzu_c.borrow_mut().remove_around_reveal(x, y);

            // Mask remaining kudzu cells so cascade can't peel them.
            let masked = {
                let k = kudzu_c.borrow();
                let mut g = game_c.borrow_mut();
                k.mask_for_cascade(&mut g)
            };
            game_c.borrow_mut().reveal(x, y);
            KudzuState::unmask_cascade(masked, &mut game_c.borrow_mut());

            if was_ready && game_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }

            // Spread only while the game is ongoing.
            if matches!(game_c.borrow().state, GameState::Playing) {
                let g = game_c.borrow();
                let mut k = kudzu_c.borrow_mut();
                k.clean_revealed(&g);
                k.spread(&g);
            }

            kudzu_liveness_check(&game_c, &kudzu_c);

            if let Some(b) = board_c.borrow().as_ref() {
                update_board_kudzu(&game_c, b, &kudzu_c);
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - flag only, no kudzu spread.
    {
        let right_click = gtk4::GestureClick::new();
        right_click.set_button(3);
        let game_c  = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c  = ctx.mine_label.clone();
        let kudzu_c = kudzu.clone();
        right_click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
            game_c.borrow_mut().toggle_flag(x, y);
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_kudzu(&game_c, b, &kudzu_c);
            }
            update_mine_counter(&game_c, &mine_c);
        });
        button.add_controller(right_click);
    }

    // Middle click - chord, spread kudzu.
    {
        let middle_click = gtk4::GestureClick::new();
        middle_click.set_button(2);
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        let kudzu_c  = kudzu.clone();
        middle_click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Playing) { return; }
            let masked = {
                let k = kudzu_c.borrow();
                let mut g = game_c.borrow_mut();
                k.mask_for_cascade(&mut g)
            };
            let chorded = game_c.borrow_mut().chord(x, y);
            KudzuState::unmask_cascade(masked, &mut game_c.borrow_mut());
            if !chorded { return; }
            if matches!(game_c.borrow().state, GameState::Playing) {
                let g = game_c.borrow();
                let mut k = kudzu_c.borrow_mut();
                k.clean_revealed(&g);
                k.spread(&g);
            }
            kudzu_liveness_check(&game_c, &kudzu_c);
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_kudzu(&game_c, b, &kudzu_c);
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        button.add_controller(middle_click);
    }

    button
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// If the game is Playing or just Won but all kudzu has been wiped out, it's
/// a loss — the player must keep at least one vine alive to the end.
fn kudzu_liveness_check(game: &Rc<RefCell<Game>>, kudzu: &Rc<RefCell<KudzuState>>) {
    let state = game.borrow().state;
    if matches!(state, GameState::Playing | GameState::Won) && kudzu.borrow().is_empty() {
        game.borrow_mut().state = GameState::Lost;
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn update_board_kudzu(
    game: &Rc<RefCell<Game>>,
    board: &gtk4::Widget,
    kudzu: &Rc<RefCell<KudzuState>>,
) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None => return,
    };
    let game_ref = game.borrow();
    let kudzu_ref = kudzu.borrow();
    for y in 0..game_ref.height {
        for x in 0..game_ref.width {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    render_cell(btn, x, y, &game_ref, Some(&kudzu_ref));
                }
            }
        }
    }
}

/// External update_board called by the variant dispatcher (no kudzu state available).
pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None => return,
    };
    let game_ref = game.borrow();
    for y in 0..game_ref.height {
        for x in 0..game_ref.width {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    render_cell(btn, x, y, &game_ref, None);
                }
            }
        }
    }
}

fn render_cell(btn: &Button, x: usize, y: usize, game: &Game, kudzu: Option<&KudzuState>) {
    let cell = &game.grid[y][x];
    let has_kudzu = kudzu.map_or(false, |k| k.kudzu[y][x]);

    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-kudzu");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{}", i));
    }

    match cell.state {
        CellState::Hidden => {
            if has_kudzu {
                btn.add_css_class("cell-kudzu");
                btn.set_label("\u{1F331}"); // 🌱
            } else {
                btn.set_label(" ");
            }
        }
        // Flag emoji always wins over kudzu so the player's information stays visible.
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3
        | CellState::FlaggedNegative => {
            btn.set_label("\u{1F6A9}"); // 🚩
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}"); // 💣
            } else {
                // Subtract any mines currently hidden under kudzu from the display.
                let kudzu_mine_count: i8 = kudzu.map_or(0, |k| {
                    all_neighbors(x, y, game.width, game.height)
                        .iter()
                        .filter(|&&(nx, ny)| k.kudzu[ny][nx] && game.grid[ny][nx].mines > 0)
                        .count() as i8
                });
                let displayed = cell.adjacent_mines - kudzu_mine_count;
                if displayed > 0 {
                    btn.add_css_class(&format!("cell-{}", displayed.clamp(1, 8) as u8));
                    btn.set_label(&displayed.to_string());
                } else {
                    btn.set_label(" ");
                }
            }
        }
    }
}
