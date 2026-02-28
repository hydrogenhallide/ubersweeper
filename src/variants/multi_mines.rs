use gtk4::prelude::*;
use gtk4::{Button, DrawingArea, GestureClick, Grid, Overlay};
use pangocairo::cairo;
use std::cell::RefCell;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};

use super::BoardContext;

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
            let cell = make_cell_widget(x, y, ctx);
            grid.attach(&cell, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
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
                if let Some(overlay) = widget.downcast_ref::<Overlay>() {
                    // First child = Button (label/CSS for numbers)
                    if let Some(btn) = overlay.first_child()
                        .and_then(|w| w.downcast::<Button>().ok())
                    {
                        render_cell_btn(&btn, &game_ref.grid[y][x]);
                    }
                    // Second child = DrawingArea (emoji only)
                    if let Some(da) = overlay.first_child()
                        .and_then(|w| w.next_sibling())
                        .and_then(|w| w.downcast::<DrawingArea>().ok())
                    {
                        da.queue_draw();
                    }
                }
            }
        }
    }
}

/// Update the Button's label and CSS classes - identical to classic for non-emoji content.
fn render_cell_btn(btn: &Button, cell: &crate::game::Cell) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=24 {
        btn.remove_css_class(&format!("cell-{}", i));
    }

    match cell.state {
        // Emoji states: button shows blank, DrawingArea draws on top
        CellState::Hidden
        | CellState::Flagged
        | CellState::Flagged2
        | CellState::Flagged3
        | CellState::FlaggedNegative => {
            btn.set_label(" ");
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label(" "); // DrawingArea draws the bomb(s)
            } else if cell.adjacent_mines > 0 {
                let n = cell.adjacent_mines as u8;
                btn.add_css_class(&format!("cell-{}", n));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}

fn make_cell_widget(x: usize, y: usize, ctx: &BoardContext) -> Overlay {
    let overlay = Overlay::new();

    // ── Button: GTK appearance + all click handling + label for numbers ──────
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click - reveal (or reveal_neighbors on a revealed number)
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
                let cell = &gr.grid[y][x];
                if cell.state == CellState::Revealed && cell.adjacent_mines != 0 {
                    drop(gr);
                    game_c.borrow_mut().reveal_neighbors(x, y);
                    if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
                    super::update_mine_counter(&game_c, &mine_c);
                    super::update_face(&game_c, &face_c, &source_c);
                    return;
                }
            }
            let was_ready = game_c.borrow().state == GameState::Ready;
            game_c.borrow_mut().reveal(x, y);
            if was_ready && game_c.borrow().state == GameState::Playing {
                super::start_timer(&start_c, &source_c, &timer_c);
            }
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            super::update_mine_counter(&game_c, &mine_c);
            super::update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - cycle flag weight
    {
        let right = GestureClick::new();
        right.set_button(3);
        let game_c  = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c  = ctx.mine_label.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            game_c.borrow_mut().toggle_flag(x, y);
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            super::update_mine_counter(&game_c, &mine_c);
        });
        btn.add_controller(right);
    }

    // Middle click - chord
    {
        let middle = GestureClick::new();
        middle.set_button(2);
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            game_c.borrow_mut().chord(x, y);
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            super::update_mine_counter(&game_c, &mine_c);
            super::update_face(&game_c, &face_c, &source_c);
        });
        btn.add_controller(middle);
    }

    overlay.set_child(Some(&btn));

    // ── DrawingArea: emoji-only overlay, transparent for numbers/blank ────────
    let da = DrawingArea::new();
    da.set_can_target(false); // clicks pass through to the Button below
    {
        let game_c = ctx.game.clone();
        da.set_draw_func(move |da, cr, width, height| {
            let pctx = da.pango_context();
            let game_ref = game_c.borrow();
            draw_emoji_overlay(cr, &pctx, width, height, &game_ref.grid[y][x]);
        });
    }
    overlay.add_overlay(&da);
    overlay.set_measure_overlay(&da, false); // don't let DrawingArea affect cell size

    overlay
}

// ── Drawing ──────────────────────────────────────────────────────────────────

/// Only draws emoji. Numbers and blank cells are left transparent so the
/// Button's native GTK label shows through underneath.
fn draw_emoji_overlay(
    cr: &cairo::Context,
    pctx: &pango::Context,
    width: i32,
    height: i32,
    cell: &crate::game::Cell,
) {
    let w = width as f64;
    let h = height as f64;

    match cell.state {
        CellState::Hidden | CellState::FlaggedNegative => {}

        CellState::Flagged => draw_emoji_centered(cr, pctx, w, h, "\u{1F6A9}", 11.0),

        CellState::Flagged2 => {
            draw_emoji_at(cr, pctx, "\u{1F6A9}", 8.0, w * 0.28, h * 0.50);
            draw_emoji_at(cr, pctx, "\u{1F6A9}", 8.0, w * 0.72, h * 0.50);
        }

        CellState::Flagged3 => {
            draw_emoji_at(cr, pctx, "\u{1F6A9}", 7.0, w * 0.50, h * 0.27);
            draw_emoji_at(cr, pctx, "\u{1F6A9}", 7.0, w * 0.28, h * 0.73);
            draw_emoji_at(cr, pctx, "\u{1F6A9}", 7.0, w * 0.72, h * 0.73);
        }

        CellState::Revealed if cell.mines == 1 => {
            draw_emoji_centered(cr, pctx, w, h, "\u{1F4A3}", 11.0);
        }
        CellState::Revealed if cell.mines == 2 => {
            draw_emoji_at(cr, pctx, "\u{1F4A3}", 8.0, w * 0.28, h * 0.50);
            draw_emoji_at(cr, pctx, "\u{1F4A3}", 8.0, w * 0.72, h * 0.50);
        }
        CellState::Revealed if cell.mines >= 3 => {
            draw_emoji_at(cr, pctx, "\u{1F4A3}", 7.0, w * 0.50, h * 0.27);
            draw_emoji_at(cr, pctx, "\u{1F4A3}", 7.0, w * 0.28, h * 0.73);
            draw_emoji_at(cr, pctx, "\u{1F4A3}", 7.0, w * 0.72, h * 0.73);
        }

        // Number or blank revealed cell - button label handles it, draw nothing
        CellState::Revealed => {}
    }
}

fn draw_emoji_centered(
    cr: &cairo::Context,
    pctx: &pango::Context,
    w: f64,
    h: f64,
    emoji: &str,
    pt: f64,
) {
    draw_emoji_at(cr, pctx, emoji, pt, w * 0.5, h * 0.5);
}

fn draw_emoji_at(
    cr: &cairo::Context,
    pctx: &pango::Context,
    emoji: &str,
    pt: f64,
    cx: f64,
    cy: f64,
) {
    let layout = pango::Layout::new(pctx);
    let desc = pango::FontDescription::from_string(&format!("emoji {pt}"));
    layout.set_font_description(Some(&desc));
    layout.set_text(emoji);
    let (pw, ph) = layout.pixel_size();
    cr.move_to(cx - pw as f64 / 2.0, cy - ph as f64 / 2.0);
    pangocairo::functions::show_layout(cr, &layout);
}
