use gtk4::prelude::*;
use gtk4::{Button, Grid};
use rand::Rng;
use std::cell::RefCell;
use std::rc::Rc;

use crate::game::{CellState, Game};

use super::BoardContext;

/// Returns true ~50% of the time - decides if a placed mine is negative.
pub fn roll_negative() -> bool {
    rand::thread_rng().gen_bool(0.5)
}

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    super::build_grid_board(ctx, update_board)
}

pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None => return,
    };
    let game_ref = game.borrow();
    let w = game_ref.width;
    let h = game_ref.height;
    for y in 0..h {
        for x in 0..w {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    // True if any neighbour has a mine (positive or negative).
                    let has_adjacent = (-1_isize..=1).flat_map(|dy| (-1_isize..=1).map(move |dx| (dx, dy)))
                        .filter(|&(dx, dy)| dx != 0 || dy != 0)
                        .any(|(dx, dy)| {
                            let nx = x as isize + dx;
                            let ny = y as isize + dy;
                            nx >= 0 && nx < w as isize && ny >= 0 && ny < h as isize
                                && game_ref.grid[ny as usize][nx as usize].mines > 0
                        });
                    render_cell(btn, &game_ref.grid[y][x], has_adjacent);
                }
            }
        }
    }
}

fn render_cell(btn: &Button, cell: &crate::game::Cell, has_adjacent: bool) {
    // Clear all state-driven classes
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-negative-mine");
    btn.remove_css_class("cell-flagged-negative");
    btn.remove_css_class("cell-zero");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{}", i));
        btn.remove_css_class(&format!("cell-neg-{}", i));
    }

    match cell.state {
        CellState::Hidden => {
            btn.set_label(" ");
        }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3 => {
            btn.set_label("\u{1F6A9}"); // 🚩 (Flagged2/3 won't appear in NegativeMines)
        }
        CellState::FlaggedNegative => {
            // Same 🚩 emoji, but CSS rotates it upside-down via .cell-flagged-negative
            btn.add_css_class("cell-flagged-negative");
            btn.set_label("\u{1F6A9}");
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                if cell.is_negative {
                    // CSS flips the 💣 upside-down and inverts the background
                    btn.add_css_class("cell-negative-mine");
                } else {
                    btn.add_css_class("cell-mine");
                }
                btn.set_label("\u{1F4A3}"); // 💣
            } else if cell.adjacent_mines > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adjacent_mines));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else if cell.adjacent_mines < 0 {
                btn.add_css_class(&format!("cell-neg-{}", cell.adjacent_mines.abs()));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else if has_adjacent {
                // Net zero but borders mines that cancelled out - faint "0".
                btn.add_css_class("cell-zero");
                btn.set_label("0");
            } else {
                btn.set_label(" ");
            }
        }
    }
}
