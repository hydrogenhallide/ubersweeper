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
                    render_cell(btn, &game_ref, x, y);
                }
            }
        }
    }
}

// Colour stops matching style.css cell-zero and cell-1 … cell-8.
// Stop 0: cell-zero is opacity:0.25 on fg - approximated as mid-grey.
// Stops 1–4 use mix(@theme_fg_color, pure, 0.5) in CSS; we use the pure colours
// so interpolation looks good regardless of theme.
const STOPS: [(u8, u8, u8); 9] = [
    (0x88, 0x88, 0x88), // 0 – grey (cell-zero: faint fg)
    (0x00, 0x66, 0xff), // 1 – blue
    (0x00, 0xcc, 0x00), // 2 – green
    (0xff, 0x00, 0x00), // 3 – red
    (0x00, 0x00, 0xcc), // 4 – dark blue
    (0xff, 0x80, 0x00), // 5 – orange
    (0x00, 0xcc, 0xcc), // 6 – teal
    (0xb0, 0xb0, 0xb0), // 7 – light grey (approximates @theme_fg_color)
    (0x80, 0x80, 0x80), // 8 – grey
];

/// Linearly interpolate between the 9 colour stops for a value in [0, 8].
fn gradient_color(v: f64) -> (u8, u8, u8) {
    let v = v.clamp(0.0, 8.0);
    let lo = v.floor() as usize;       // index of lower stop (0–7)
    let hi = (lo + 1).min(8);         // index of upper stop
    let t  = v - lo as f64;           // fractional part 0..1
    let (r0, g0, b0) = STOPS[lo];
    let (r1, g1, b1) = STOPS[hi];
    let lerp = |a: u8, b: u8| -> u8 {
        (a as f64 + t * (b as f64 - a as f64)).round() as u8
    };
    (lerp(r0, r1), lerp(g0, g1), lerp(b0, b1))
}

fn render_cell(btn: &Button, game: &Game, x: usize, y: usize) {
    let cell = &game.grid[y][x];
    let w = game.width;
    let h = game.height;

    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");

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
            // Sum the Classic adjacent-mine counts of all 8 surrounding positions.
            // Out-of-bounds positions contribute 0; always divide by 8.
            let mut sum = 0i32;
            for dy in -1_isize..=1 {
                for dx in -1_isize..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    if nx >= 0 && nx < w as isize && ny >= 0 && ny < h as isize {
                        sum += game.grid[ny as usize][nx as usize].adjacent_mines as i32;
                    }
                }
            }
            if sum == 0 {
                btn.set_label(" ");
                return;
            }
            let avg = sum as f64 / 8.0;
            let (r, g, b) = gradient_color(avg);
            // Set coloured label via Pango markup on the button's child Label.
            let markup = format!(
                "<span foreground=\"#{r:02x}{g:02x}{b:02x}\">{avg:.1}</span>"
            );
            if let Some(label) = btn.child().and_then(|c| c.downcast::<gtk4::Label>().ok()) {
                label.set_markup(&markup);
            }
        }
    }
}
