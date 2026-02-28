use gtk4::prelude::*;
use gtk4::{Button, EventControllerMotion, GestureClick, Grid};
use std::cell::RefCell;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

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

    // Shared hover position - motion controllers write it, click handlers read it.
    let hovered: Rc<RefCell<Option<(usize, usize)>>> = Rc::new(RefCell::new(None));

    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx, &hovered);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }

    grid.upcast()
}

#[allow(dead_code)]
pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    // Called by the dispatch table; in practice the closure from create_board is used.
    update_board_with(game, board, None);
}

fn update_board_with(
    game: &Rc<RefCell<Game>>,
    board: &gtk4::Widget,
    hovered: Option<(usize, usize)>,
) {
    let grid = match board.downcast_ref::<Grid>() {
        Some(g) => g,
        None => return,
    };
    let game_ref = game.borrow();
    let show_all = matches!(game_ref.state, GameState::Won | GameState::Lost);

    for y in 0..game_ref.height {
        for x in 0..game_ref.width {
            if let Some(widget) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = widget.downcast_ref::<Button>() {
                    if show_all || hovered == Some((x, y)) {
                        render_true(btn, &game_ref.grid[y][x]);
                    } else {
                        render_blind(btn);
                    }
                }
            }
        }
    }
}

// ── Cell rendering ────────────────────────────────────────────────────────────

/// Show the cell's true game state - identical to Classic rendering, except
/// empty revealed cells show a faint "0" so the player can distinguish them
/// from unrevealed cells while hovering.
fn render_true(btn: &Button, cell: &crate::game::Cell) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-zero");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{i}"));
    }
    match cell.state {
        CellState::Hidden => btn.set_label(" "),
        CellState::Flagged
        | CellState::Flagged2
        | CellState::Flagged3
        | CellState::FlaggedNegative => btn.set_label("\u{1F6A9}"),
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else if cell.adjacent_mines > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adjacent_mines));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else {
                btn.add_css_class("cell-zero");
                btn.set_label("0");
            }
        }
    }
}

/// Make the cell look unrevealed regardless of its actual game state.
fn render_blind(btn: &Button) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-zero");
    for i in 1u8..=8 {
        btn.remove_css_class(&format!("cell-{i}"));
    }
    btn.set_label(" ");
}

// ── Button factory ────────────────────────────────────────────────────────────

fn make_cell_button(
    x: usize,
    y: usize,
    ctx: &BoardContext,
    hovered: &Rc<RefCell<Option<(usize, usize)>>>,
) -> Button {
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click - reveal (or chord on a revealed cell)
    {
        let game_c = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c = ctx.mine_label.clone();
        let timer_c = ctx.timer_label.clone();
        let face_c = ctx.face_button.clone();
        let start_c = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();
        let hover_c = hovered.clone();
        btn.connect_clicked(move |_| {
            {
                let gr = game_c.borrow();
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !game_c.borrow_mut().chord(x, y) { return; }
                    if let Some(b) = board_c.borrow().as_ref() {
                        update_board_with(&game_c, b, *hover_c.borrow());
                    }
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
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_with(&game_c, b, *hover_c.borrow());
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - flag
    {
        let right = GestureClick::new();
        right.set_button(3);
        let game_c = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c = ctx.mine_label.clone();
        let hover_c = hovered.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            game_c.borrow_mut().toggle_flag(x, y);
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_with(&game_c, b, *hover_c.borrow());
            }
            update_mine_counter(&game_c, &mine_c);
        });
        btn.add_controller(right);
    }

    // Middle click - chord
    {
        let middle = GestureClick::new();
        middle.set_button(2);
        let game_c = ctx.game.clone();
        let board_c = ctx.board_widget.clone();
        let mine_c = ctx.mine_label.clone();
        let face_c = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        let hover_c = hovered.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            game_c.borrow_mut().chord(x, y);
            if let Some(b) = board_c.borrow().as_ref() {
                update_board_with(&game_c, b, *hover_c.borrow());
            }
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        btn.add_controller(middle);
    }

    // Hover - reveal cell under cursor, hide it when cursor leaves
    {
        let motion = EventControllerMotion::new();

        {
            let game_c = ctx.game.clone();
            let board_c = ctx.board_widget.clone();
            let hover_c = hovered.clone();
            motion.connect_enter(move |_, _, _| {
                *hover_c.borrow_mut() = Some((x, y));
                if let Some(b) = board_c.borrow().as_ref() {
                    update_board_with(&game_c, b, Some((x, y)));
                }
            });
        }

        {
            let game_c = ctx.game.clone();
            let board_c = ctx.board_widget.clone();
            let hover_c = hovered.clone();
            motion.connect_leave(move |_| {
                {
                    let mut h = hover_c.borrow_mut();
                    if *h != Some((x, y)) {
                        return;
                    }
                    *h = None;
                }
                if let Some(b) = board_c.borrow().as_ref() {
                    update_board_with(&game_c, b, None);
                }
            });
        }

        btn.add_controller(motion);
    }

    btn
}
