pub mod average;
pub mod crosswalk;
pub mod encrypted;
pub mod minelayer;
pub mod blindsweeper;
pub mod infection;
pub mod nested;
pub mod chain;
pub mod offset;
pub mod classic;
pub mod drift;
pub mod multi_mines;
pub mod negative_mines;
pub mod marathon;
pub mod merge;
pub mod panic;
pub mod relative;
pub mod rgb;
pub mod rotation;
pub mod subtract;
pub mod threed;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Grid};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
    Average,
    Chain,
    Crosswalk,
    Classic,
    Encrypted,
    Infection,
    Nested,
    Drift,
    Offset,
    Blindsweeper,
    NegativeMines,
    MultiMines,
    Marathon,
    Minelayer,
    Merge,
    Panic,
    Relative,
    Rgb,
    Rotation,
    Subtract,
    Threed,
}

/// Everything a variant board needs to wire up its interaction handlers.
pub struct BoardContext {
    pub game: Rc<RefCell<Game>>,
    /// The Rc that will hold the board widget once created; handlers read it at click time.
    pub board_widget: Rc<RefCell<Option<gtk4::Widget>>>,
    pub mine_label: Rc<RefCell<Option<gtk4::Label>>>,
    pub timer_label: Rc<RefCell<Option<gtk4::Label>>>,
    pub face_button: Rc<RefCell<Option<Button>>>,
    pub start_time: Rc<RefCell<Option<Instant>>>,
    pub timer_source: Rc<RefCell<Option<glib::SourceId>>>,
}

/// Create the board widget for the current variant.
pub fn create_board(variant: Variant, ctx: &BoardContext) -> gtk4::Widget {
    match variant {
        Variant::Average    => average::create_board(ctx),
        Variant::Chain      => chain::create_board(ctx),
        Variant::Classic    => classic::create_board(ctx),
        Variant::Crosswalk  => crosswalk::create_board(ctx),
        Variant::Encrypted  => encrypted::create_board(ctx),
        Variant::Infection  => infection::create_board(ctx),
        Variant::Nested     => nested::create_board(ctx),
        Variant::Drift   => drift::create_board(ctx),
        Variant::Offset => offset::create_board(ctx),
        Variant::Blindsweeper => blindsweeper::create_board(ctx),
        Variant::NegativeMines => negative_mines::create_board(ctx),
        Variant::MultiMines => multi_mines::create_board(ctx),
        Variant::Marathon   => marathon::create_board(ctx),
        Variant::Minelayer  => minelayer::create_board(ctx),
        Variant::Merge => merge::create_board(ctx),
        Variant::Panic => panic::create_board(ctx),
        Variant::Relative => relative::create_board(ctx),
        Variant::Rgb      => rgb::create_board(ctx),
        Variant::Rotation => rotation::create_board(ctx),
        Variant::Subtract => subtract::create_board(ctx),
        Variant::Threed   => threed::create_board(ctx),
    }
}

