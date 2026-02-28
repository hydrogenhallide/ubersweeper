use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::glib;
use gtk4::graphene;
use gtk4::gsk;
use gtk4::prelude::*;
use gtk4::{Button, Fixed, GestureClick, Grid};

use crate::constants::CELL_SIZE;
use crate::game::{CellState, Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

const ANIM_FRAMES: u32 = 15; // ~240 ms at 16 ms / frame
const FRAME_MS:    u64 = 16;

// ---------------------------------------------------------------------------
// Context bundle
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ctx {
    game:         Rc<RefCell<Game>>,
    board_widget: Rc<RefCell<Option<gtk4::Widget>>>,
    mine_label:   Rc<RefCell<Option<gtk4::Label>>>,
    timer_label:  Rc<RefCell<Option<gtk4::Label>>>,
    face_button:  Rc<RefCell<Option<Button>>>,
    start_time:   Rc<RefCell<Option<Instant>>>,
    timer_source: Rc<RefCell<Option<glib::SourceId>>>,
    animating:    Rc<RefCell<bool>>,
}

impl Ctx {
    fn from(ctx: &BoardContext) -> Self {
        Ctx {
            game:         ctx.game.clone(),
            board_widget: ctx.board_widget.clone(),
            mine_label:   ctx.mine_label.clone(),
            timer_label:  ctx.timer_label.clone(),
            face_button:  ctx.face_button.clone(),
            start_time:   ctx.start_time.clone(),
            timer_source: ctx.timer_source.clone(),
            animating:    Rc::new(RefCell::new(false)),
        }
    }
}

// ---------------------------------------------------------------------------
// Board creation
// ---------------------------------------------------------------------------

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let rctx = Ctx::from(ctx);
    let (w, h) = { let g = rctx.game.borrow(); (g.width, g.height) };

    // Outer Fixed: persistent wrapper that stays alive across rotations.
    // Overflow::Visible lets the board peek outside its box while mid-spin.
    let outer = Fixed::new();
    outer.set_size_request((w * CELL_SIZE as usize) as i32,
                           (h * CELL_SIZE as usize) as i32);
    outer.set_overflow(gtk4::Overflow::Visible);
    outer.set_halign(gtk4::Align::Center);

    let inner = build_inner(w, h, &rctx);
    outer.put(&inner, 0.0, 0.0);

    outer.upcast()
}

fn build_inner(width: usize, height: usize, ctx: &Ctx) -> Grid {
    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);
    for y in 0..height {
        for x in 0..width {
            let btn = make_cell_button(x, y, ctx);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
        }
    }
    grid
}

// ---------------------------------------------------------------------------
// Rendering - delegate to Classic
// ---------------------------------------------------------------------------

pub fn update_board(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    if let Some(outer) = board.downcast_ref::<Fixed>() {
        if let Some(inner) = outer.first_child().and_then(|w| w.downcast::<Grid>().ok()) {
            super::classic::update_board(game, inner.upcast_ref());
        }
    }
}

// ---------------------------------------------------------------------------
// Post-action: refresh UI then start rotation
// ---------------------------------------------------------------------------

fn post_action(ctx: &Ctx) {
    if let Some(b) = ctx.board_widget.borrow().as_ref() { update_board(&ctx.game, b); }
    update_mine_counter(&ctx.game, &ctx.mine_label);
    update_face(&ctx.game, &ctx.face_button, &ctx.timer_source);

    if !matches!(ctx.game.borrow().state, GameState::Playing) { return; }

    let outer: Fixed = {
        let bw = ctx.board_widget.borrow();
        match bw.as_ref().and_then(|w| w.downcast_ref::<Fixed>()) {
            Some(f) => f.clone(),
            None    => return,
        }
    };

    animate_rotation(outer, ctx.clone());
}

// ---------------------------------------------------------------------------
// Rotation animation
//
// Rotates the inner Grid 0 → 90° around the board's centre using GskTransform.
// At 90° the data is rotated via rotate_90_cw(), the grid is rebuilt, and the
// transform resets to 0°.  Because the old board at 90° is visually identical
// to the new board at 0°, the rebuild is imperceptible.
// ---------------------------------------------------------------------------

