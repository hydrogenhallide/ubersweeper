use gtk4::prelude::*;
use gtk4::{Button, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use crate::game::{CellState, Game};
use super::BoardContext;

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    super::build_grid_board(ctx, update_board)
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
                    let above_adj = if y == 0 {
                        None
                    } else {
                        Some(game_ref.grid[y - 1][x].adjacent_mines)
                    };
                    render_cell(btn, &game_ref.grid[y][x], above_adj);
                }
            }
        }
    }
}

fn render_cell(btn: &Button, cell: &crate::game::Cell, above_adj: Option<i8>) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-zero");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{i}"));
        btn.remove_css_class(&format!("cell-neg-{i}"));
    }

    match cell.state {
        CellState::Hidden => {
            btn.set_label(" ");
        }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3 | CellState::FlaggedNegative => {
            btn.set_label("\u{1F6A9}");
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
                return;
            }
            let display = match above_adj {
                None       => cell.adjacent_mines,           // top row: absolute
                Some(prev) => cell.adjacent_mines - prev,    // rest: delta from above
            };
            if display > 0 {
                btn.add_css_class(&format!("cell-{display}"));
                btn.set_label(&display.to_string());
            } else if display < 0 {
                btn.add_css_class(&format!("cell-neg-{}", display.unsigned_abs()));
                btn.set_label(&display.to_string());
            } else if cell.adjacent_mines != 0 {
                // Borders mines but the delta cancels out - faint "0".
                btn.add_css_class("cell-zero");
                btn.set_label("0");
            } else {
                btn.set_label(" ");
            }
        }
    }
}
