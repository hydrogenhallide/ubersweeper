// Difficulty presets
pub const BEGINNER_WIDTH: usize = 9;
pub const BEGINNER_HEIGHT: usize = 9;
pub const BEGINNER_MINES: usize = 10;

pub const INTERMEDIATE_WIDTH: usize = 16;
pub const INTERMEDIATE_HEIGHT: usize = 16;
pub const INTERMEDIATE_MINES: usize = 40;

pub const EXPERT_WIDTH: usize = 30;
pub const EXPERT_HEIGHT: usize = 16;
pub const EXPERT_MINES: usize = 99;

// Cell size in pixels (width and height)
pub const CELL_SIZE: i32 = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Expert,
    Custom(usize, usize, usize), // width, height, mines
}

impl Difficulty {
    pub fn dimensions(&self) -> (usize, usize, usize) {
        match self {
            Difficulty::Beginner => (BEGINNER_WIDTH, BEGINNER_HEIGHT, BEGINNER_MINES),
            Difficulty::Intermediate => (INTERMEDIATE_WIDTH, INTERMEDIATE_HEIGHT, INTERMEDIATE_MINES),
            Difficulty::Expert => (EXPERT_WIDTH, EXPERT_HEIGHT, EXPERT_MINES),
            Difficulty::Custom(w, h, m) => (*w, *h, *m),
        }
    }
}