fn animate_rotation(outer: Fixed, ctx: Ctx) {
    *ctx.animating.borrow_mut() = true;
    let frame = Rc::new(RefCell::new(0u32));

    glib::timeout_add_local(Duration::from_millis(FRAME_MS), move || {
        let f = { let mut v = frame.borrow_mut(); *v += 1; *v };

        let inner = match outer.first_child() {
            Some(w) => w,
            None    => { *ctx.animating.borrow_mut() = false; return glib::ControlFlow::Break; }
        };

        if f < ANIM_FRAMES {
            // Smoothstep easing: accelerate then decelerate
            let t = f as f32 / ANIM_FRAMES as f32;
            let t = t * t * t * (t * (6.0 * t - 15.0) + 10.0);
            let angle = t * 90.0_f32;

            let (cx, cy) = board_centre(&ctx.game);
            let transform = gsk::Transform::new()
                .translate(&graphene::Point::new(cx, cy))
                .rotate(angle)
                .translate(&graphene::Point::new(-cx, -cy));
            outer.set_child_transform(&inner, Some(&transform));

            // Resize the outer container each frame so the window tracks the
            // rotating bounding box: w·cosθ + h·sinθ  ×  w·sinθ + h·cosθ
            let (w, h) = { let g = ctx.game.borrow(); (g.width, g.height) };
            let cell = CELL_SIZE as f32;
            let rad  = angle.to_radians();
            let (sin_a, cos_a) = (rad.sin().abs(), rad.cos().abs());
            let bw = (w as f32 * cell * cos_a + h as f32 * cell * sin_a) as i32;
            let bh = (w as f32 * cell * sin_a + h as f32 * cell * cos_a) as i32;
            outer.set_size_request(bw, bh);

            glib::ControlFlow::Continue

        } else {
            // Animation complete.  Clear transform, rotate data, rebuild grid.
            outer.set_child_transform(&inner, None);
            outer.remove(&inner);

            ctx.game.borrow_mut().rotate_90_cw();

            let (w, h) = { let g = ctx.game.borrow(); (g.width, g.height) };
            let new_inner = build_inner(w, h, &ctx);
            outer.put(&new_inner, 0.0, 0.0);
            outer.set_size_request((w * CELL_SIZE as usize) as i32,
                                   (h * CELL_SIZE as usize) as i32);

            // Paint the new grid immediately so there's no blank frame.
            update_board(&ctx.game, outer.upcast_ref());
            update_mine_counter(&ctx.game, &ctx.mine_label);
            update_face(&ctx.game, &ctx.face_button, &ctx.timer_source);

            *ctx.animating.borrow_mut() = false;
            glib::ControlFlow::Break
        }
    });
}

fn board_centre(game: &Rc<RefCell<Game>>) -> (f32, f32) {
    let g = game.borrow();
    (g.width  as f32 * CELL_SIZE as f32 / 2.0,
     g.height as f32 * CELL_SIZE as f32 / 2.0)
}

// ---------------------------------------------------------------------------
// Button factory
// ---------------------------------------------------------------------------

fn make_cell_button(x: usize, y: usize, ctx: &Ctx) -> Button {
    let btn = Button::with_label(" ");
    btn.set_size_request(CELL_SIZE, CELL_SIZE);
    btn.set_hexpand(false);
    btn.set_vexpand(false);
    btn.set_halign(gtk4::Align::Center);
    btn.set_valign(gtk4::Align::Center);
    btn.add_css_class("cell");

    // Left click - reveal or chord
    {
        let ctx_c = ctx.clone();
        btn.connect_clicked(move |_| {
            if *ctx_c.animating.borrow() { return; }
            {
                let gr = ctx_c.game.borrow();
                if !matches!(gr.state, GameState::Ready | GameState::Playing) { return; }
                if gr.grid[y][x].state == CellState::Revealed {
                    drop(gr);
                    if !ctx_c.game.borrow_mut().chord(x, y) { return; }
                    post_action(&ctx_c);
                    return;
                }
            }
            let was_ready = ctx_c.game.borrow().state == GameState::Ready;
            ctx_c.game.borrow_mut().reveal(x, y);
            if was_ready && ctx_c.game.borrow().state == GameState::Playing {
                start_timer(&ctx_c.start_time, &ctx_c.timer_source, &ctx_c.timer_label);
            }
            post_action(&ctx_c);
        });
    }

    // Right click - flag
    {
        let right = GestureClick::new();
        right.set_button(3);
        let ctx_c = ctx.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if *ctx_c.animating.borrow() { return; }
            if !ctx_c.game.borrow_mut().toggle_flag(x, y) { return; }
            post_action(&ctx_c);
        });
        btn.add_controller(right);
    }

    // Middle click - chord
    {
        let middle = GestureClick::new();
        middle.set_button(2);
        let ctx_c = ctx.clone();
        middle.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if *ctx_c.animating.borrow() { return; }
            if !ctx_c.game.borrow_mut().chord(x, y) { return; }
            post_action(&ctx_c);
        });
        btn.add_controller(middle);
    }

    btn
}
