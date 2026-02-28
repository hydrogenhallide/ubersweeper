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
    let w = game_ref.width;
    let h = game_ref.height;
    for y in 0..h {
        for x in 0..w {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    render_cell(btn, &game_ref.grid[y][x], x, y, w, h);
                }
            }
        }
    }
}

/// How many of the 8 surrounding slots actually exist on the grid.
fn neighbor_count(x: usize, y: usize, w: usize, h: usize) -> u8 {
    let mut n = 0u8;
    for dy in -1_isize..=1 {
        for dx in -1_isize..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if nx >= 0 && nx < w as isize && ny >= 0 && ny < h as isize {
                n += 1;
            }
        }
    }
    n
}

fn render_cell(btn: &Button, cell: &crate::game::Cell, x: usize, y: usize, w: usize, h: usize) {
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
            btn.set_label("\u{1F6A9}");
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else {
                // Show empty-neighbour count instead of mine count.
                let subtract = neighbor_count(x, y, w, h).saturating_sub(cell.adjacent_mines as u8);
                if subtract > 0 {
                    btn.add_css_class(&format!("cell-{}", subtract));
                    btn.set_label(&subtract.to_string());
                } else {
                    // subtract == 0 means every neighbour is a mine - blank like classic's 0.
                    btn.set_label(" ");
                }
            }
        }
    }
}
