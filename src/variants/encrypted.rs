use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Entry, GestureClick, Grid, Label, Orientation};
use gtk4::glib;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use rand::Rng;

use crate::game::{CellState, Game, GameState};
use crate::constants::CELL_SIZE;
use super::{BoardContext, start_timer, update_face, update_mine_counter};

// ---------------------------------------------------------------------------
// Equation generation
// ---------------------------------------------------------------------------

struct Equation {
    display: String,
    answer:  i64,
}

impl Equation {
    fn generate(level: u8, rng: &mut impl Rng) -> Self {
        match level {
            1 => Self::arithmetic(rng),
            2 => Self::arithmetic_chain(rng),
            3 => Self::linear(rng),
            4 => Self::quadratic(rng),
            5 => Self::series(rng),
            6 => Self::modular_exp(rng),
            7 => Self::derivative(rng),
            _ => Self::integral(rng),
        }
    }

    // Level 1 - basic arithmetic
    fn arithmetic(rng: &mut impl Rng) -> Self {
        let a = rng.gen_range(2i64..=50);
        let b = rng.gen_range(2i64..=50);
        match rng.gen_range(0u8..3) {
            0 => Self { display: format!("{} + {} = ?", a, b),             answer: a + b },
            1 => Self { display: format!("{} − {} = ?", a.max(b), a.min(b)), answer: (a - b).abs() },
            _ => {
                let x = rng.gen_range(2i64..=12);
                let y = rng.gen_range(2i64..=12);
                Self { display: format!("{} × {} = ?", x, y), answer: x * y }
            }
        }
    }

    // Level 2 - chained arithmetic and squares (results capped ~50)
    fn arithmetic_chain(rng: &mut impl Rng) -> Self {
        match rng.gen_range(0u8..4) {
            0 => {
                // a + b × c  →  max 8 + 6×6 = 44
                let a = rng.gen_range(1i64..=8);
                let b = rng.gen_range(2i64..=6);
                let c = rng.gen_range(2i64..=6);
                Self { display: format!("{} + {} × {} = ?", a, b, c), answer: a + b * c }
            }
            1 => {
                // (a + b) × c  →  max (6+6)×4 = 48
                let a = rng.gen_range(1i64..=6);
                let b = rng.gen_range(1i64..=6);
                let c = rng.gen_range(2i64..=4);
                Self { display: format!("({} + {}) × {} = ?", a, b, c), answer: (a + b) * c }
            }
            2 => {
                // a × b − c  →  max 7×7−1 = 48, always positive
                let a = rng.gen_range(3i64..=7);
                let b = rng.gen_range(3i64..=7);
                let c = rng.gen_range(1i64..=(a * b / 3).max(1));
                Self { display: format!("{} × {} − {} = ?", a, b, c), answer: a * b - c }
            }
            _ => {
                // n² + a  →  max 6²+8 = 44
                let n = rng.gen_range(2i64..=6);
                let a = rng.gen_range(1i64..=8);
                Self { display: format!("{}² + {} = ?", n, a), answer: n * n + a }
            }
        }
    }

    // Level 3 - linear equation  ax + b = c,  solve for x
    fn linear(rng: &mut impl Rng) -> Self {
        let x = rng.gen_range(-12i64..=12);
        let a = rng.gen_range(2i64..=9);
        let b = rng.gen_range(-20i64..=20);
        let c = a * x + b;
        let display = if b >= 0 {
            format!("{}x + {} = {}   [x = ?]", a, b, c)
        } else {
            format!("{}x − {} = {}   [x = ?]", a, b.abs(), c)
        };
        Self { display, answer: x }
    }

    // Level 4 - quadratic  (x − r1)(x − r2) = 0,  report the larger root
    fn quadratic(rng: &mut impl Rng) -> Self {
        let r1 = rng.gen_range(-8i64..=8);
        let r2 = rng.gen_range(-8i64..=8);
        let b  = -(r1 + r2);
        let c  = r1 * r2;
        let display = match (b >= 0, c >= 0) {
            (true,  true)  => format!("x² + {}x + {} = 0   [larger x = ?]", b,    c),
            (true,  false) => format!("x² + {}x − {} = 0   [larger x = ?]", b,    c.abs()),
            (false, true)  => format!("x² − {}x + {} = 0   [larger x = ?]", b.abs(), c),
            (false, false) => format!("x² − {}x − {} = 0   [larger x = ?]", b.abs(), c.abs()),
        };
        Self { display, answer: r1.max(r2) }
    }

