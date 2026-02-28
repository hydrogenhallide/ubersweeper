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
        CellState::Hidden => {
            btn.set_label(" ");
        }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3 | CellState::FlaggedNegative => {
            btn.set_label("\u{1F6A9}"); // 🚩 (Flagged2/3 won't appear in Classic)
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}"); // 💣
            } else if cell.adjacent_mines > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adjacent_mines));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}
