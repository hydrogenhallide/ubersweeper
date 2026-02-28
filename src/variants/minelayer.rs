use gtk4::prelude::*;
use gtk4::{Button, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use crate::game::Game;
use crate::constants::CELL_SIZE;
use super::{BoardContext, start_timer, stop_timer};

// ---------------------------------------------------------------------------
// Minelayer state
// ---------------------------------------------------------------------------

struct Ml {
    width: usize,
    height: usize,
    /// Starting adjacent-mine count per cell.
    /// Mine-position cells start at 0; non-mine cells carry their real count.
    base: Vec<Vec<i32>>,
    /// Player-placed mine markers.
    placed: Vec<Vec<bool>>,
    total_mines: usize,
    placed_count: usize,
    won: bool,
}

impl Ml {
    fn from_game(game: &Game) -> Self {
        let (w, h) = (game.width, game.height);
        // Compute neighbor-mine count for ALL cells, including mine-position cells.
        // This way mine cells show a number (their mine-neighbor count) instead of
        // a blank that screams "dig here".
        // Use closed-neighbourhood counting (cell + its 8 neighbours).
        // This means a mine cell counts itself, so it always shows ≥1 instead
        // of appearing as a blank "dig here" hint.
        let base = (0..h)
            .map(|y| (0..w)
                .map(|x| {
                    let mut n = 0i32;
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            // No skip for (0,0) - include the cell itself.
                            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                            if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                                n += game.grid[ny as usize][nx as usize].mines as i32;
                            }
                        }
                    }
                    n
                })
                .collect())
            .collect();
        Ml {
            width: w,
            height: h,
            base,
            placed: vec![vec![false; w]; h],
            total_mines: game.mine_count,
            placed_count: 0,
            won: false,
        }
    }

    /// How much this cell still needs to be cancelled by neighboring mines.
    fn display_count(&self, x: usize, y: usize) -> i32 {
        let mut n = self.base[y][x];
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 { continue; }
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx >= 0 && nx < self.width as i32 && ny >= 0 && ny < self.height as i32 {
                    if self.placed[ny as usize][nx as usize] { n -= 1; }
                }
            }
        }
        n
    }

    fn toggle(&mut self, x: usize, y: usize) {
        if self.won { return; }
        if self.placed[y][x] {
            self.placed[y][x] = false;
            self.placed_count -= 1;
        } else {
            self.placed[y][x] = true;
            self.placed_count += 1;
        }
        if self.placed_count == self.total_mines {
            self.won = (0..self.height).all(|y| (0..self.width).all(|x|
                self.placed[y][x] || self.display_count(x, y) == 0
            ));
        }
    }

    fn mines_remaining(&self) -> i32 {
        self.total_mines as i32 - self.placed_count as i32
    }
}

// ---------------------------------------------------------------------------
// Board creation
// ---------------------------------------------------------------------------

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    // Eagerly place mines and compute adjacency with no safe zone.
    ctx.game.borrow_mut().generate();

    let ml: Rc<RefCell<Ml>> = Rc::new(RefCell::new(Ml::from_game(&ctx.game.borrow())));
    let started: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let (w, h) = { let g = ctx.game.borrow(); (g.width, g.height) };

    // Initialise mine counter to total mines (none placed yet).
    if let Some(lbl) = ctx.mine_label.borrow().as_ref() {
        lbl.set_text(&format!("{:03}", ml.borrow().total_mines));
    }

    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    for y in 0..h {
        for x in 0..w {
            let btn = make_cell_button(x, y, ctx, ml.clone(), started.clone());
            render_cell(&btn, false, ml.borrow().display_count(x, y));
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

fn make_cell_button(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    ml: Rc<RefCell<Ml>>,
    started: Rc<RefCell<bool>>,
) -> Button {
    let button = Button::with_label(" ");
    button.set_size_request(CELL_SIZE, CELL_SIZE);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_halign(gtk4::Align::Center);
    button.set_valign(gtk4::Align::Center);
    button.add_css_class("cell");

    let board_c  = ctx.board_widget.clone();
    let mine_c   = ctx.mine_label.clone();
    let timer_c  = ctx.timer_label.clone();
    let face_c   = ctx.face_button.clone();
    let start_c  = ctx.start_time.clone();
    let source_c = ctx.timer_source.clone();

    button.connect_clicked(move |_| {
        if ml.borrow().won { return; }

        // Start timer on first interaction.
        if !*started.borrow() {
            *started.borrow_mut() = true;
            start_timer(&start_c, &source_c, &timer_c);
        }

        ml.borrow_mut().toggle(x, y);

        let (won, remaining) = {
            let m = ml.borrow();
            (m.won, m.mines_remaining())
        };

        // Update mine counter.
        if let Some(lbl) = mine_c.borrow().as_ref() {
            lbl.set_text(&format!("{:03}", remaining));
        }

        // Win feedback.
        if won {
            if let Some(btn) = face_c.borrow().as_ref() {
                btn.set_label("\u{1F60E}"); // 😎
            }
            stop_timer(&source_c);
        }

        // Redraw.
        if let Some(board) = board_c.borrow().as_ref() {
            do_update_board(board, &ml);
        }
    });

    button
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn do_update_board(board: &gtk4::Widget, ml: &Rc<RefCell<Ml>>) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None => return,
    };
    let ml = ml.borrow();
    for y in 0..ml.height {
        for x in 0..ml.width {
            if let Some(w) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = w.downcast_ref::<Button>() {
                    render_cell(btn, ml.placed[y][x], ml.display_count(x, y));
                }
            }
        }
    }
}

/// No-op: MinelayerState is captured by closures; the global dispatcher
/// cannot access it, so this path is unused in practice.
pub fn update_board(_game: &Rc<RefCell<Game>>, _board: &gtk4::Widget) {}

fn render_cell(btn: &Button, placed: bool, display: i32) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{}", i));
    }

    if placed {
        // Player has placed a mine here.
        btn.add_css_class("cell-mine");
        btn.set_label("\u{1F4A3}"); // 💣
    } else if display > 0 {
        // Still needs cancelling - show remaining count with classic coloring.
        let n = (display as u8).clamp(1, 8);
        btn.add_css_class("cell-revealed");
        btn.add_css_class(&format!("cell-{}", n));
        btn.set_label(&display.to_string());
    } else if display < 0 {
        // Over-mined: too many mines placed nearby.
        btn.add_css_class("cell-mine");
        btn.set_label(&display.to_string());
    } else {
        // Fully cancelled / genuinely empty zone.
        btn.add_css_class("cell-revealed");
        btn.set_label(" ");
    }
}
