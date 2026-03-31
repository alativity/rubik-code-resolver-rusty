use cgmath::Vector3;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Up,    // Белый
    Down,  // Желтый
    Front, // Красный
    Back,  // Оранжевый
    Right, // Зеленый
    Left,  // Синий
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    U, U2, UPrime,
    D, D2, DPrime,
    F, F2, FPrime,
    B, B2, BPrime,
    R, R2, RPrime,
    L, L2, LPrime,
}

impl Move {
    pub fn all() -> Vec<Move> {
        vec![
            Move::U, Move::U2, Move::UPrime,
            Move::D, Move::D2, Move::DPrime,
            Move::F, Move::F2, Move::FPrime,
            Move::B, Move::B2, Move::BPrime,
            Move::R, Move::R2, Move::RPrime,
            Move::L, Move::L2, Move::LPrime,
        ]
    }

    pub fn inverse(&self) -> Move {
        match self {
            Move::U => Move::UPrime,
            Move::U2 => Move::U2,
            Move::UPrime => Move::U,
            Move::D => Move::DPrime,
            Move::D2 => Move::D2,
            Move::DPrime => Move::D,
            Move::F => Move::FPrime,
            Move::F2 => Move::F2,
            Move::FPrime => Move::F,
            Move::B => Move::BPrime,
            Move::B2 => Move::B2,
            Move::BPrime => Move::B,
            Move::R => Move::RPrime,
            Move::R2 => Move::R2,
            Move::RPrime => Move::R,
            Move::L => Move::LPrime,
            Move::L2 => Move::L2,
            Move::LPrime => Move::L,
        }
    }

    pub fn rotation_axis(&self) -> Vector3<f32> {
        match self {
            Move::U | Move::U2 | Move::UPrime | Move::D | Move::D2 | Move::DPrime => Vector3::unit_y(),
            Move::F | Move::F2 | Move::FPrime | Move::B | Move::B2 | Move::BPrime => Vector3::unit_z(),
            Move::R | Move::R2 | Move::RPrime | Move::L | Move::L2 | Move::LPrime => Vector3::unit_x(),
        }
    }

    pub fn rotation_angle_deg(&self) -> f32 {
        match self {
            // Inverse (Synchronous) rotations
            Move::U | Move::R | Move::F => 90.0,
            Move::D | Move::L | Move::B => -90.0,
            Move::U2 | Move::D2 | Move::F2 | Move::B2 | Move::R2 | Move::L2 => 180.0,
            Move::UPrime | Move::RPrime | Move::FPrime => -90.0,
            Move::DPrime | Move::LPrime | Move::BPrime => 90.0,
        }
    }

    pub fn face_id(&self) -> u8 {
        match self {
            Move::U | Move::U2 | Move::UPrime => 0,
            Move::D | Move::D2 | Move::DPrime => 1,
            Move::F | Move::F2 | Move::FPrime => 2,
            Move::B | Move::B2 | Move::BPrime => 3,
            Move::R | Move::R2 | Move::RPrime => 4,
            Move::L | Move::L2 | Move::LPrime => 5,
        }
    }

