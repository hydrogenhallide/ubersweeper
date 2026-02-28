use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Grid};
use rand::seq::SliceRandom;
use rand::Rng;
use rand::thread_rng;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::constants::CELL_SIZE;
use crate::game::{Game, GameState};
use super::{BoardContext, start_timer, update_face, update_mine_counter};

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum GroupState { Hidden, Flagged, Revealed }

struct MergeGroup {
    origin:   (usize, usize), // top-left cell (col, row)
    span_w:   usize,          // 1 or 2
    span_h:   usize,          // 1 or 2
    cells:    Vec<(usize, usize)>,
    mine:     bool,
    adjacent: usize,          // neighbouring mine-groups
    state:    GroupState,
}

struct MergeGame {
    width:        usize,
    height:       usize,
    group_map:    Vec<Vec<usize>>, // [y][x] → group index
    groups:       Vec<MergeGroup>,
    state:        GameState,
    mine_count:   usize,
    flags_placed: usize,
}

impl MergeGame {
    fn new(width: usize, height: usize, mine_count: usize) -> Self {
        let mut group_map = vec![vec![usize::MAX; width]; height];
        let mut groups: Vec<MergeGroup> = Vec::new();
        let mut rng = thread_rng();

        for y in 0..height {
            for x in 0..width {
                if group_map[y][x] != usize::MAX { continue; }

                let can_r   = x + 1 < width  && group_map[y][x + 1] == usize::MAX;
                let can_d   = y + 1 < height && group_map[y + 1][x] == usize::MAX;
                let can_2x2 = can_r && can_d  && group_map[y + 1][x + 1] == usize::MAX;

                let (sw, sh) = if can_2x2 && rng.gen_bool(0.03) {
                    (2, 2)
                } else if can_r && rng.gen_bool(0.05) {
                    (2, 1)
                } else if can_d && rng.gen_bool(0.05) {
                    (1, 2)
                } else {
                    (1, 1)
                };

                let gid = groups.len();
                let mut cells = Vec::new();
                for dy in 0..sh {
                    for dx in 0..sw {
                        cells.push((x + dx, y + dy));
                        group_map[y + dy][x + dx] = gid;
                    }
                }
                groups.push(MergeGroup {
                    origin: (x, y), span_w: sw, span_h: sh, cells,
                    mine: false, adjacent: 0, state: GroupState::Hidden,
                });
            }
        }

        let mc = mine_count.min(groups.len().saturating_sub(9));
        MergeGame {
            width, height, group_map, groups,
            state: GameState::Ready, mine_count: mc, flags_placed: 0,
        }
    }

