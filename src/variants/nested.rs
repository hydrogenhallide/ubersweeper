// Nested: every hidden tile conceals a 3×3 mini-minesweeper.
// Win the mini-game → the outer tile is revealed (mine or safe).
// Lose the mini-game → outer board game-over.
// Right-click still flags without opening the mini-game.

use gtk4::{glib, prelude::*, Button, Grid};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use rand::Rng;

use crate::game::{CellState, Game, GameState};
use crate::constants::CELL_SIZE;
use super::{BoardContext, start_timer, stop_timer};

const INNER_SIZE:  usize = 5;
const INNER_MINES: usize = 5;
const INNER_CELL:  i32   = 40;

// ---------------------------------------------------------------------------
// Outer board (custom - no cascade, mines placed on first won mini-game)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Outer { Hidden, Flagged, Revealed }

#[derive(Clone)]
struct OuterCell { is_mine: bool, adjacent: u8, state: Outer }

impl OuterCell {
    fn new() -> Self { OuterCell { is_mine: false, adjacent: 0, state: Outer::Hidden } }
}

struct OuterBoard {
    w: usize, h: usize,
    grid:       Vec<Vec<OuterCell>>,
    state:      GameState,
    mine_count: usize,
    flags:      usize,
    revealed:   usize,
}

impl OuterBoard {
    fn new(w: usize, h: usize, mine_count: usize) -> Self {
        OuterBoard {
            w, h,
            grid: vec![vec![OuterCell::new(); w]; h],
            state: GameState::Ready,
            mine_count,
            flags: 0,
            revealed: 0,
        }
    }

    fn place_mines(&mut self, sx: usize, sy: usize) {
        let mut rng = rand::thread_rng();
        let mut placed = 0;
        while placed < self.mine_count {
            let x = rng.gen_range(0..self.w);
            let y = rng.gen_range(0..self.h);
            if self.grid[y][x].is_mine { continue; }
            if (x as i32 - sx as i32).abs() <= 1
            && (y as i32 - sy as i32).abs() <= 1 { continue; }
            self.grid[y][x].is_mine = true;
            placed += 1;
        }
        for y in 0..self.h { for x in 0..self.w {
            if !self.grid[y][x].is_mine {
                let mut n = 0u8;
                for dy in -1i32..=1 { for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = x as i32 + dx; let ny = y as i32 + dy;
                    if nx >= 0 && nx < self.w as i32 && ny >= 0 && ny < self.h as i32 {
                        if self.grid[ny as usize][nx as usize].is_mine { n += 1; }
                    }
                }}
                self.grid[y][x].adjacent = n;
            }
        }}
        self.state = GameState::Playing;
    }

    // Called when player wins a mini-game for cell (x, y).
    // If the cell is blank (adjacent == 0), cascades through connected blank/numbered cells.
    fn on_mini_won(&mut self, x: usize, y: usize) {
        if self.state == GameState::Ready { self.place_mines(x, y); }
        if self.state != GameState::Playing { return; }
        self.reveal_cell(x, y);
    }

    fn reveal_cell(&mut self, x: usize, y: usize) {
        if self.grid[y][x].state != Outer::Hidden { return; }
        self.grid[y][x].state = Outer::Revealed;
        if self.grid[y][x].is_mine {
            self.state = GameState::Lost;
            for gy in 0..self.h { for gx in 0..self.w {
                if self.grid[gy][gx].is_mine { self.grid[gy][gx].state = Outer::Revealed; }
            }}
        } else {
            self.revealed += 1;
            if self.revealed + self.mine_count == self.w * self.h {
                self.state = GameState::Won;
                return;
            }
            // Cascade: blank cells reveal their neighbours for free.
            if self.grid[y][x].adjacent == 0 {
                for dy in -1i32..=1 { for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = x as i32 + dx; let ny = y as i32 + dy;
                    if nx >= 0 && nx < self.w as i32 && ny >= 0 && ny < self.h as i32 {
                        self.reveal_cell(nx as usize, ny as usize);
                    }
                }}
            }
        }
    }

    // Called when player loses a mini-game.
    fn on_mini_lost(&mut self) {
        let was_playing = self.state == GameState::Playing;
        self.state = GameState::Lost;
        if was_playing {
            for gy in 0..self.h { for gx in 0..self.w {
                if self.grid[gy][gx].is_mine { self.grid[gy][gx].state = Outer::Revealed; }
            }}
        }
    }

    // Returns hidden neighbours to open if chord condition is met (flags == adjacent).
    fn chord_targets(&self, x: usize, y: usize) -> Vec<(usize, usize)> {
        if self.grid[y][x].state != Outer::Revealed || self.grid[y][x].adjacent == 0 {
            return Vec::new();
        }
        let mut flags = 0u8;
        let mut hidden = Vec::new();
        for dy in -1i32..=1 { for dx in -1i32..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = x as i32 + dx; let ny = y as i32 + dy;
            if nx >= 0 && nx < self.w as i32 && ny >= 0 && ny < self.h as i32 {
                match self.grid[ny as usize][nx as usize].state {
                    Outer::Flagged => flags += 1,
                    Outer::Hidden  => hidden.push((nx as usize, ny as usize)),
                    _              => {}
                }
            }
        }}
        if flags >= self.grid[y][x].adjacent { hidden } else { Vec::new() }
    }

    fn toggle_flag(&mut self, x: usize, y: usize) {
        match self.grid[y][x].state {
            Outer::Hidden  => { self.grid[y][x].state = Outer::Flagged; self.flags += 1; }
            Outer::Flagged => { self.grid[y][x].state = Outer::Hidden;
                                if self.flags > 0 { self.flags -= 1; } }
            _ => {}
        }
    }

    fn remaining_mines(&self) -> isize { self.mine_count as isize - self.flags as isize }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

