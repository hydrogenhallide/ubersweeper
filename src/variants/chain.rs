use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

// ---------------------------------------------------------------------------
// Reachability helpers
// ---------------------------------------------------------------------------

/// True if any unrevealed tile exists within the 5×5 centred on `last`.
fn any_in_5x5(game: &Game, last: (usize, usize)) -> bool {
    let (lx, ly) = (last.0 as isize, last.1 as isize);
    for y in 0..game.height {
        for x in 0..game.width {
            if (x as isize - lx).abs() <= 2
                && (y as isize - ly).abs() <= 2
                && game.grid[y][x].state == CellState::Hidden
            {
                return true;
            }
        }
    }
    false
}

/// True if (x,y) borders at least one revealed tile (8-directional frontier).
fn on_frontier(game: &Game, x: usize, y: usize) -> bool {
    for dy in -1_isize..=1 {
        for dx in -1_isize..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if nx >= 0 && nx < game.width as isize && ny >= 0 && ny < game.height as isize {
                if game.grid[ny as usize][nx as usize].state == CellState::Revealed {
                    return true;
                }
            }
        }
    }
    false
}

/// Compute reachability for a single cell.
/// `has_5x5` should be pre-computed with `any_in_5x5` to avoid O(n²) scanning.
fn is_reachable(
    game: &Game,
    x: usize,
    y: usize,
    last_pos: Option<(usize, usize)>,
    has_5x5: bool,
) -> bool {
    if game.state == GameState::Ready { return true; }
    if game.grid[y][x].state == CellState::Revealed { return false; }

    match last_pos {
        None => on_frontier(game, x, y),
        Some((lx, ly)) => {
            if has_5x5 {
                // Primary zone: 5×5 around last click
                (x as isize - lx as isize).abs() <= 2
                    && (y as isize - ly as isize).abs() <= 2
            } else {
                // 5×5 exhausted - fall back to frontier
                on_frontier(game, x, y)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Board creation
// ---------------------------------------------------------------------------

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    let (width, height) = {
        let g = ctx.game.borrow();
        (g.width, g.height)
    };

    let last_pos: Rc<RefCell<Option<(usize, usize)>>> = Rc::new(RefCell::new(None));

    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx, &last_pos);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    update_board_inner(game, board, None);
}

fn update_board_inner(
    game: &Rc<RefCell<Game>>,
    board: &gtk4::Widget,
    last_pos: Option<(usize, usize)>,
) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None => return,
    };
    let game_ref = game.borrow();
    let has_5x5 = last_pos.map_or(false, |lp| any_in_5x5(&game_ref, lp));
    for y in 0..game_ref.height {
        for x in 0..game_ref.width {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    render_cell(btn, &game_ref, x, y, last_pos, has_5x5);
                }
            }
        }
    }
}

fn render_cell(
    btn: &Button,
    game: &Game,
    x: usize,
    y: usize,
    last_pos: Option<(usize, usize)>,
    has_5x5: bool,
) {
    let cell = &game.grid[y][x];

    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-reachable");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{i}")); }

    match cell.state {
        CellState::Hidden => {
            btn.set_label(" ");
            if is_reachable(game, x, y, last_pos, has_5x5) {
                btn.add_css_class("cell-reachable");
            }
        }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3 | CellState::FlaggedNegative => {
            btn.set_label("\u{1F6A9}");
            if is_reachable(game, x, y, last_pos, has_5x5) {
                btn.add_css_class("cell-reachable");
            }
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else if cell.adjacent_mines > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adjacent_mines));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Button factory
// ---------------------------------------------------------------------------

fn make_cell_button(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    last_pos: &Rc<RefCell<Option<(usize, usize)>>>,
) -> Button {
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click - chord if revealed; reveal if reachable; ignore otherwise
    {
        let game_c    = ctx.game.clone();
        let board_c   = ctx.board_widget.clone();
        let mine_c    = ctx.mine_label.clone();
        let timer_c   = ctx.timer_label.clone();
        let face_c    = ctx.face_button.clone();
        let start_c   = ctx.start_time.clone();
        let source_c  = ctx.timer_source.clone();
        let last_pos_c = last_pos.clone();
        btn.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    *last_pos_c.borrow_mut() = Some((x, y));
                    let lp = *last_pos_c.borrow();
                    if let Some(b) = board_c.borrow().as_ref() {
                        update_board_inner(&game_c, b, lp);
                    }
                    update_mine_counter(&game_c, &mine_c);
                    update_face(&game_c, &face_c, &source_c);
                    return;
                }
                let lp = *last_pos_c.borrow();
                let has_5x5 = lp.map_or(false, |lp| any_in_5x5(&gr, lp));
                if !is_reachable(&gr, x, y, lp, has_5x5) { return; }
            }
            *last_pos_c.borrow_mut() = Some((x, y));
            let was_ready = game_c.borrow().state == GameState::Ready;
            game_c.borrow_mut().reveal(x, y);
            if was_ready && game_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }
            let lp = *last_pos_c.borrow();
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_inner(&game_c, b, lp);
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - flag, only if reachable
    {
        let right      = GestureClick::new();
        right.set_button(3);
        let game_c     = ctx.game.clone();
        let board_c    = ctx.board_widget.clone();
        let mine_c     = ctx.mine_label.clone();
        let last_pos_c = last_pos.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let lp = *last_pos_c.borrow();
            {
                let gr = game_c.borrow();
                let has_5x5 = lp.map_or(false, |lp| any_in_5x5(&gr, lp));
                if !is_reachable(&gr, x, y, lp, has_5x5) { return; }
            }
            game_c.borrow_mut().toggle_flag(x, y);
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_inner(&game_c, b, lp);
            }
            update_mine_counter(&game_c, &mine_c);
        });
        btn.add_controller(right);
    }

    // Middle click - chord (target is revealed, always reachable)
    {
        let middle     = GestureClick::new();
        middle.set_button(2);
        let game_c     = ctx.game.clone();
        let board_c    = ctx.board_widget.clone();
        let mine_c     = ctx.mine_label.clone();
        let face_c     = ctx.face_button.clone();
        let source_c   = ctx.timer_source.clone();
        let last_pos_c = last_pos.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !game_c.borrow_mut().chord(x, y) { return; }
            *last_pos_c.borrow_mut() = Some((x, y));
            let lp = *last_pos_c.borrow();
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_inner(&game_c, b, lp);
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        btn.add_controller(middle);
    }

    btn
}