    /// All unique neighbouring group indices for `gid`.
    fn neighbors(&self, gid: usize) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut out  = Vec::new();
        for &(cx, cy) in &self.groups[gid].cells {
            for dy in -1_isize..=1 {
                for dx in -1_isize..=1 {
                    let nx = cx as isize + dx;
                    let ny = cy as isize + dy;
                    if nx < 0 || nx >= self.width  as isize { continue; }
                    if ny < 0 || ny >= self.height as isize { continue; }
                    let ng = self.group_map[ny as usize][nx as usize];
                    if ng != gid && seen.insert(ng) { out.push(ng); }
                }
            }
        }
        out
    }

    fn place_mines(&mut self, first: usize) {
        let mut safe = HashSet::new();
        safe.insert(first);
        for ng in self.neighbors(first) { safe.insert(ng); }

        let mut cands: Vec<usize> = (0..self.groups.len())
            .filter(|g| !safe.contains(g)).collect();
        cands.shuffle(&mut thread_rng());

        let n = self.mine_count.min(cands.len());
        for &gid in &cands[..n] { self.groups[gid].mine = true; }

        for gid in 0..self.groups.len() {
            self.groups[gid].adjacent = self.neighbors(gid).iter()
                .filter(|&&ng| self.groups[ng].mine).count();
        }
    }

    fn reveal(&mut self, gid: usize) -> bool {
        if self.state == GameState::Ready {
            self.place_mines(gid);
            self.state = GameState::Playing;
        }
        if self.state != GameState::Playing { return false; }
        if self.groups[gid].state != GroupState::Hidden { return false; }

        self.groups[gid].state = GroupState::Revealed;

        if self.groups[gid].mine {
            self.state = GameState::Lost;
            for g in &mut self.groups {
                if g.mine && g.state == GroupState::Hidden { g.state = GroupState::Revealed; }
            }
            return true;
        }

        if self.groups[gid].adjacent == 0 { self.cascade(gid); }
        self.check_win();
        true
    }

    fn cascade(&mut self, start: usize) {
        let mut stack = vec![start];
        while let Some(gid) = stack.pop() {
            for ng in self.neighbors(gid) {
                let g = &self.groups[ng];
                if g.state == GroupState::Hidden && !g.mine {
                    self.groups[ng].state = GroupState::Revealed;
                    if self.groups[ng].adjacent == 0 { stack.push(ng); }
                }
            }
        }
    }

    fn toggle_flag(&mut self, gid: usize) -> bool {
        if self.state != GameState::Playing { return false; }
        match self.groups[gid].state {
            GroupState::Hidden   => { self.groups[gid].state = GroupState::Flagged;  self.flags_placed += 1; true }
            GroupState::Flagged  => { self.groups[gid].state = GroupState::Hidden;   self.flags_placed -= 1; true }
            GroupState::Revealed => false,
        }
    }

    fn check_win(&mut self) {
        if self.groups.iter().all(|g| g.mine || g.state == GroupState::Revealed) {
            self.state = GameState::Won;
        }
    }

    /// Chord: if `gid` is revealed and flag count around it equals its adjacent count,
    /// reveal all unflagged hidden neighbors. Returns true if anything changed.
    fn chord(&mut self, gid: usize) -> bool {
        if self.state != GameState::Playing { return false; }
        if self.groups[gid].state != GroupState::Revealed { return false; }
        if self.groups[gid].adjacent == 0 { return false; }

        let nbrs = self.neighbors(gid);
        let flags: usize = nbrs.iter().filter(|&&ng| self.groups[ng].state == GroupState::Flagged).count();
        if flags != self.groups[gid].adjacent { return false; }

        // Every flagged neighbour must actually be a mine group.
        for &ng in &nbrs {
            if self.groups[ng].state == GroupState::Flagged && !self.groups[ng].mine {
                return false;
            }
        }

        let mut changed = false;
        for ng in nbrs {
            if self.groups[ng].state == GroupState::Hidden {
                self.reveal(ng);
                changed = true;
            }
        }
        changed
    }

    fn sync_to(&self, game: &Rc<RefCell<Game>>) {
        let mut g = game.borrow_mut();
        g.state        = self.state;
        g.mine_count   = self.mine_count;
        g.flags_placed = self.flags_placed;
    }
}

// ── Board creation ─────────────────────────────────────────────────────────────