fn render_outer(btn: &Button, cell: &OuterCell) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{}", i)); }
    match cell.state {
        Outer::Hidden   => { btn.set_label(" "); }
        Outer::Flagged  => { btn.set_label("\u{1F6A9}"); }
        Outer::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.is_mine {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else if cell.adjacent > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adjacent.min(8)));
                btn.set_label(&cell.adjacent.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}

fn redraw_outer(outer: &Rc<RefCell<OuterBoard>>, board: &gtk4::Widget) {
    let grid = match board.downcast_ref::<Grid>() { Some(g) => g, None => return };
    let ob = outer.borrow();
    for y in 0..ob.h { for x in 0..ob.w {
        if let Some(w) = grid.child_at(x as i32, y as i32) {
            if let Some(btn) = w.downcast_ref::<Button>() {
                render_outer(btn, &ob.grid[y][x]);
            }
        }
    }}
}

fn render_inner(btn: &Button, cell: &crate::game::Cell) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{}", i)); }
    match cell.state {
        CellState::Hidden => { btn.set_label(" "); }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3
        | CellState::FlaggedNegative => { btn.set_label("\u{1F6A9}"); }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if cell.mines > 0 {
                btn.add_css_class("cell-mine");
                btn.set_label("\u{1F4A3}");
            } else if cell.adjacent_mines > 0 {
                btn.add_css_class(&format!("cell-{}", cell.adjacent_mines.clamp(1, 8) as u8));
                btn.set_label(&cell.adjacent_mines.to_string());
            } else {
                btn.set_label(" ");
            }
        }
    }
}

fn redraw_inner(game: &Rc<RefCell<Game>>, board: &gtk4::Widget) {
    let grid = match board.downcast_ref::<Grid>() { Some(g) => g, None => return };
    let gr = game.borrow();
    for y in 0..INNER_SIZE { for x in 0..INNER_SIZE {
        if let Some(w) = grid.child_at(x as i32, y as i32) {
            if let Some(btn) = w.downcast_ref::<Button>() {
                render_inner(btn, &gr.grid[y][x]);
            }
        }
    }}
}

