use gtk4::prelude::*;
use gtk4::{Button, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use rand::Rng;

use crate::game::{CellState, Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};
use crate::constants::CELL_SIZE;

const INFECT_CHANCE: f64 = 0.20;

// ---------------------------------------------------------------------------
// Infection logic: after revealing a numbered cell, 20% chance to spawn a
// new mine on a random hidden (non-flagged) neighbour.
// Newly created mines update the adjacent_mines counts of all their own
// neighbours (including already-revealed ones, so numbers visibly tick up).
// ---------------------------------------------------------------------------

fn neighbours(x: usize, y: usize, w: usize, h: usize) -> Vec<(usize, usize)> {
    let mut v = Vec::with_capacity(8);
    for dy in -1i32..=1 { for dx in -1i32..=1 {
        if dx == 0 && dy == 0 { continue; }
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
            v.push((nx as usize, ny as usize));
        }
    }}
    v
}

/// Try to infect once from the revealed cell at (x, y).
/// Returns true if a new mine was spawned.
fn try_infect(game: &mut Game, x: usize, y: usize) -> bool {
    if game.grid[y][x].adjacent_mines <= 0 { return false; }
    let mut rng = rand::thread_rng();
    if rng.gen::<f64>() >= INFECT_CHANCE { return false; }

    let w = game.width;
    let h = game.height;
    let candidates: Vec<(usize, usize)> = neighbours(x, y, w, h)
        .into_iter()
        .filter(|&(nx, ny)| game.grid[ny][nx].state == CellState::Hidden
                          && game.grid[ny][nx].mines == 0)
        .collect();

    if candidates.is_empty() { return false; }

    let (mx, my) = candidates[rng.gen_range(0..candidates.len())];

    // Spawn the mine.
    game.grid[my][mx].mines = 1;
    game.mine_count += 1;

    // Update adjacent_mines for every neighbour of the new mine.
    for (nx, ny) in neighbours(mx, my, w, h) {
        if game.grid[ny][nx].mines == 0 {
            game.grid[ny][nx].adjacent_mines += 1;
        }
    }

    true
}

/// Called AFTER game.reveal(x, y): pass the pre-reveal hidden snapshot.
fn infect_after_reveal(
    game: &mut Game,
    was_hidden: &[Vec<bool>],
) {
    let w = game.width;
    let h = game.height;

    let newly_revealed: Vec<(usize, usize)> = (0..h)
        .flat_map(|gy| (0..w).map(move |gx| (gx, gy)))
        .filter(|&(gx, gy)|
            was_hidden[gy][gx]
            && game.grid[gy][gx].state == CellState::Revealed
            && game.grid[gy][gx].adjacent_mines > 0
        )
        .collect();

    for (cx, cy) in newly_revealed {
        // Stop infecting once the game is over.
        if !matches!(game.state, GameState::Playing) { break; }
        try_infect(game, cx, cy);
    }
}

// ---------------------------------------------------------------------------
// Board
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

fn make_cell_button(x: usize, y: usize, ctx: &BoardContext) -> Button {
    let button = Button::with_label(" ");
    button.set_size_request(CELL_SIZE, CELL_SIZE);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_halign(gtk4::Align::Center);
    button.set_valign(gtk4::Align::Center);
    button.add_css_class("cell");

    // Left click - reveal with infection hook.
    {
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let timer_c  = ctx.timer_label.clone();
        let face_c   = ctx.face_button.clone();
        let start_c  = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();

        button.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if !matches!(gr.state, GameState::Ready | GameState::Playing) { return; }
                // Chord if already revealed.
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    // Snapshot, chord, infect.
                    let was_hidden: Vec<Vec<bool>> = {
                        let g = game_c.borrow();
                        (0..g.height).map(|gy|
                            (0..g.width).map(|gx| g.grid[gy][gx].state == CellState::Hidden).collect()
                        ).collect()
                    };
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    if matches!(game_c.borrow().state, GameState::Playing) {
                        infect_after_reveal(&mut game_c.borrow_mut(), &was_hidden);
                    }
                    if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
                    update_mine_counter(&game_c, &mine_c);
                    update_face(&game_c, &face_c, &source_c);
                    return;
                }
            }

            let was_ready = game_c.borrow().state == GameState::Ready;

            // Snapshot hidden cells before reveal.
            let was_hidden: Vec<Vec<bool>> = {
                let g = game_c.borrow();
                (0..g.height).map(|gy|
                    (0..g.width).map(|gx| g.grid[gy][gx].state == CellState::Hidden).collect()
                ).collect()
            };

            game_c.borrow_mut().reveal(x, y);

            if was_ready && game_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }

            // Apply infection only if game is still running.
            if matches!(game_c.borrow().state, GameState::Playing) {
                infect_after_reveal(&mut game_c.borrow_mut(), &was_hidden);
            }

            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - flag.
    {
        let right_click = gtk4::GestureClick::new();
        right_click.set_button(3);
        let game_c  = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c  = ctx.mine_label.clone();
        right_click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
            game_c.borrow_mut().toggle_flag(x, y);
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
        });
        button.add_controller(right_click);
    }

    // Middle click - chord (with infection).
    {
        let middle_click = gtk4::GestureClick::new();
        middle_click.set_button(2);
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        middle_click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Playing) { return; }
            let was_hidden: Vec<Vec<bool>> = {
                let g = game_c.borrow();
                (0..g.height).map(|gy|
                    (0..g.width).map(|gx| g.grid[gy][gx].state == CellState::Hidden).collect()
                ).collect()
            };
            if !game_c.borrow_mut().chord(x, y) { return; }
            if matches!(game_c.borrow().state, GameState::Playing) {
                infect_after_reveal(&mut game_c.borrow_mut(), &was_hidden);
            }
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        button.add_controller(middle_click);
    }

    button
}

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
                    render_cell(btn, &game_ref.grid[y][x]);
                }
            }
        }
    }
}

fn render_cell(btn: &Button, cell: &crate::game::Cell) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{}", i));
    }
    match cell.state {
        CellState::Hidden => { btn.set_label(" "); }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3
        | CellState::FlaggedNegative => { btn.set_label("\u{1F6A9}"); }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else if cell.adjacent_mines > 0 {
                btn.add_css_class(&format!("cell-{}",
                    cell.adjacent_mines.clamp(1, 8) as u8));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}