/// Update the board visuals from current game state.
/// (Currently unused - variants capture their update fn directly at board-creation time,
/// but this exists for external callers that need to force a refresh.)
#[allow(dead_code)]
pub fn update_board(variant: Variant, game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    match variant {
        Variant::Average    => average::update_board(game, board),
        Variant::Chain      => chain::update_board(game, board),
        Variant::Classic    => classic::update_board(game, board),
        Variant::Crosswalk  => crosswalk::update_board(game, board),
        Variant::Encrypted  => encrypted::update_board(game, board),
        Variant::Infection  => infection::update_board(game, board),
        Variant::Nested     => nested::update_board(game, board),
        Variant::Drift   => drift::update_board(game, board),
        Variant::Offset => offset::update_board(game, board),
        Variant::Blindsweeper => blindsweeper::update_board(game, board),
        Variant::NegativeMines => negative_mines::update_board(game, board),
        Variant::MultiMines => multi_mines::update_board(game, board),
        Variant::Marathon   => marathon::update_board(game, board),
        Variant::Minelayer  => minelayer::update_board(game, board),
        Variant::Merge => merge::update_board(game, board),
        Variant::Panic => panic::update_board(game, board),
        Variant::Relative => relative::update_board(game, board),
        Variant::Rgb      => rgb::update_board(game, board),
        Variant::Rotation => rotation::update_board(game, board),
        Variant::Subtract => subtract::update_board(game, board),
        Variant::Threed   => threed::update_board(game, board),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (timer, face, mine counter) used by variant click handlers
// ---------------------------------------------------------------------------

pub(crate) fn update_mine_counter(
    game: &Rc<RefCell<Game>>,
    mine_label: &Rc<RefCell<Option<gtk4::Label>>>,
) {
    let remaining = game.borrow().remaining_mines();
    if let Some(label) = mine_label.borrow().as_ref() {
        label.set_text(&format!("{:03}", remaining));
    }
}

pub(crate) fn update_face(
    game: &Rc<RefCell<Game>>,
    face_button: &Rc<RefCell<Option<Button>>>,
    timer_source: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    let state = game.borrow().state;
    if let Some(btn) = face_button.borrow().as_ref() {
        match state {
            GameState::Won => {
                btn.set_label("\u{1F60E}"); // 😎
                stop_timer(timer_source);
            }
            GameState::Lost => {
                btn.set_label("\u{1F635}"); // 😵
                stop_timer(timer_source);
            }
            _ => {
                btn.set_label("\u{1F642}"); // 🙂
            }
        }
    }
}

pub fn start_timer(
    start_time: &Rc<RefCell<Option<Instant>>>,
    timer_source: &Rc<RefCell<Option<glib::SourceId>>>,
    timer_label: &Rc<RefCell<Option<gtk4::Label>>>,
) {
    *start_time.borrow_mut() = Some(Instant::now());
    let start_c = start_time.clone();
    let label_c = timer_label.clone();
    let source_id = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        if let Some(start) = *start_c.borrow() {
            let elapsed = start.elapsed().as_secs().min(999);
            if let Some(label) = label_c.borrow().as_ref() {
                label.set_text(&format!("{:03}", elapsed));
            }
        }
        glib::ControlFlow::Continue
    });
    *timer_source.borrow_mut() = Some(source_id);
}

pub fn stop_timer(timer_source: &Rc<RefCell<Option<glib::SourceId>>>) {
    if let Some(source_id) = timer_source.borrow_mut().take() {
        source_id.remove();
    }
}

// ---------------------------------------------------------------------------
// Shared grid board builder - used by any variant that wants a Grid layout.
// Takes the variant's update function so each cell's handlers call the right one.
// ---------------------------------------------------------------------------

pub(crate) fn build_grid_board<F>(ctx: &BoardContext, update_fn: F) -> gtk4::Widget
where
    F: Fn(&Rc<RefCell<Game>>, &gtk4::Widget) + Clone + 'static,
{
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
            let btn = make_cell_button(x, y, ctx, update_fn.clone());
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

fn make_cell_button<F>(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    update_fn: F,
) -> Button
where
    F: Fn(&Rc<RefCell<Game>>, &gtk4::Widget) + Clone + 'static,
{
    let button = Button::with_label(" ");
    button.set_size_request(CELL_SIZE, CELL_SIZE);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_halign(gtk4::Align::Center);
    button.set_valign(gtk4::Align::Center);
    button.add_css_class("cell");

    // Left click - reveal (or reveal neighbors if clicking a revealed number)
    {
        let game_c = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c = ctx.mine_label.clone();
        let timer_c = ctx.timer_label.clone();
        let face_c = ctx.face_button.clone();
        let start_c = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();
        let upd = update_fn.clone();
        button.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    if let Some(b) = board_c.borrow().as_ref() { upd(&game_c, b); }
                    update_mine_counter(&game_c, &mine_c);
                    update_face(&game_c, &face_c, &source_c);
                    return;
                }
            }
            let was_ready = game_c.borrow().state == GameState::Ready;
            game_c.borrow_mut().reveal(x, y);
            if was_ready && game_c.borrow().state == GameState::Playing {
                start_timer(&start_c, &source_c, &timer_c);
            }
            if let Some(b) = board_c.borrow().as_ref() { upd(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - flag / cycle
    {
        let right_click = GestureClick::new();
        right_click.set_button(3);
        let game_c = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c = ctx.mine_label.clone();
        let upd = update_fn.clone();
        right_click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            game_c.borrow_mut().toggle_flag(x, y);
            if let Some(b) = board_c.borrow().as_ref() { upd(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
        });
        button.add_controller(right_click);
    }

    // Middle click - chord
    {
        let middle_click = GestureClick::new();
        middle_click.set_button(2);
        let game_c = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c = ctx.mine_label.clone();
        let face_c = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        let upd = update_fn.clone();
        middle_click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            game_c.borrow_mut().chord(x, y);
            if let Some(b) = board_c.borrow().as_ref() { upd(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        button.add_controller(middle_click);
    }

    button
}
