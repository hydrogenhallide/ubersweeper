use gtk4::prelude::*;
use gtk4::{Button, Grid};
use rand::Rng;
use rand::thread_rng;
use std::cell::RefCell;
use std::rc::Rc;

use crate::game::{CellState, Game};
use super::BoardContext;

#[derive(Clone, Copy)]
enum Dir { N, S, E, W }

impl Dir {
    fn random() -> Self {
        match thread_rng().gen_range(0..4) {
            0 => Self::N, 1 => Self::S, 2 => Self::E, _ => Self::W,
        }
    }
    fn arrow(self) -> &'static str {
        match self { Self::N => "↑", Self::S => "↓", Self::E => "→", Self::W => "←" }
    }
    /// Offset of the 3×3 centre from the cell's own position.
    fn center_offset(self) -> (isize, isize) {
        match self { Self::N => (0, -1), Self::S => (0, 1), Self::E => (1, 0), Self::W => (-1, 0) }
    }
}

/// Count mines in the 3×3 centred one tile in `dir` from (x, y).
fn count_offset(game: &Game, x: usize, y: usize, dir: Dir) -> u8 {
    let (odx, ody) = dir.center_offset();
    let cx = x as isize + odx;
    let cy = y as isize + ody;
    let mut n = 0u8;
    for dy in -1_isize..=1 {
        for dx in -1_isize..=1 {
            let nx = cx + dx;
            let ny = cy + dy;
            if nx >= 0 && nx < game.width as isize && ny >= 0 && ny < game.height as isize {
                n += game.grid[ny as usize][nx as usize].mines;
            }
        }
    }
    n
}

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height) = {
        let g = ctx.game.borrow();
        (g.width, g.height)
    };

    // Assign a random direction to every cell up front.
    let dirs: Rc<Vec<Vec<Dir>>> = Rc::new(
        (0..height).map(|_| (0..width).map(|_| Dir::random()).collect()).collect()
    );

    let dirs_c = dirs.clone();
    super::build_grid_board(ctx, move |game, board| {
        update_board_inner(game, board, &dirs_c);
    })
}

#[allow(dead_code)]
pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    // Directions aren't available here (they live in the closure), so this
    // no-ops; the variant always uses the captured closure from create_board.
    let _ = (game, board);
}

fn update_board_inner(
    game: &Rc<RefCell<Game>>,
    board: &gtk4::Widget,
    dirs: &Vec<Vec<Dir>>,
) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None => return,
    };
    let game_ref = game.borrow();
    for y in 0..game_ref.height {
        for x in 0..game_ref.width {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    render_cell(btn, &game_ref, x, y, dirs[y][x]);
                }
            }
        }
    }
}

fn render_cell(btn: &Button, game: &Game, x: usize, y: usize, dir: Dir) {
    let cell = &game.grid[y][x];

    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-zero");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{i}")); }

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
            let count = count_offset(game, x, y, dir);
            if count == 0 {
                btn.add_css_class("cell-zero");
            } else {
                btn.add_css_class(&format!("cell-{}", count.min(8)));
            }
            btn.set_label(&format!("{} {}", dir.arrow(), count));
        }
    }
}
