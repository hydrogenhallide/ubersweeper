use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Grid};
use rand::seq::SliceRandom;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, update_face, update_mine_counter};

const PANIC_SECS: u32 = 5;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_frontier(game: &Game) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for y in 0..game.height {
        for x in 0..game.width {
            if game.grid[y][x].state != CellState::Hidden { continue; }
            'cell: for dy in -1_isize..=1 {
                for dx in -1_isize..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    if nx >= 0 && nx < game.width as isize
                        && ny >= 0 && ny < game.height as isize
                        && game.grid[ny as usize][nx as usize].state == CellState::Revealed
                    {
                        out.push((x, y));
                        break 'cell;
                    }
                }
            }
        }
    }
    out
}

fn set_countdown_label(timer_label: &Rc<RefCell<Option<gtk4::Label>>>, n: u32) {
    if let Some(lbl) = timer_label.borrow().as_ref() {
        lbl.set_text(&n.to_string());
    }
}

/// Reset the player's countdown and update the label. Call on every interaction.
fn reset_countdown(
    countdown: &Rc<RefCell<u32>>,
    timer_label: &Rc<RefCell<Option<gtk4::Label>>>,
) {
    *countdown.borrow_mut() = PANIC_SECS;
    set_countdown_label(timer_label, PANIC_SECS);
}

/// Start the 1-second repeating panic timer. Stores its SourceId in `timer_source`
/// so that the normal stop_timer / reset_game path cancels it automatically.
fn start_panic_timer(
    countdown:    Rc<RefCell<u32>>,
    timer_source: Rc<RefCell<Option<gtk4::glib::SourceId>>>,
    timer_label:  Rc<RefCell<Option<gtk4::Label>>>,
    game:         Rc<RefCell<Game>>,
    board:        Rc<RefCell<Option<gtk4::Widget>>>,
    mine_label:   Rc<RefCell<Option<gtk4::Label>>>,
    face_button:  Rc<RefCell<Option<Button>>>,
    // We pass timer_source again so update_face can stop the game clock on win/loss.
    // Since Panic reuses timer_source for itself, this correctly cancels on game-end.
    timer_src2:   Rc<RefCell<Option<gtk4::glib::SourceId>>>,
) {
    let source_id = gtk4::glib::timeout_add_local(Duration::from_secs(1), move || {
        if !matches!(game.borrow().state, GameState::Playing) {
            return gtk4::glib::ControlFlow::Break;
        }

        let new_val = {
            let mut cd = countdown.borrow_mut();
            *cd = cd.saturating_sub(1);
            *cd
        };
        set_countdown_label(&timer_label, new_val);

        if new_val == 0 {
            // Auto-reveal a random frontier tile.
            let frontier = collect_frontier(&game.borrow());
            if let Some(&(rx, ry)) = frontier.choose(&mut rand::thread_rng()) {
                game.borrow_mut().reveal(rx, ry);
                if let Some(b) = board.borrow().as_ref() {
                    update_board(&game, b);
                }
                update_mine_counter(&game, &mine_label);
                update_face(&game, &face_button, &timer_src2);
            }
            // Reset countdown (also updates label).
            *countdown.borrow_mut() = PANIC_SECS;
            set_countdown_label(&timer_label, PANIC_SECS);
        }

        gtk4::glib::ControlFlow::Continue
    });

    *timer_source.borrow_mut() = Some(source_id);
}

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

    // Show initial countdown in the timer label.
    set_countdown_label(&ctx.timer_label, PANIC_SECS);

    let countdown: Rc<RefCell<u32>> = Rc::new(RefCell::new(PANIC_SECS));
    let panic_started: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx, &countdown, &panic_started);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

// ---------------------------------------------------------------------------
// Rendering (identical to Classic)
// ---------------------------------------------------------------------------

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
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{i}")); }

    match cell.state {
        CellState::Hidden => { btn.set_label(" "); }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3 | CellState::FlaggedNegative => {
            btn.set_label("\u{1F6A9}");
        }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else if cell.adjacent_mines > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adjacent_mines));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Button factory
// ---------------------------------------------------------------------------

fn make_cell_button(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    countdown:     &Rc<RefCell<u32>>,
    panic_started: &Rc<RefCell<bool>>,
) -> Button {
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click
    {
        let game_c         = ctx.game.clone();
        let board_c        = ctx.board_widget.clone();
        let mine_c         = ctx.mine_label.clone();
        let timer_c        = ctx.timer_label.clone();
        let face_c         = ctx.face_button.clone();
        let source_c       = ctx.timer_source.clone();
        let countdown_c    = countdown.clone();
        let started_c      = panic_started.clone();
        btn.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    reset_countdown(&countdown_c, &timer_c);
                    if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
                    update_mine_counter(&game_c, &mine_c);
                    update_face(&game_c, &face_c, &source_c);
                    return;
                }
            }
            let was_ready = game_c.borrow().state == GameState::Ready;
            game_c.borrow_mut().reveal(x, y);
            reset_countdown(&countdown_c, &timer_c);
            if was_ready
                && game_c.borrow().state == GameState::Playing
                && !*started_c.borrow()
            {
                *started_c.borrow_mut() = true;
                start_panic_timer(
                    countdown_c.clone(),
                    source_c.clone(),
                    timer_c.clone(),
                    game_c.clone(),
                    board_c.clone(),
                    mine_c.clone(),
                    face_c.clone(),
                    source_c.clone(),
                );
            }
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - flag
    {
        let right       = GestureClick::new();
        right.set_button(3);
        let game_c      = ctx.game.clone();
        let board_c     = ctx.board_widget.clone();
        let mine_c      = ctx.mine_label.clone();
        let timer_c     = ctx.timer_label.clone();
        let countdown_c = countdown.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            game_c.borrow_mut().toggle_flag(x, y);
            reset_countdown(&countdown_c, &timer_c);
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
        });
        btn.add_controller(right);
    }

    // Middle click - chord
    {
        let middle      = GestureClick::new();
        middle.set_button(2);
        let game_c      = ctx.game.clone();
        let board_c     = ctx.board_widget.clone();
        let mine_c      = ctx.mine_label.clone();
        let timer_c     = ctx.timer_label.clone();
        let face_c      = ctx.face_button.clone();
        let source_c    = ctx.timer_source.clone();
        let countdown_c = countdown.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !game_c.borrow_mut().chord(x, y) { return; }
            reset_countdown(&countdown_c, &timer_c);
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        btn.add_controller(middle);
    }

    btn
}
