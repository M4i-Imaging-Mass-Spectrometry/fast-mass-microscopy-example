#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn reverse(&self) -> Direction {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

#[derive(Copy, Clone)]
pub struct Coord {
    pub x: f64,
    pub y: f64,
    pub direction: Direction,
}

impl Coord {
    pub fn is_not_inf(&self) -> bool { self.x != f64::INFINITY && self.y != f64::INFINITY }
}