    // Level 5 - series: triangular, sum-of-squares, geometric
    fn series(rng: &mut impl Rng) -> Self {
        match rng.gen_range(0u8..3) {
            0 => {
                let n = rng.gen_range(5i64..=30);
                Self {
                    display: format!("∑ᵢ₌₁^{} i = ?", n),
                    answer:  n * (n + 1) / 2,
                }
            }
            1 => {
                let n = rng.gen_range(3i64..=15);
                Self {
                    display: format!("∑ᵢ₌₁^{} i² = ?", n),
                    answer:  n * (n + 1) * (2 * n + 1) / 6,
                }
            }
            _ => {
                let a = rng.gen_range(1i64..=4);
                let n = rng.gen_range(3i64..=6);
                let sum = a * ((1i64 << n) - 1);
                Self {
                    display: format!("{} + {} + {} + …  ({} terms, r=2) = ?", a, a*2, a*4, n),
                    answer:  sum,
                }
            }
        }
    }

    // Level 6 - modular exponentiation  a^b mod p
    fn modular_exp(rng: &mut impl Rng) -> Self {
        const PRIMES: [i64; 6] = [7, 11, 13, 17, 19, 23];
        let p = PRIMES[rng.gen_range(0..PRIMES.len())];
        let a = rng.gen_range(2i64..p);
        let b = rng.gen_range(2u32..=6);
        let answer = (0..b).fold(1i64, |acc, _| (acc * a) % p);
        Self {
            display: format!("{}^{} mod {} = ?", a, b, p),
            answer,
        }
    }

    // Level 7 - derivative of  ax^n + bx  evaluated at x = k
    fn derivative(rng: &mut impl Rng) -> Self {
        let a = rng.gen_range(1i64..=5);
        let n = rng.gen_range(2u32..=3);
        let b = rng.gen_range(1i64..=8);
        let k = rng.gen_range(1i64..=4);
        // d/dx = a·n·x^(n-1) + b
        let answer = a * n as i64 * k.pow(n - 1) + b;
        Self {
            display: format!("d/dx [{}x^{} + {}x] at x={} = ?", a, n, b, k),
            answer,
        }
    }

    // Level 8 - definite integral  ∫₀ⁿ (ax + b) dx  (integer result guaranteed)
    fn integral(rng: &mut impl Rng) -> Self {
        let n = rng.gen_range(2i64..=10);
        let a = rng.gen_range(1i64..=6) * 2; // even → integer result
        let b = rng.gen_range(1i64..=10);
        let answer = a * n * n / 2 + b * n;
        Self {
            display: format!("∫₀^{} ({}x + {}) dx = ?", n, a, b),
            answer,
        }
    }
}

fn equation_level(width: usize, height: usize, mine_count: usize) -> u8 {
    let density = mine_count as f64 / (width * height) as f64;
    let score   = density * 100.0 + ((width * height) as f64).log2();
    match score as u32 {
        0..=14 => 1,
        15..=21 => 2,
        22..=25 => 3,
        26..=29 => 4,
        30..=33 => 5,
        34..=37 => 6,
        38..=41 => 7,
        _       => 8,
    }
}

// ---------------------------------------------------------------------------
// Encrypted game state
// ---------------------------------------------------------------------------

struct EncState {
    level:        u8,
    peek_token:   bool,
    peeked:       Option<(usize, usize)>,
    chord_fails:  Vec<Vec<u8>>,          // failed chord attempts per cell
    locked:       Vec<Vec<bool>>,         // cells locked out of chording
}

