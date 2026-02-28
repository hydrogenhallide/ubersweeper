use std::collections::HashSet;
use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;

use crate::variants::{Variant, negative_mines};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellState {
    Hidden,
    Revealed,
    Flagged,
    Flagged2,
    Flagged3,
    FlaggedNegative,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub state: CellState,
    /// Number of mines stacked on this cell (0 = no mine, 1–3 = mined).
    pub mines: u8,
    pub is_negative: bool,
    pub adjacent_mines: i8,
}

impl Cell {
    pub fn new() -> Self {
        Cell {
            state: CellState::Hidden,
            mines: 0,
            is_negative: false,
            adjacent_mines: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameState {
    Ready,
    Playing,
    Won,
    Lost,
}

pub struct Game {
    pub grid: Vec<Vec<Cell>>,
    pub width: usize,
    pub height: usize,
    pub mine_count: usize,
    pub state: GameState,
    pub flags_placed: usize,
    pub variant: Variant,
    /// Suppress the win condition - used by Marathon (endless mode).
    pub endless: bool,
    /// Original mine density (mines / cells), used to generate new Marathon rows.
    pub mine_density: f64,
}

impl Game {
    pub fn new(width: usize, height: usize, mine_count: usize, variant: Variant) -> Self {
        let num_cells = mine_count.min(width * height - 1);
        let grid = vec![vec![Cell::new(); width]; height];
        let density = num_cells as f64 / (width * height) as f64;
        let mut game = Game {
            grid,
            width,
            height,
            mine_count: num_cells,
            state: GameState::Ready,
            flags_placed: 0,
            variant,
            endless: false,
            mine_density: density,
        };
        // Pre-place stacks so mine_count reflects total units from the start.
        // Adjacency is computed later (on first click) once the safe zone is known.
        if variant == Variant::MultiMines {
            game.preplaced_multi_mines(num_cells);
        }
        game
    }

    /// Place MultiMines stacks across the whole board (no safe zone yet).
    /// Updates mine_count to the actual total mine units.
    fn preplaced_multi_mines(&mut self, num_cells: usize) {
        let mut rng = thread_rng();
        let mut positions: Vec<(usize, usize)> = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .collect();
        positions.shuffle(&mut rng);
        for i in 0..num_cells.min(positions.len()) {
            let (x, y) = positions[i];
            self.grid[y][x].mines = rng.gen_range(1u8..=3);
        }
        self.mine_count = self.grid.iter().flatten()
            .map(|c| c.mines as usize)
            .sum();
    }

    fn place_mines(&mut self, first_x: usize, first_y: usize) {
        let mut rng = thread_rng();

        if self.variant == Variant::MultiMines {
            // Stacks are already placed; just move any mines out of the 3×3 safe
            // zone, then compute adjacency. mine_count stays correct throughout.
            let mut destinations: Vec<(usize, usize)> = (0..self.height)
                .flat_map(|y| (0..self.width).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let dx = (x as isize - first_x as isize).abs();
                    let dy = (y as isize - first_y as isize).abs();
                    (dx > 1 || dy > 1) && self.grid[y][x].mines == 0
                })
                .collect();
            destinations.shuffle(&mut rng);
            let mut dest_idx = 0;

            for dy in -1_isize..=1 {
                for dx in -1_isize..=1 {
                    let nx = first_x as isize + dx;
                    let ny = first_y as isize + dy;
                    if nx < 0 || nx >= self.width as isize
                        || ny < 0 || ny >= self.height as isize { continue; }
                    let (nx, ny) = (nx as usize, ny as usize);
                    if self.grid[ny][nx].mines > 0 && dest_idx < destinations.len() {
                        let (tx, ty) = destinations[dest_idx];
                        dest_idx += 1;
                        self.grid[ty][tx].mines = self.grid[ny][nx].mines;
                        self.grid[ny][nx].mines = 0;
                    }
                }
            }
        } else {
            let mut positions: Vec<(usize, usize)> = (0..self.height)
                .flat_map(|y| (0..self.width).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let dx = (x as isize - first_x as isize).abs();
                    let dy = (y as isize - first_y as isize).abs();
                    dx > 1 || dy > 1
                })
                .collect();

            if positions.len() < self.mine_count {
                positions = (0..self.height)
                    .flat_map(|y| (0..self.width).map(move |x| (x, y)))
                    .filter(|&(x, y)| x != first_x || y != first_y)
                    .collect();
            }

            positions.shuffle(&mut rng);
            for i in 0..self.mine_count.min(positions.len()) {
                let (x, y) = positions[i];
                self.grid[y][x].mines = 1;
                if self.variant == Variant::NegativeMines {
                    self.grid[y][x].is_negative = negative_mines::roll_negative();
                }
            }
        }

        for y in 0..self.height {
            for x in 0..self.width {
                if self.grid[y][x].mines == 0 {
                    self.grid[y][x].adjacent_mines = self.count_adjacent_mines(x, y);
                }
            }
        }
    }

    fn count_adjacent_mines(&self, x: usize, y: usize) -> i8 {
        let mut count: i8 = 0;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < self.width as isize && ny >= 0 && ny < self.height as isize {
                    let cell = &self.grid[ny as usize][nx as usize];
                    if cell.mines > 0 {
                        let contrib = cell.mines as i8;
                        if cell.is_negative { count -= contrib; } else { count += contrib; }
                    }
                }
            }
        }
        count
    }

    /// Place mines eagerly with no safe zone and compute adjacency.
    /// Used by Minelayer, which reveals the full board from the start.
    pub fn generate(&mut self) {
        let mut rng = thread_rng();
        let mut positions: Vec<(usize, usize)> = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .collect();
        positions.shuffle(&mut rng);
        for i in 0..self.mine_count.min(positions.len()) {
            let (x, y) = positions[i];
            self.grid[y][x].mines = 1;
        }
        for y in 0..self.height {
            for x in 0..self.width {
                if self.grid[y][x].mines == 0 {
                    self.grid[y][x].adjacent_mines = self.count_adjacent_mines(x, y);
                }
            }
        }
    }

    pub fn reveal(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }

        if self.state == GameState::Ready {
            self.place_mines(x, y);
            self.state = GameState::Playing;
        }

        if self.state != GameState::Playing { return false; }

        let cell = &self.grid[y][x];
        if cell.state != CellState::Hidden { return false; }

        self.grid[y][x].state = CellState::Revealed;

        if self.grid[y][x].mines > 0 {
            self.state = GameState::Lost;
            self.reveal_all_mines();
            return true;
        }

        if self.grid[y][x].adjacent_mines == 0 {
            self.cascade_reveal(x, y);
        }

        if !self.endless { self.check_win(); }
        true
    }

    fn cascade_reveal(&mut self, x: usize, y: usize) {
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < self.width as isize && ny >= 0 && ny < self.height as isize {
                    let nx = nx as usize;
                    let ny = ny as usize;
                    if self.grid[ny][nx].state == CellState::Hidden && self.grid[ny][nx].mines == 0 {
                        self.grid[ny][nx].state = CellState::Revealed;
                        if self.grid[ny][nx].adjacent_mines == 0 {
                            self.cascade_reveal(nx, ny);
                        }
                    }
                }
            }
        }
    }

    pub fn toggle_flag(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }
        if self.state != GameState::Playing && self.state != GameState::Ready { return false; }

        let cell = &mut self.grid[y][x];
        match cell.state {
            CellState::Hidden => {
                cell.state = CellState::Flagged;
                self.flags_placed += 1;
            }
            CellState::Flagged => match self.variant {
                Variant::MultiMines => {
                    cell.state = CellState::Flagged2;
                    self.flags_placed += 1; // now at weight 2
                }
                Variant::NegativeMines => {
                    cell.state = CellState::FlaggedNegative;
                    // flags_placed unchanged - still 1 unit
                }
                _ => {
                    cell.state = CellState::Hidden;
                    self.flags_placed -= 1;
                }
            },
            CellState::Flagged2 => {
                cell.state = CellState::Flagged3;
                self.flags_placed += 1; // now at weight 3
            }
            CellState::Flagged3 => {
                cell.state = CellState::Hidden;
                self.flags_placed -= 3;
            }
            CellState::FlaggedNegative => {
                cell.state = CellState::Hidden;
                self.flags_placed -= 1;
            }
            CellState::Revealed => return false,
        }
        true
    }

    pub fn chord(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }
        if self.state != GameState::Playing { return false; }
        // These variants display values that don't correspond to adjacent mine count,
        // so chording against adjacent_mines would be meaningless.
        if matches!(self.variant, Variant::Relative | Variant::Average | Variant::Offset) { return false; }

        let cell = &self.grid[y][x];
        if cell.state != CellState::Revealed || cell.adjacent_mines == 0 { return false; }

        // Weighted flag sum: Flagged=+1, Flagged2=+2, Flagged3=+3, FlaggedNegative=-1
        let mut flag_count: i8 = 0;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < self.width as isize && ny >= 0 && ny < self.height as isize {
                    match self.grid[ny as usize][nx as usize].state {
                        CellState::Flagged => flag_count += 1,
                        CellState::Flagged2 => flag_count += 2,
                        CellState::Flagged3 => flag_count += 3,
                        CellState::FlaggedNegative => flag_count -= 1,
                        _ => {}
                    }
                }
            }
        }

        if flag_count != self.grid[y][x].adjacent_mines { return false; }

        // Verify every flagged neighbour is correctly placed on the right kind of mine.
        // This ensures chord only fires when all flags are accurate, not just numerically matching.
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || nx >= self.width as isize || ny < 0 || ny >= self.height as isize { continue; }
                let nc = &self.grid[ny as usize][nx as usize];
                let correct = match nc.state {
                    CellState::Flagged         => nc.mines == 1 && !nc.is_negative,
                    CellState::FlaggedNegative => nc.mines == 1 && nc.is_negative,
                    CellState::Flagged2        => nc.mines == 2,
                    CellState::Flagged3        => nc.mines == 3,
                    _                          => true,
                };
                if !correct { return false; }
            }
        }

        let mut revealed = false;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < self.width as isize && ny >= 0 && ny < self.height as isize {
                    let nx = nx as usize;
                    let ny = ny as usize;
                    if self.grid[ny][nx].state == CellState::Hidden {
                        if self.reveal(nx, ny) { revealed = true; }
                    }
                }
            }
        }
        revealed
    }

    pub fn reveal_neighbors(&mut self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height { return false; }
        if self.state != GameState::Playing { return false; }

        let cell = &self.grid[y][x];
        if cell.state != CellState::Revealed || cell.adjacent_mines == 0 { return false; }

        let mut revealed = false;
        for dy in -1_isize..=1 {
            for dx in -1_isize..=1 {
                if dx == 0 && dy == 0 { continue; }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < self.width as isize && ny >= 0 && ny < self.height as isize {
                    let nx = nx as usize;
                    let ny = ny as usize;
                    if self.grid[ny][nx].state == CellState::Hidden {
                        if self.reveal(nx, ny) { revealed = true; }
                    }
                }
            }
        }
        revealed
    }

    fn reveal_all_mines(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                if self.grid[y][x].mines > 0 {
                    self.grid[y][x].state = CellState::Revealed;
                }
            }
        }
    }

    fn check_win(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                let cell = &self.grid[y][x];
                if cell.mines == 0 && cell.state != CellState::Revealed { return; }
            }
        }
        self.state = GameState::Won;
    }

    pub fn recompute_all_adjacency(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                if self.grid[y][x].mines == 0 {
                    self.grid[y][x].adjacent_mines = self.count_adjacent_mines(x, y);
                }
            }
        }
    }

    pub fn remaining_mines(&self) -> isize {
        if self.state == GameState::Ready { return 0; }
        self.mine_count as isize - self.flags_placed as isize
    }

    /// Called at the end of every N-move window in Marathon mode.
    ///
    /// Validates the bottom row: every cell must be `Revealed`, or `Flagged` on
    /// an actual mine (Hidden or wrong-flag = instant loss).
    /// On success: removes the bottom row, inserts a freshly-mined row at the
    /// top, recomputes adjacency for the affected rows, and returns `true`.
    /// On failure: sets state to `Lost`, reveals all mines, returns `false`.
    /// Every 3 moves in Drift mode: each mine slides to a random adjacent
    /// unrevealed cell that isn't already occupied.  All moves are planned
    /// against the *original* grid snapshot and applied atomically, so no
    /// mine can collide with or displace another.
    /// Rotate the grid 90° clockwise.
    /// Cell (x, y) moves to (new_x = old_h-1-y, new_y = x).
    /// Adjacent mine counts are preserved - every cell and its neighbours
    /// rotate together, so the spatial relationships are unchanged.
    pub fn rotate_90_cw(&mut self) {
        let (old_w, old_h) = (self.width, self.height);
        // New dimensions: new_width = old_h, new_height = old_w
        let mut new_grid = vec![vec![Cell::new(); old_h]; old_w];
        for y in 0..old_h {
            for x in 0..old_w {
                new_grid[x][old_h - 1 - y] = self.grid[y][x];
            }
        }
        self.grid   = new_grid;
        self.width  = old_h;
        self.height = old_w;
    }

    pub fn drift_mines(&mut self) {
        let mut rng = thread_rng();

        // Snapshot: (x, y, mine_units, is_negative)
        let mut mines: Vec<(usize, usize, u8, bool)> = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|&(x, y)| self.grid[y][x].mines > 0)
            .map(|(x, y)| (x, y, self.grid[y][x].mines, self.grid[y][x].is_negative))
            .collect();
        mines.shuffle(&mut rng);

        // Plan destinations without modifying the grid yet.
        // `claimed` prevents two mines landing on the same cell.
        let mut claimed: HashSet<(usize, usize)> = HashSet::new();
        let mut assignments: Vec<((usize, usize), (usize, usize), u8, bool)> = Vec::new();

        for &(mx, my, mv, neg) in &mines {
            let targets: Vec<(usize, usize)> = (-1_isize..=1)
                .flat_map(|dy| (-1_isize..=1).map(move |dx| (dx, dy)))
                .filter(|&(dx, dy)| dx != 0 || dy != 0)
                .filter_map(|(dx, dy)| {
                    let nx = mx as isize + dx;
                    let ny = my as isize + dy;
                    if nx < 0 || nx >= self.width as isize
                        || ny < 0 || ny >= self.height as isize { return None; }
                    let (nx, ny) = (nx as usize, ny as usize);
                    // Valid target: no mine in original grid, not revealed, not claimed.
                    if self.grid[ny][nx].mines == 0
                        && self.grid[ny][nx].state != CellState::Revealed
                        && !claimed.contains(&(nx, ny))
                    { Some((nx, ny)) } else { None }
                })
                .collect();

            let dest = if targets.is_empty() { (mx, my) }
                       else { *targets.choose(&mut rng).unwrap() };
            claimed.insert(dest);
            assignments.push(((mx, my), dest, mv, neg));
        }

        // Apply: clear originals, then set destinations.
        for &((ox, oy), _, _, _) in &assignments {
            self.grid[oy][ox].mines = 0;
            self.grid[oy][ox].is_negative = false;
        }
        for &(_, (dx, dy), mv, neg) in &assignments {
            self.grid[dy][dx].mines = mv;
            self.grid[dy][dx].is_negative = neg;
        }

        // Recompute adjacency for every non-mine cell.
        for y in 0..self.height {
            for x in 0..self.width {
                if self.grid[y][x].mines == 0 {
                    self.grid[y][x].adjacent_mines = self.count_adjacent_mines(x, y);
                }
            }
        }
    }

    pub fn marathon_shift(&mut self) -> bool {
        let hy = self.height - 1;

        // ── Validate bottom row ──────────────────────────────────────────────
        for x in 0..self.width {
            let cell = &self.grid[hy][x];
            let bad = match cell.state {
                CellState::Hidden        => true,
                CellState::Flagged       => !(cell.mines == 1 && !cell.is_negative),
                CellState::FlaggedNegative => !(cell.mines == 1 && cell.is_negative),
                CellState::Flagged2      => cell.mines != 2,
                CellState::Flagged3      => cell.mines != 3,
                CellState::Revealed      => false,
            };
            if bad {
                self.state = GameState::Lost;
                self.reveal_all_mines();
                return false;
            }
        }

        // ── Remove flags from the departing row ──────────────────────────────
        for x in 0..self.width {
            match self.grid[hy][x].state {
                CellState::Flagged | CellState::FlaggedNegative => self.flags_placed -= 1,
                CellState::Flagged2 => self.flags_placed -= 2,
                CellState::Flagged3 => self.flags_placed -= 3,
                _ => {}
            }
        }

        // ── Remove bottom row, adjust mine_count ────────────────────────────
        let removed: usize = self.grid[hy].iter().map(|c| c.mines as usize).sum();
        self.mine_count -= removed;
        self.grid.remove(hy);

        // ── Generate new top row at original density ─────────────────────────
        let mut rng = thread_rng();
        let mut new_row = vec![Cell::new(); self.width];
        for cell in new_row.iter_mut() {
            if rng.gen::<f64>() < self.mine_density {
                cell.mines = 1;
            }
        }
        let added: usize = new_row.iter().map(|c| c.mines as usize).sum();
        self.mine_count += added;
        self.grid.insert(0, new_row);

        // ── Recompute adjacency for changed rows ─────────────────────────────
        // Row 0 (new):       all neighbours are new.
        // Row 1 (old row 0): its top neighbour is now the new row.
        // Row height-1 (old row height-2): its bottom neighbour is gone.
        let recompute = {
            let mut v = vec![0usize, self.height - 1];
            if self.height > 1 { v.push(1); }
            v.sort_unstable();
            v.dedup();
            v
        };
        for ry in recompute {
            for x in 0..self.width {
                if self.grid[ry][x].mines == 0 {
                    self.grid[ry][x].adjacent_mines = self.count_adjacent_mines(x, ry);
                }
            }
        }

        true
    }
}