fn set_outer_mine_label(outer: &OuterBoard, lbl: &Rc<RefCell<Option<gtk4::Label>>>) {
    if let Some(l) = lbl.borrow().as_ref() {
        l.set_text(&format!("{:03}", outer.remaining_mines()));
    }
}

fn set_outer_face(
    state: GameState,
    face:  &Rc<RefCell<Option<Button>>>,
    src:   &Rc<RefCell<Option<glib::SourceId>>>,
) {
    if let Some(btn) = face.borrow().as_ref() {
        match state {
            GameState::Won  => { btn.set_label("\u{1F60E}"); stop_timer(src); }
            GameState::Lost => { btn.set_label("\u{1F635}"); stop_timer(src); }
            _               => { btn.set_label("\u{1F642}"); }
        }
    }
}

// ---------------------------------------------------------------------------
// Mini-game dialog
// ---------------------------------------------------------------------------

fn open_mini_game(
    parent:    Option<gtk4::Window>,
    inner:     Rc<RefCell<Game>>,
    on_end:    impl Fn(bool) + 'static,   // true = won, false = lost
) {
    // Don't re-open a finished game.
    if !matches!(inner.borrow().state, GameState::Ready | GameState::Playing) { return; }

    let win = gtk4::Window::new();
    if let Some(p) = &parent { win.set_transient_for(Some(p)); }
    win.set_modal(true);
    win.set_decorated(false);
    win.set_resizable(false);

    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_margin_top(8); grid.set_margin_bottom(8);
    grid.set_margin_start(8); grid.set_margin_end(8);

    let done    = Rc::new(Cell::new(false));
    let on_end  = Rc::new(on_end);
    let bw: Rc<RefCell<Option<gtk4::Widget>>> = Rc::new(RefCell::new(None));

    for gy in 0..INNER_SIZE { for gx in 0..INNER_SIZE {
        let btn = Button::with_label(" ");
        btn.set_size_request(INNER_CELL, INNER_CELL);
        btn.add_css_class("cell");

        // Left click - reveal / chord.
        {
            let ig = inner.clone(); let bw = bw.clone();
            let done = done.clone(); let on_end = on_end.clone(); let win_c = win.clone();
            btn.connect_clicked(move |_| {
                if done.get() { return; }
                if !matches!(ig.borrow().state, GameState::Ready | GameState::Playing) { return; }
                {
                    let state = ig.borrow().grid[gy][gx].state;
                    if state == CellState::Revealed {
                        ig.borrow_mut().chord(gx, gy);
                    } else {
                        ig.borrow_mut().reveal(gx, gy);
                    }
                }
                if let Some(b) = bw.borrow().as_ref() { redraw_inner(&ig, b); }
                match ig.borrow().state {
                    GameState::Won => {
                        done.set(true);
                        // Brief pause so the player can see the solved board.
                        let on_end = on_end.clone(); let win_c = win_c.clone();
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(350),
                            move || { on_end(true); win_c.close(); }
                        );
                    }
                    GameState::Lost => {
                        done.set(true);
                        // Show the exploded mine briefly.
                        let on_end = on_end.clone(); let win_c = win_c.clone();
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(500),
                            move || { on_end(false); win_c.close(); }
                        );
                    }
                    _ => {}
                }
            });
        }

        // Right click - flag.
        {
            let ig = inner.clone(); let bw = bw.clone();
            let right = gtk4::GestureClick::new();
            right.set_button(3);
            right.connect_pressed(move |g, _, _, _| {
                g.set_state(gtk4::EventSequenceState::Claimed);
                if !matches!(ig.borrow().state, GameState::Ready | GameState::Playing) { return; }
                ig.borrow_mut().toggle_flag(gx, gy);
                if let Some(b) = bw.borrow().as_ref() { redraw_inner(&ig, b); }
            });
            btn.add_controller(right);
        }

        // Middle click - chord.
        {
            let ig = inner.clone(); let bw = bw.clone();
            let done = done.clone(); let on_end = on_end.clone(); let win_c = win.clone();
            let mid = gtk4::GestureClick::new();
            mid.set_button(2);
            mid.connect_pressed(move |g, _, _, _| {
                g.set_state(gtk4::EventSequenceState::Claimed);
                if done.get() { return; }
                if ig.borrow().state != GameState::Playing { return; }
                ig.borrow_mut().chord(gx, gy);
                if let Some(b) = bw.borrow().as_ref() { redraw_inner(&ig, b); }
                match ig.borrow().state {
                    GameState::Won => {
                        done.set(true);
                        let on_end = on_end.clone(); let win_c = win_c.clone();
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(350),
                            move || { on_end(true); win_c.close(); }
                        );
                    }
                    GameState::Lost => {
                        done.set(true);
                        let on_end = on_end.clone(); let win_c = win_c.clone();
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(500),
                            move || { on_end(false); win_c.close(); }
                        );
                    }
                    _ => {}
                }
            });
            btn.add_controller(mid);
        }

        grid.attach(&btn, gx as i32, gy as i32, 1, 1);
    }}

    *bw.borrow_mut() = Some(grid.clone().upcast());
    // Render initial state (inner game may be partially played from a previous opening).
    redraw_inner(&inner, bw.borrow().as_ref().unwrap());

    win.set_child(Some(&grid));
    win.present();
}