impl EncState {
    fn new(level: u8, w: usize, h: usize) -> Self {
        EncState {
            level,
            peek_token: false,
            peeked: None,
            chord_fails: vec![vec![0u8; w]; h],
            locked: vec![vec![false; w]; h],
        }
    }
    fn on_move(&mut self) {
        self.peeked = None;
    }
    fn lock_area(&mut self, x: usize, y: usize, w: usize, h: usize) {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                    self.locked[ny as usize][nx as usize] = true;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Random unicode symbol pool
// ---------------------------------------------------------------------------

const SYMBOLS: &[char] = &[
    '∑','∫','∂','∇','∏','√','∞','∝','∀','∃','∈','∉','⊂','⊃','∪','∩',
    'α','β','γ','δ','ε','ζ','η','θ','κ','λ','μ','ν','ξ','π','ρ','σ','τ','φ','χ','ψ',
    '★','☆','♦','♣','♠','♥','✦','❋','⌈','⌉','⌊','⌋','±','×','÷','→','←','↑','↓',
    '⊕','⊗','⊙','⊞','⊟','Ω','Φ','Ψ','Λ','Δ','Σ','Π','Γ','Θ','Ξ','≠','≈','≤','≥','`','~','-','_','=','/','}','{','[',']',';',':','|','?','.',',','$','&','#','@',
];

fn random_symbol(rng: &mut impl Rng) -> char {
    SYMBOLS[rng.gen_range(0..SYMBOLS.len())]
}

// ---------------------------------------------------------------------------
// Board creation
// ---------------------------------------------------------------------------

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (w, h, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let level  = equation_level(w, h, mine_count);
    let state  = Rc::new(RefCell::new(EncState::new(level, w, h)));
    let equation: Rc<RefCell<Equation>> = Rc::new(RefCell::new(
        Equation::generate(level, &mut rand::thread_rng()),
    ));

    // Per-cell random symbols
    let symbols: Rc<RefCell<Vec<Vec<char>>>> = Rc::new(RefCell::new({
        let mut rng = rand::thread_rng();
        (0..h).map(|_| (0..w).map(|_| random_symbol(&mut rng)).collect()).collect()
    }));

    // ── outer layout: equation row + grid ──────────────────────────────────
    let outer = GtkBox::new(Orientation::Vertical, 8);

    // Equation label
    let eq_label = Label::new(Some(&equation.borrow().display));
    eq_label.add_css_class("enc-equation");
    eq_label.set_wrap(true);
    eq_label.set_halign(gtk4::Align::Center);
    outer.append(&eq_label);

    // Answer entry + peek-token indicator
    let answer_row = GtkBox::new(Orientation::Horizontal, 6);
    answer_row.set_halign(gtk4::Align::Center);

    let eq_entry = Entry::new();
    eq_entry.set_width_chars(10);
    eq_entry.set_placeholder_text(Some("answer…"));

    let peek_label = Label::new(Some("✓"));
    peek_label.add_css_class("enc-peek");
    peek_label.set_visible(false);

    answer_row.append(&eq_entry);
    answer_row.append(&peek_label);
    outer.append(&answer_row);

    // ── grid ───────────────────────────────────────────────────────────────
    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    for y in 0..h {
        for x in 0..w {
            let btn = make_cell_button(x, y, ctx, &grid, state.clone(), symbols.clone(), peek_label.clone());
            render_cell(&btn, crate::game::CellState::Hidden, ' ', false, false);
            grid.attach(&btn, x as i32, y as i32, 1, 1);
            schedule_cell_flash(x, y, btn, grid.clone(), ctx.game.clone(), symbols.clone(), state.clone());
        }
    }
    outer.append(&grid);

    // ── Entry submit ────────────────────────────────────────────────────────
    {
        let state_c      = state.clone();
        let eq_c         = equation.clone();
        let eq_label_c   = eq_label.clone();
        let peek_label_c = peek_label.clone();
        let start_c      = ctx.start_time.clone();

        eq_entry.connect_activate(move |entry| {
            let text = entry.text();
            entry.set_text("");

            let Ok(given) = text.trim().parse::<i64>() else { return; };

            if given == eq_c.borrow().answer {
                // Correct - grant peek token, show checkmark, generate new equation.
                state_c.borrow_mut().peek_token = true;
                peek_label_c.set_visible(true);
                let mut rng = rand::thread_rng();
                let lvl = state_c.borrow().level;
                *eq_c.borrow_mut() = Equation::generate(lvl, &mut rng);
                eq_label_c.set_text(&eq_c.borrow().display);
                eq_label_c.remove_css_class("enc-wrong");
            } else {
                // Wrong - 20-second penalty.
                apply_penalty(&start_c);
                // Show error in label for 2s then restore.
                eq_label_c.add_css_class("enc-wrong");
                eq_label_c.set_text("✗  Wrong - +20s penalty");
                let eq_c2       = eq_c.clone();
                let eq_label_c2 = eq_label_c.clone();
                glib::timeout_add_local_once(Duration::from_secs(2), move || {
                    eq_label_c2.remove_css_class("enc-wrong");
                    eq_label_c2.set_text(&eq_c2.borrow().display);
                });
            }
        });
    }

    outer.upcast()
}

// ---------------------------------------------------------------------------
// Flash scheduler - one independent timer per cell, each with its own random
// interval so cells update at different times rather than all at once.
// ---------------------------------------------------------------------------

fn schedule_cell_flash(
    x:       usize,
    y:       usize,
    btn:     Button,
    grid:    Grid,
    game:    Rc<RefCell<Game>>,
    symbols: Rc<RefCell<Vec<Vec<char>>>>,
    state:   Rc<RefCell<EncState>>,
) {
    let delay = rand::thread_rng().gen_range(50u64..=100);
    glib::timeout_add_local(Duration::from_millis(delay), move || {
        // Stop when the grid has been removed from the widget tree.
        if grid.parent().is_none() {
            return glib::ControlFlow::Break;
        }

        // Gather cell info (drop borrows before touching symbols).
        let (cell_state, cell_mines, peeked, locked) = {
            let g  = game.borrow();
            let st = state.borrow();
            let cell = &g.grid[y][x];
            (cell.state, cell.mines, st.peeked == Some((x, y)), st.locked[y][x])
        };

        // Randomise this cell's symbol only if it should flash.
        let symbol = if cell_state == CellState::Revealed && cell_mines == 0 && !peeked {
            let new_sym = random_symbol(&mut rand::thread_rng());
            symbols.borrow_mut()[y][x] = new_sym;
            new_sym
        } else {
            symbols.borrow()[y][x]
        };

        // Re-render just this button.
        render_cell(&btn, cell_state, symbol, peeked, locked);
        if cell_state == CellState::Revealed && (peeked || cell_mines > 0) {
            let g = game.borrow();
            render_true(&btn, &g.grid[y][x]);
        }

        // Re-schedule at a fresh random interval.
        schedule_cell_flash(x, y, btn.clone(), grid.clone(), game.clone(), symbols.clone(), state.clone());
        glib::ControlFlow::Break
    });
}

// ---------------------------------------------------------------------------
// Cell button
// ---------------------------------------------------------------------------

fn make_cell_button(
    x:          usize,
    y:          usize,
    ctx:        &BoardContext,
    grid:       &Grid,
    state:      Rc<RefCell<EncState>>,
    symbols:    Rc<RefCell<Vec<Vec<char>>>>,
    peek_label: Label,
) -> Button {
    let button = Button::with_label(" ");
    button.set_size_request(CELL_SIZE, CELL_SIZE);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_halign(gtk4::Align::Center);
    button.set_valign(gtk4::Align::Center);
    button.add_css_class("cell");

    // Left click - reveal / peek / chord
    {
        let game_c   = ctx.game.clone();
        let mine_c   = ctx.mine_label.clone();
        let timer_c  = ctx.timer_label.clone();
        let face_c   = ctx.face_button.clone();
        let start_c  = ctx.start_time.clone();
        let source_c = ctx.timer_source.clone();
        let state_c      = state.clone();
        let syms_c       = symbols.clone();
        let grid_c       = grid.clone();
        let peek_label_c = peek_label.clone();

        button.connect_clicked(move |_| {
            {
                let g = game_c.borrow();
                if !matches!(g.state, GameState::Ready | GameState::Playing) { return; }

                if g.grid[y][x].state == CellState::Revealed {
                    drop(g);
                    if state_c.borrow().peek_token {
                        // Use peek token: show true value and unlock this cell.
                        {
                            let mut st = state_c.borrow_mut();
                            st.peek_token = false;
                            st.peeked = Some((x, y));
                            st.locked[y][x] = false;
                        }
                        peek_label_c.set_visible(false);
                        do_render(&grid_c, &game_c, &syms_c, &state_c);
                        return;
                    }
                    // Locked cells cannot be chorded.
                    if state_c.borrow().locked[y][x] { return; }
                    // Chord - track failures and lock the area on the 2nd miss.
                    if !game_c.borrow_mut().chord(x, y) {
                        let (w, h) = { let g = game_c.borrow(); (g.width, g.height) };
                        let mut st = state_c.borrow_mut();
                        st.chord_fails[y][x] += 1;
                        if st.chord_fails[y][x] >= 2 {
                            st.lock_area(x, y, w, h);
                        }
                        drop(st);
                        do_render(&grid_c, &game_c, &syms_c, &state_c);
                        return;
                    }
                    state_c.borrow_mut().on_move();
                    do_render(&grid_c, &game_c, &syms_c, &state_c);
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
            state_c.borrow_mut().on_move();
            do_render(&grid_c, &game_c, &syms_c, &state_c);
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
    }

    // Right click - flag (unlimited; no equation required, does NOT use peek)
    {
        let right = GestureClick::new();
        right.set_button(3);
        let game_c   = ctx.game.clone();
        let mine_c   = ctx.mine_label.clone();
        let state_c  = state.clone();
        let syms_c   = symbols.clone();
        let grid_c   = grid.clone();
        right.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Ready | GameState::Playing) { return; }
            game_c.borrow_mut().toggle_flag(x, y);
            state_c.borrow_mut().on_move();
            do_render(&grid_c, &game_c, &syms_c, &state_c);
            update_mine_counter(&game_c, &mine_c);
        });
        button.add_controller(right);
    }

    // Middle click - chord
    {
        let mid = GestureClick::new();
        mid.set_button(2);
        let game_c   = ctx.game.clone();
        let mine_c   = ctx.mine_label.clone();
        let face_c   = ctx.face_button.clone();
        let source_c = ctx.timer_source.clone();
        let state_c  = state.clone();
        let syms_c   = symbols.clone();
        let grid_c   = grid.clone();
        mid.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if !matches!(game_c.borrow().state, GameState::Playing) { return; }
            if state_c.borrow().locked[y][x] { return; }
            if !game_c.borrow_mut().chord(x, y) {
                let (w, h) = { let g = game_c.borrow(); (g.width, g.height) };
                let mut st = state_c.borrow_mut();
                st.chord_fails[y][x] += 1;
                if st.chord_fails[y][x] >= 2 {
                    st.lock_area(x, y, w, h);
                }
                drop(st);
                do_render(&grid_c, &game_c, &syms_c, &state_c);
                return;
            }
            state_c.borrow_mut().on_move();
            do_render(&grid_c, &game_c, &syms_c, &state_c);
            update_mine_counter(&game_c, &mine_c);
            update_face(&game_c, &face_c, &source_c);
        });
        button.add_controller(mid);
    }

    button
}

// ---------------------------------------------------------------------------
// Penalty: add 20 s to the running timer by moving start_time back.
// ---------------------------------------------------------------------------

fn apply_penalty(start_time: &Rc<RefCell<Option<std::time::Instant>>>) {
    let st = *start_time.borrow();
    if let Some(start) = st {
        let elapsed = start.elapsed() + Duration::from_secs(20);
        // Compute a new start_time that is `elapsed` seconds before now.
        // checked_sub guards against the (practically impossible) case where
        // elapsed exceeds the monotonic clock's range.
        if let Some(new_start) = std::time::Instant::now().checked_sub(elapsed) {
            *start_time.borrow_mut() = Some(new_start);
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn do_render(
    grid:    &Grid,
    game:    &Rc<RefCell<Game>>,
    symbols: &Rc<RefCell<Vec<Vec<char>>>>,
    state:   &Rc<RefCell<EncState>>,
) {
    let g   = game.borrow();
    let sym = symbols.borrow();
    let st  = state.borrow();
    for y in 0..g.height {
        for x in 0..g.width {
            if let Some(w) = grid.child_at(x as i32, y as i32) {
                if let Some(btn) = w.downcast_ref::<Button>() {
                    let cell   = &g.grid[y][x];
                    let peeked = st.peeked == Some((x, y));
                    let locked = st.locked[y][x];
                    render_cell(btn, cell.state, sym[y][x], peeked, locked);
                    if cell.state == CellState::Revealed && (peeked || cell.mines > 0) {
                        render_true(btn, cell);
                    }
                }
            }
        }
    }
}

fn render_cell(btn: &Button, state: CellState, symbol: char, peeked: bool, locked: bool) {
    btn.remove_css_class("cell-revealed");
    btn.remove_css_class("cell-mine");
    btn.remove_css_class("cell-encrypted");
    btn.remove_css_class("cell-locked");
    for i in 1u8..=8 { btn.remove_css_class(&format!("cell-{}", i)); }

    match state {
        CellState::Hidden => { btn.set_label(" "); }
        CellState::Flagged | CellState::Flagged2 | CellState::Flagged3
        | CellState::FlaggedNegative => { btn.set_label("\u{1F6A9}"); }
        CellState::Revealed => {
            btn.add_css_class("cell-revealed");
            if locked { btn.add_css_class("cell-locked"); }
            if peeked {
                // render_true will fill in the real content
            } else {
                btn.add_css_class("cell-encrypted");
                btn.set_label(&symbol.to_string());
            }
        }
    }
}

/// Fill in the true number / mine label (called on top of render_cell for peeked/mine cells).
fn render_true(btn: &Button, cell: &crate::game::Cell) {
    btn.remove_css_class("cell-encrypted");
    if cell.mines > 0 {
        btn.add_css_class("cell-mine");
        btn.set_label("\u{1F4A3}");
    } else if cell.adjacent_mines > 0 {
        let n = (cell.adjacent_mines as u8).clamp(1, 8);
        btn.add_css_class(&format!("cell-{}", n));
        btn.set_label(&cell.adjacent_mines.to_string());
    } else {
        btn.set_label(" ");
    }
}

pub fn update_board(_game: &Rc<RefCell<Game>>, _board: &gtk4::Widget) {
    // Rendering is driven entirely by the flash timer and click handlers.
}