pub fn create_board(ctx: &BoardContext) -> gtk4::Widget {
    let (width, height, mine_count) = {
        let g = ctx.game.borrow();
        (g.width, g.height, g.mine_count)
    };

    let merge = Rc::new(RefCell::new(MergeGame::new(width, height, mine_count)));

    let grid = Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    grid.set_halign(gtk4::Align::Center);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);

    // One button per grid cell, all 1×1 initially.
    let buttons: Rc<Vec<Vec<Button>>> = Rc::new(
        (0..height).map(|y| {
            (0..width).map(|x| {
                let btn = Button::with_label(" ");
                btn.set_size_request(CELL_SIZE, CELL_SIZE);
                btn.add_css_class("cell");
                grid.attach(&btn, x as i32, y as i32, 1, 1);
                btn
            }).collect()
        }).collect()
    );

    // Tracks whether each group's grid span has been adjusted on reveal.
    let span_done: Rc<RefCell<Vec<bool>>> =
        Rc::new(RefCell::new(vec![false; merge.borrow().groups.len()]));

    for y in 0..height {
        for x in 0..width {
            // Left click - reveal
            {
                let merge_c  = merge.clone();
                let game_c   = ctx.game.clone();
                let mine_c   = ctx.mine_label.clone();
                let timer_c  = ctx.timer_label.clone();
                let face_c   = ctx.face_button.clone();
                let start_c  = ctx.start_time.clone();
                let source_c = ctx.timer_source.clone();
                let btns_c   = buttons.clone();
                let span_c   = span_done.clone();
                let grid_c   = grid.clone();
                buttons[y][x].connect_clicked(move |_| {
                    let gid = merge_c.borrow().group_map[y][x];
                    // Chord if already revealed
                    if merge_c.borrow().groups[gid].state == GroupState::Revealed {
                        if !merge_c.borrow_mut().chord(gid) { return; }
                        merge_c.borrow().sync_to(&game_c);
                        render_board(&merge_c.borrow(), &grid_c, &btns_c, &span_c);
                        update_mine_counter(&game_c, &mine_c);
                        update_face(&game_c, &face_c, &source_c);
                        return;
                    }
                    let was_ready = merge_c.borrow().state == GameState::Ready;
                    if !merge_c.borrow_mut().reveal(gid) { return; }
                    merge_c.borrow().sync_to(&game_c);
                    if was_ready && merge_c.borrow().state == GameState::Playing {
                        start_timer(&start_c, &source_c, &timer_c);
                    }
                    render_board(&merge_c.borrow(), &grid_c, &btns_c, &span_c);
                    update_mine_counter(&game_c, &mine_c);
                    update_face(&game_c, &face_c, &source_c);
                });
            }

            // Right click - flag
            {
                let right    = GestureClick::new();
                right.set_button(3);
                let merge_c  = merge.clone();
                let game_c   = ctx.game.clone();
                let mine_c   = ctx.mine_label.clone();
                let btns_c   = buttons.clone();
                let span_c   = span_done.clone();
                let grid_c   = grid.clone();
                right.connect_pressed(move |gesture, _, _, _| {
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                    let gid = merge_c.borrow().group_map[y][x];
                    if !merge_c.borrow_mut().toggle_flag(gid) { return; }
                    merge_c.borrow().sync_to(&game_c);
                    render_board(&merge_c.borrow(), &grid_c, &btns_c, &span_c);
                    update_mine_counter(&game_c, &mine_c);
                });
                buttons[y][x].add_controller(right);
            }
        }
    }

    grid.upcast()
}

#[allow(dead_code)]
pub fn update_board(_game: &Rc<RefCell<Game>>, _board: &gtk4::Widget) {
    // Merge manages its own rendering via captured closures.
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_board(
    merge:     &MergeGame,
    grid:      &Grid,
    buttons:   &Vec<Vec<Button>>,
    span_done: &Rc<RefCell<Vec<bool>>>,
) {
    let mut done = span_done.borrow_mut();

    for gid in 0..merge.groups.len() {
        let g   = &merge.groups[gid];
        let (ox, oy) = g.origin;
        let btn = &buttons[oy][ox];

        btn.remove_css_class("cell-revealed");
        btn.remove_css_class("cell-mine");
        for i in 1..=12usize { btn.remove_css_class(&format!("cell-{i}")); }

        let multi = g.span_w > 1 || g.span_h > 1;

        match g.state {
            GroupState::Hidden => {
                btn.set_label(" ");
                // Contract back if this group was previously expanded by a flag.
                if done[gid] && multi {
                    grid.remove(btn);
                    for &(cx, cy) in &g.cells {
                        grid.attach(&buttons[cy][cx], cx as i32, cy as i32, 1, 1);
                    }
                    done[gid] = false;
                }
            }
            GroupState::Flagged => {
                // Expand to reveal true dimensions - player learns the shape on flag.
                if !done[gid] && multi {
                    for &(cx, cy) in &g.cells {
                        grid.remove(&buttons[cy][cx]);
                    }
                    grid.attach(btn, ox as i32, oy as i32, g.span_w as i32, g.span_h as i32);
                    done[gid] = true;
                }
                btn.set_label("\u{1F6A9}");
            }
            GroupState::Revealed => {
                if !done[gid] {
                    for &(cx, cy) in &g.cells {
                        grid.remove(&buttons[cy][cx]);
                    }
                    grid.attach(btn, ox as i32, oy as i32, g.span_w as i32, g.span_h as i32);
                    done[gid] = true;
                }
                btn.add_css_class("cell-revealed");
                if g.mine {
                    btn.add_css_class("cell-mine");
                    btn.set_label("\u{1F4A3}");
                } else if g.adjacent > 0 {
                    btn.add_css_class(&format!("cell-{}", g.adjacent));
                    btn.set_label(&g.adjacent.to_string());
                } else {
                    btn.set_label(" ");
                }
            }
        }
    }
}
