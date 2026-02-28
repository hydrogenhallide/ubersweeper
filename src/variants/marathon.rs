use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, update_face, update_mine_counter};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// N = 2 * (mines in bottom 2 rows) + 5.
fn compute_n(game: &Game) -> u32 {
    let h = game.height;
    let mines: u32 = (h.saturating_sub(2)..h)
        .flat_map(|y| (0..game.width).map(move |x| (x, y)))
        .map(|(x, y)| game.grid[y][x].mines as u32)
        .sum();
    2 * mines + 5
}

/// Win if every cell is either correctly Revealed (non-mine) or correctly Flagged (mine).
fn check_marathon_win(game: &mut Game) {
    for y in 0..game.height {
        for x in 0..game.width {
            let cell = &game.grid[y][x];
            let ok = match cell.state {
                CellState::Revealed        => cell.mines == 0,
                CellState::Flagged         => cell.mines == 1 && !cell.is_negative,
                CellState::FlaggedNegative => cell.mines == 1 && cell.is_negative,
                CellState::Flagged2        => cell.mines == 2,
                CellState::Flagged3        => cell.mines == 3,
                CellState::Hidden          => false,
            };
            if !ok { return; }
        }
    }
    game.state = GameState::Won;
}

fn set_move_label(timer_label: &Rc<RefCell<Option<gtk4::Label>>>, n: u32) {
    if let Some(lbl) = timer_label.borrow().as_ref() {
        lbl.set_text(&format!("{:03}", n));
    }
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

    // Disable the normal win check - we use our own full-board win.
    ctx.game.borrow_mut().endless = true;

    // Before the first click mines aren't placed yet, so use the expected N
    // as the initial display. It snaps to the actual value on first action.
    let expected_n = {
        let g = ctx.game.borrow();
        let expected_bottom2_mines =
            (g.mine_density * g.width as f64 * 2.0).round() as u32;
        2 * expected_bottom2_mines + 5
    };

    let move_count: Rc<RefCell<u32>> = Rc::new(RefCell::new(expected_n));
    let rows_cleared: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));

    set_move_label(&ctx.timer_label, expected_n);

    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx, &move_count, &rows_cleared);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None    => return,
    };
    let game_ref = game.borrow();
    let bottom   = game_ref.height - 1;
    for y in 0..game_ref.height {
        for x in 0..game_ref.width {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    render_cell(btn, &game_ref.grid[y][x], y == bottom);
                }
            }
        }
    }
}

fn render_cell(btn: &Button, cell: &crate::game::Cell, is_bottom: bool) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-bottom");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{i}")); }

    match cell.state {
        CellState::Hidden => {
            btn.set_label(" ");
            if is_bottom { btn.add_css_class("cell-bottom"); }
        }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3 | CellState::FlaggedNegative => {
            btn.set_label("\u{1F6A9}");
            if is_bottom { btn.add_css_class("cell-bottom"); }
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
// Move accounting
// ---------------------------------------------------------------------------

fn on_move(
    game:         &Rc<RefCell<Game>>,
    board:        &Rc<RefCell<Option<gtk4::Widget>>>,
    mine_label:   &Rc<RefCell<Option<gtk4::Label>>>,
    timer_label:  &Rc<RefCell<Option<gtk4::Label>>>,
    face_button:  &Rc<RefCell<Option<Button>>>,
    timer_src:    &Rc<RefCell<Option<gtk4::glib::SourceId>>>,
    move_count:   &Rc<RefCell<u32>>,
    rows_cleared: &Rc<RefCell<u32>>,
) {
    // If the action ended the game naturally (mine hit), just refresh UI.
    if !matches!(game.borrow().state, GameState::Playing) {
        if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
        update_mine_counter(game, mine_label);
        update_face(game, face_button, timer_src);
        return;
    }

    // Decrement by exactly 1 per action (reveal or chord — never per cell revealed).
    let remaining = {
        let mut mc = move_count.borrow_mut();
        *mc = mc.saturating_sub(1);
        *mc
    };
    set_move_label(timer_label, remaining);

    // Check if the board is fully solved (marathon win).
    {
        let mut g = game.borrow_mut();
        check_marathon_win(&mut g);
    }
    if !matches!(game.borrow().state, GameState::Playing) {
        if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
        update_mine_counter(game, mine_label);
        update_face(game, face_button, timer_src);
        return;
    }

    if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
    update_mine_counter(game, mine_label);

    if remaining == 0 {
        let ok = game.borrow_mut().marathon_shift();
        if ok {
            *rows_cleared.borrow_mut() += 1;
            // Recompute N for the new bottom rows.
            let new_n = compute_n(&game.borrow());
            *move_count.borrow_mut() = new_n;
            set_move_label(timer_label, new_n);
        }
        if let Some(b) = board.borrow().as_ref() { update_board(game, b); }
        update_mine_counter(game, mine_label);
        update_face(game, face_button, timer_src);
    }
}

// ---------------------------------------------------------------------------
// Button factory
// ---------------------------------------------------------------------------

fn make_cell_button(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    move_count:   &Rc<RefCell<u32>>,
    rows_cleared: &Rc<RefCell<u32>>,
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
        let source_c = ctx.timer_source.clone();
        let move_c   = move_count.clone();
        let rows_c   = rows_cleared.clone();
        btn.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if !matches!(gr.state, GameState::Ready | GameState::Playing) { return; }
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    on_move(&game_c, &board_c, &mine_c, &timer_c, &face_c, &source_c, &move_c, &rows_c);
                    return;
                }
            }
            // Only count as a move if reveal actually did something (i.e. cell was Hidden).
            if !game_c.borrow_mut().reveal(x, y) { return; }
            on_move(&game_c, &board_c, &mine_c, &timer_c, &face_c, &source_c, &move_c, &rows_c);
        });
    }

    // Right click - flag (does NOT count as a move; no counter decrement, no shift).
    {
        let right    = GestureClick::new();
        right.set_button(3);
        let game_c   = ctx.game.clone();
        let board_c  = ctx.board_widget.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Playing) { return; }
            if !game_c.borrow_mut().toggle_flag(x, y) { return; }
            // Check marathon win (no move decrement for flags).
            { check_marathon_win(&mut game_c.borrow_mut()); }
            if let Some(b) = board_c.borrow().as_ref() { update_board(&game_c, b); }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
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
        let timer_c  = ctx.timer_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        let move_c   = move_count.clone();
        let rows_c   = rows_cleared.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !game_c.borrow_mut().chord(x, y) { return; }
            on_move(&game_c, &board_c, &mine_c, &timer_c, &face_c, &source_c, &move_c, &rows_c);
        });
        btn.add_controller(middle);
    }

    btn
}
