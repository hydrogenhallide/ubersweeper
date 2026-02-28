use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

const DRIFT_EVERY: u8 = 3;

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

    let drift_counter: Rc<RefCell<u8>> = Rc::new(RefCell::new(0));

    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx, &drift_counter);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

// ---------------------------------------------------------------------------
// Rendering - identical to Classic
// ---------------------------------------------------------------------------

pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    super::classic::update_board(game, board);
}

// ---------------------------------------------------------------------------
// Post-action: update UI then maybe drift
// ---------------------------------------------------------------------------

fn post_action(
    game:          &Rc<RefCell<Game>>,
    board:         &Rc<RefCell<Option<gtk4::Widget>>>,
    mine_label:    &Rc<RefCell<Option<gtk4::Label>>>,
    face_button:   &Rc<RefCell<Option<Button>>>,
    timer_source:  &Rc<RefCell<Option<gtk4::glib::SourceId>>>,
    drift_counter: &Rc<RefCell<u8>>,
) {
    if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
    update_mine_counter(game, mine_label);
    update_face(game, face_button, timer_source);

    // Only count moves (and drift) while the game is live.
    if !matches!(game.borrow().state, GameState::Playing) { return; }

    let do_drift = {
        let mut dc = drift_counter.borrow_mut();
        *dc += 1;
        if *dc >= DRIFT_EVERY { *dc = 0; true } else { false }
    };

    if do_drift {
        game.borrow_mut().drift_mines();
        if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
        update_mine_counter(game, mine_label);
    }
}

// ---------------------------------------------------------------------------
// Button factory
// ---------------------------------------------------------------------------

fn make_cell_button(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    drift_counter: &Rc<RefCell<u8>>,
) -> Button {
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
        let drift_c  = drift_counter.clone();
        btn.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if !matches!(gr.state, GameState::Ready | GameState::Playing) { return; }
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    post_action(&game_c, &board_c, &mine_c, &face_c, &source_c, &drift_c);
                    return;
                }
            }
            let was_ready = game_c.borrow().state == GameState::Ready;
            game_c.borrow_mut().reveal(x, y);
            if was_ready && game_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }
            post_action(&game_c, &board_c, &mine_c, &face_c, &source_c, &drift_c);
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
        let drift_c  = drift_counter.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !game_c.borrow_mut().toggle_flag(x, y) { return; }
            post_action(&game_c, &board_c, &mine_c, &face_c, &source_c, &drift_c);
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
        let drift_c  = drift_counter.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !game_c.borrow_mut().chord(x, y) { return; }
            post_action(&game_c, &board_c, &mine_c, &face_c, &source_c, &drift_c);
        });
        btn.add_controller(middle);
    }

    btn
}