// ---------------------------------------------------------------------------
// Sequential mini-game queue (for chord: opens one window at a time)
// ---------------------------------------------------------------------------

fn open_sequence(
    queue:  Rc<RefCell<VecDeque<(usize, usize)>>>,
    parent: Option<gtk4::Window>,
    ig:     Rc<Vec<Vec<Rc<RefCell<Game>>>>>,
    outer:  Rc<RefCell<OuterBoard>>,
    bw:     Rc<RefCell<Option<gtk4::Widget>>>,
    ml:     Rc<RefCell<Option<gtk4::Label>>>,
    fb:     Rc<RefCell<Option<Button>>>,
    st:     Rc<RefCell<Option<std::time::Instant>>>,
    sr:     Rc<RefCell<Option<glib::SourceId>>>,
    tl:     Rc<RefCell<Option<gtk4::Label>>>,
) {
    loop {
        if !matches!(outer.borrow().state, GameState::Ready | GameState::Playing) { return; }
        let Some((x, y)) = queue.borrow_mut().pop_front() else { return };
        // Cell may have been cascade-revealed already; skip it.
        if outer.borrow().grid[y][x].state != Outer::Hidden { continue; }

        let was_ready  = outer.borrow().state == GameState::Ready;
        let inner_game = ig[y][x].clone();

        let (q, p, i, o, b, m, f, s, r, t) = (
            queue.clone(), parent.clone(), ig.clone(), outer.clone(),
            bw.clone(), ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone(),
        );
        open_mini_game(parent.clone(), inner_game, move |won| {
            if won {
                o.borrow_mut().on_mini_won(x, y);
                if was_ready && o.borrow().state == GameState::Playing {
                    start_timer(&s, &r, &t);
                }
            } else {
                o.borrow_mut().on_mini_lost();
            }
            let state = o.borrow().state;
            if let Some(board) = b.borrow().as_ref() { redraw_outer(&o, board); }
            set_outer_mine_label(&o.borrow(), &m);
            set_outer_face(state, &f, &r);
            // Clone before passing - on_end is Fn, not FnOnce.
            open_sequence(
                q.clone(), p.clone(), i.clone(), o.clone(),
                b.clone(), m.clone(), f.clone(), s.clone(), r.clone(), t.clone(),
            );
        });
        return; // wait for this mini-game before opening the next
    }
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let outer = Rc::new(RefCell::new(OuterBoard::new(width, height, mine_count)));

    // One persistent inner Game per outer cell (created upfront, state carries over).
    let inner_games: Rc<Vec<Vec<Rc<RefCell<Game>>>>> = Rc::new(
        (0..height).map(|_| (0..width).map(|_|
            Rc::new(RefCell::new(Game::new(
                INNER_SIZE, INNER_SIZE, INNER_MINES,
                super::Variant::Classic,
            )))
        ).collect()).collect()
    );

    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    let board_widget: Rc<RefCell<Option<gtk4::Widget>>> = Rc::new(RefCell::new(None));

    let ml = ctx.mine_label.clone();
    let fb = ctx.face_button.clone();
    let st = ctx.start_time.clone();
    let sr = ctx.timer_source.clone();
    let tl = ctx.timer_label.clone();

    set_outer_mine_label(&outer.borrow(), &ml);

    for y in 0..height { for x in 0..width {
        let btn = Button::with_label(" ");
        btn.set_size_request(CELL_SIZE, CELL_SIZE);
        btn.set_hexpand(false); btn.set_vexpand(false);
        btn.set_halign(gtk4::Align::Center); btn.set_valign(gtk4::Align::Center);
        btn.add_css_class("cell");

        // Left click - open mini-game (Hidden) or chord sequence (Revealed).
        {
            let outer_c = outer.clone();
            let ig      = inner_games.clone();
            let bw      = board_widget.clone();
            let (ml_c, fb_c, st_c, sr_c, tl_c) =
                (ml.clone(), fb.clone(), st.clone(), sr.clone(), tl.clone());

            btn.connect_clicked(move |btn| {
                let ob = outer_c.borrow();
                if !matches!(ob.state, GameState::Ready | GameState::Playing) { return; }

                // Chord: revealed cell → queue mini-games for each hidden neighbour.
                if ob.grid[y][x].state == Outer::Revealed {
                    let targets = ob.chord_targets(x, y);
                    drop(ob);
                    if targets.is_empty() { return; }
                    let queue  = Rc::new(RefCell::new(targets.into_iter().collect::<VecDeque<_>>()));
                    let parent = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                    open_sequence(queue, parent,
                        ig.clone(), outer_c.clone(), bw.clone(),
                        ml_c.clone(), fb_c.clone(), st_c.clone(), sr_c.clone(), tl_c.clone());
                    return;
                }

                if ob.grid[y][x].state != Outer::Hidden { return; }
                drop(ob);

                let was_ready  = outer_c.borrow().state == GameState::Ready;
                let parent_win = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                let inner_game = ig[y][x].clone();

                let (o, b, m, f, s, r, t) = (
                    outer_c.clone(), bw.clone(),
                    ml_c.clone(), fb_c.clone(), st_c.clone(), sr_c.clone(), tl_c.clone(),
                );
                open_mini_game(parent_win, inner_game, move |won| {
                    if won {
                        o.borrow_mut().on_mini_won(x, y);
                        if was_ready && o.borrow().state == GameState::Playing {
                            start_timer(&s, &r, &t);
                        }
                    } else {
                        o.borrow_mut().on_mini_lost();
                    }
                    let state = o.borrow().state;
                    if let Some(board) = b.borrow().as_ref() { redraw_outer(&o, board); }
                    set_outer_mine_label(&o.borrow(), &m);
                    set_outer_face(state, &f, &r);
                });
            });
        }

        // Right click - flag (no mini-game).
        {
            let outer_c = outer.clone();
            let bw      = board_widget.clone();
            let ml_c    = ml.clone();
            let right   = gtk4::GestureClick::new();
            right.set_button(3);
            right.connect_pressed(move |g, _, _, _| {
                g.set_state(gtk4::EventSequenceState::Claimed);
                if !matches!(outer_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
                outer_c.borrow_mut().toggle_flag(x, y);
                if let Some(b) = bw.borrow().as_ref() { redraw_outer(&outer_c, b); }
                set_outer_mine_label(&outer_c.borrow(), &ml_c);
            });
            btn.add_controller(right);
        }

        grid.attach(&btn, x as i32, y as i32, 1, 1);
    }}

    let widget = grid.upcast::<gtk4::Widget>();
    *board_widget.borrow_mut() = Some(widget.clone());
    widget
}

pub fn update_board(_game: &Rc<RefCell<crate::game::Game>>, _board: &gtk4::Widget) {
    // Nested manages its own state; standard game resets are handled at create_board time.
}