    pub fn layer_index(&self) -> usize {
        match self {
            Move::U | Move::U2 | Move::UPrime => 2,  // Top layer
            Move::D | Move::D2 | Move::DPrime => 0,  // Bottom
            Move::F | Move::F2 | Move::FPrime => 2,  // Front
            Move::B | Move::B2 | Move::BPrime => 0,  // Back
            Move::R | Move::R2 | Move::RPrime => 2,  // Right
            Move::L | Move::L2 | Move::LPrime => 0,  // Left
        }
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Move::U => write!(f, "U"),
            Move::U2 => write!(f, "U2"),
            Move::UPrime => write!(f, "U'"),
            Move::D => write!(f, "D"),
            Move::D2 => write!(f, "D2"),
            Move::DPrime => write!(f, "D'"),
            Move::F => write!(f, "F"),
            Move::F2 => write!(f, "F2"),
            Move::FPrime => write!(f, "F'"),
            Move::B => write!(f, "B"),
            Move::B2 => write!(f, "B2"),
            Move::BPrime => write!(f, "B'"),
            Move::R => write!(f, "R"),
            Move::R2 => write!(f, "R2"),
            Move::RPrime => write!(f, "R'"),
            Move::L => write!(f, "L"),
            Move::L2 => write!(f, "L2"),
            Move::LPrime => write!(f, "L'"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RubiksCube {
    up: [[Face; 3]; 3],
    down: [[Face; 3]; 3],
    front: [[Face; 3]; 3],
    back: [[Face; 3]; 3],
    right: [[Face; 3]; 3],
    left: [[Face; 3]; 3],
}

impl RubiksCube {
    pub fn new() -> Self {
        Self {
            up: [[Face::Up; 3]; 3],
            down: [[Face::Down; 3]; 3],
            front: [[Face::Front; 3]; 3],
            back: [[Face::Back; 3]; 3],
            right: [[Face::Right; 3]; 3],
            left: [[Face::Left; 3]; 3],
        }
    }

    pub fn scrambled(num_moves: usize) -> Self {
        use rand::Rng;
        let mut rng = rand::rng();
        let mut cube = Self::new();
        let moves = Move::all();

        let mut last_face: Option<u8> = None;
        for _ in 0..num_moves {
            loop {
                let m = moves[rng.random_range(0..moves.len())];
                let face = m.face_id();
                if last_face != Some(face) {
                    cube.apply_move(m);
                    last_face = Some(face);
                    break;
                }
            }
        }

        while cube.is_solved() {
            let m = moves[rng.random_range(0..moves.len())];
            cube.apply_move(m);
        }

        cube
    }

    pub fn apply_move(&mut self, m: Move) {
        match m {
            Move::U => self.rotate_u(),
            Move::U2 => { self.rotate_u(); self.rotate_u(); }
            Move::UPrime => { self.rotate_u(); self.rotate_u(); self.rotate_u(); }

            Move::D => self.rotate_d(),
            Move::D2 => { self.rotate_d(); self.rotate_d(); }
            Move::DPrime => { self.rotate_d(); self.rotate_d(); self.rotate_d(); }

            Move::F => self.rotate_f(),
            Move::F2 => { self.rotate_f(); self.rotate_f(); }
            Move::FPrime => { self.rotate_f(); self.rotate_f(); self.rotate_f(); }

            Move::B => self.rotate_b(),
            Move::B2 => { self.rotate_b(); self.rotate_b(); }
            Move::BPrime => { self.rotate_b(); self.rotate_b(); self.rotate_b(); }

            Move::R => self.rotate_r(),
            Move::R2 => { self.rotate_r(); self.rotate_r(); }
            Move::RPrime => { self.rotate_r(); self.rotate_r(); self.rotate_r(); }

            Move::L => self.rotate_l(),
            Move::L2 => { self.rotate_l(); self.rotate_l(); }
            Move::LPrime => { self.rotate_l(); self.rotate_l(); self.rotate_l(); }
        }
    }

    fn rotate_face_clockwise(face: &mut [[Face; 3]; 3]) {
        let temp = face[0][0];
        face[0][0] = face[2][0];
        face[2][0] = face[2][2];
        face[2][2] = face[0][2];
        face[0][2] = temp;

        let temp = face[0][1];
        face[0][1] = face[1][0];
        face[1][0] = face[2][1];
        face[2][1] = face[1][2];
        face[1][2] = temp;
    }

    fn rotate_u(&mut self) {
        RubiksCube::rotate_face_clockwise(&mut self.up);

        let temp = [self.front[0][0], self.front[0][1], self.front[0][2]];
        self.front[0] = self.right[0];
        self.right[0] = self.back[0];
        self.back[0] = self.left[0];
        self.left[0] = temp;
    }

    fn rotate_d(&mut self) {
        RubiksCube::rotate_face_clockwise(&mut self.down);

        let temp = [self.front[2][0], self.front[2][1], self.front[2][2]];
        self.front[2] = self.left[2];
        self.left[2] = self.back[2];
        self.back[2] = self.right[2];
        self.right[2] = temp;
    }

    fn rotate_f(&mut self) {
        RubiksCube::rotate_face_clockwise(&mut self.front);

        let temp_up = [self.up[2][0], self.up[2][1], self.up[2][2]];
        self.up[2] = [self.left[2][2], self.left[1][2], self.left[0][2]];
        self.left[0][2] = self.down[0][0];
        self.left[1][2] = self.down[0][1];
        self.left[2][2] = self.down[0][2];
        self.down[0] = [self.right[2][0], self.right[1][0], self.right[0][0]];
        self.right[0][0] = temp_up[0];
        self.right[1][0] = temp_up[1];
        self.right[2][0] = temp_up[2];
    }

    fn rotate_b(&mut self) {
        RubiksCube::rotate_face_clockwise(&mut self.back);

        let temp_up = [self.up[0][0], self.up[0][1], self.up[0][2]];
        self.up[0] = [self.right[0][2], self.right[1][2], self.right[2][2]];
        self.right[0][2] = self.down[2][2];
        self.right[1][2] = self.down[2][1];
        self.right[2][2] = self.down[2][0];
        self.down[2] = [self.left[2][0], self.left[1][0], self.left[0][0]];
        self.left[0][0] = temp_up[0];
        self.left[1][0] = temp_up[1];
        self.left[2][0] = temp_up[2];
    }

    fn rotate_r(&mut self) {
        RubiksCube::rotate_face_clockwise(&mut self.right);

        let temp_up = [self.up[0][2], self.up[1][2], self.up[2][2]];
        self.up[0][2] = self.front[0][2];
        self.up[1][2] = self.front[1][2];
        self.up[2][2] = self.front[2][2];
        self.front[0][2] = self.down[0][2];
        self.front[1][2] = self.down[1][2];
        self.front[2][2] = self.down[2][2];
        self.down[0][2] = self.back[2][0];
        self.down[1][2] = self.back[1][0];
        self.down[2][2] = self.back[0][0];
        self.back[0][0] = temp_up[2];
        self.back[1][0] = temp_up[1];
        self.back[2][0] = temp_up[0];
    }

    fn rotate_l(&mut self) {
        RubiksCube::rotate_face_clockwise(&mut self.left);

        let temp_up = [self.up[0][0], self.up[1][0], self.up[2][0]];
        self.up[0][0] = self.back[2][2];
        self.up[1][0] = self.back[1][2];
        self.up[2][0] = self.back[0][2];
        self.back[0][2] = self.down[2][0];
        self.back[1][2] = self.down[1][0];
        self.back[2][2] = self.down[0][0];
        self.down[0][0] = self.front[0][0];
        self.down[1][0] = self.front[1][0];
        self.down[2][0] = self.front[2][0];
        self.front[0][0] = temp_up[0];
        self.front[1][0] = temp_up[1];
        self.front[2][0] = temp_up[2];
    }

    pub fn is_solved(&self) -> bool {
        self.check_face(&self.up, Face::Up) &&
            self.check_face(&self.down, Face::Down) &&
            self.check_face(&self.front, Face::Front) &&
            self.check_face(&self.back, Face::Back) &&
            self.check_face(&self.right, Face::Right) &&
            self.check_face(&self.left, Face::Left)
    }

    fn check_face(&self, face: &[[Face; 3]; 3], expected: Face) -> bool {
        face.iter().all(|row| row.iter().all(|&cell| cell == expected))
    }

    pub fn get_face(&self, face: Face) -> &[[Face; 3]; 3] {
        match face {
            Face::Up => &self.up,
            Face::Down => &self.down,
            Face::Front => &self.front,
            Face::Back => &self.back,
            Face::Right => &self.right,
            Face::Left => &self.left,
        }
    }
}