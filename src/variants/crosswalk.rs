use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use rand::Rng;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

// ---------------------------------------------------------------------------
// Shift logic
//
// On every move, two distinct stripes of the same axis (randomly rows or
// columns) shift in opposite directions with wraparound.  Only mine presence
// moves - cell states (Hidden/Revealed/Flagged) stay at their positions.
//
// Adjacency is recomputed with standard non-wrapping 8-connectivity, so a
// mine that wrapped around the edge is invisible to cells on the other side.
// ---------------------------------------------------------------------------

fn shift_row_left(game: &mut Game, row: usize) {
    let w = game.width;
    let saved = game.grid[row][0];
    for x in 0..w - 1 {
        game.grid[row][x] = game.grid[row][x + 1];
    }
    game.grid[row][w - 1] = saved;
}

fn shift_row_right(game: &mut Game, row: usize) {
    let w = game.width;
    let saved = game.grid[row][w - 1];
    for x in (1..w).rev() {
        game.grid[row][x] = game.grid[row][x - 1];
    }
    game.grid[row][0] = saved;
}

fn shift_col_up(game: &mut Game, col: usize) {
    let h = game.height;
    let saved = game.grid[0][col];
    for y in 0..h - 1 {
        game.grid[y][col] = game.grid[y + 1][col];
    }
    game.grid[h - 1][col] = saved;
}

fn shift_col_down(game: &mut Game, col: usize) {
    let h = game.height;
    let saved = game.grid[h - 1][col];
    for y in (1..h).rev() {
        game.grid[y][col] = game.grid[y - 1][col];
    }
    game.grid[0][col] = saved;
}

fn apply_shift(game: &mut Game) {
    let mut rng = rand::thread_rng();

    // One stripe, one direction per move.
    if rng.gen_bool(0.5) {
        let row = rng.gen_range(0..game.height);
        if rng.gen_bool(0.5) { shift_row_left(game, row); }
        else                  { shift_row_right(game, row); }
    } else {
        let col = rng.gen_range(0..game.width);
        if rng.gen_bool(0.5) { shift_col_up(game, col); }
        else                  { shift_col_down(game, col); }
    }

    // Recalculate all adjacency counts with non-wrapping rules.
    game.recompute_all_adjacency();

    // Re-cascade from any revealed blank cell that now has hidden safe neighbours
    // (can happen when mines shift away from a formerly-numbered tile).
    post_shift_cascade(game);
}

/// Cascade-reveal hidden mine-free neighbours of every revealed blank cell.
/// Mirrors game::cascade_reveal but operates on the post-shift board.
fn post_shift_cascade(game: &mut Game) {
    // Collect seeds: all revealed blank cells.
    let seeds: Vec<(usize, usize)> = (0..game.height)
        .flat_map(|y| (0..game.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let c = &game.grid[y][x];
            c.state == crate::game::CellState::Revealed
                && c.mines == 0
                && c.adjacent_mines == 0
        })
        .collect();

    for (x, y) in seeds {
        cascade_from(game, x, y);
    }
}

fn cascade_from(game: &mut Game, x: usize, y: usize) {
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || nx >= game.width as i32 || ny < 0 || ny >= game.height as i32 { continue; }
            let (nx, ny) = (nx as usize, ny as usize);
            if game.grid[ny][nx].state == crate::game::CellState::Hidden
                && game.grid[ny][nx].mines == 0
            {
                game.grid[ny][nx].state = crate::game::CellState::Revealed;
                if game.grid[ny][nx].adjacent_mines == 0 {
                    cascade_from(game, nx, ny);
                }
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

    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    super::classic::update_board(game, board);
}

// ---------------------------------------------------------------------------
// Post-action: update UI, then shift if the game is still live.
// ---------------------------------------------------------------------------

fn post_action(
    game:         &Rc<RefCell<Game>>,
    board:        &Rc<RefCell<Option<gtk4::Widget>>>,
    mine_label:   &Rc<RefCell<Option<gtk4::Label>>>,
    face_button:  &Rc<RefCell<Option<Button>>>,
    timer_source: &Rc<RefCell<Option<gtk4::glib::SourceId>>>,
) {
    if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
    update_mine_counter(game, mine_label);
    update_face(game, face_button, timer_source);

    if !matches!(game.borrow().state, GameState::Playing) { return; }

    apply_shift(&mut game.borrow_mut());

    if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
    update_mine_counter(game, mine_label);
}

// ---------------------------------------------------------------------------
// Button factory
// ---------------------------------------------------------------------------

fn make_cell_button(x: usize, y: usize, ctx: &BoardContext) -> Button {
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click - reveal or chord
    {
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let timer_c  = ctx.timer_label.clone();
        let face_c   = ctx.face_button.clone();
        let start_c  = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();
        btn.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if !matches!(gr.state, GameState::Ready | GameState::Playing) { return; }
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    post_action(&game_c, &board_c, &mine_c, &face_c, &source_c);
                    return;
                }
            }
            let was_ready = game_c.borrow().state == GameState::Ready;
            game_c.borrow_mut().reveal(x, y);
            if was_ready && game_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }
            post_action(&game_c, &board_c, &mine_c, &face_c, &source_c);
        });
    }

    // Right click - flag
    {
        let right    = GestureClick::new();
        right.set_button(3);
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !game_c.borrow_mut().toggle_flag(x, y) { return; }
            post_action(&game_c, &board_c, &mine_c, &face_c, &source_c);
        });
        btn.add_controller(right);
    }

    // Middle click - chord
    {
        let middle   = GestureClick::new();
        middle.set_button(2);
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !game_c.borrow_mut().chord(x, y) { return; }
            post_action(&game_c, &board_c, &mine_c, &face_c, &source_c);
        });
        btn.add_controller(middle);
    }

    btn
}
