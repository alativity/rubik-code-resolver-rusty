use crate::cube::{Face, Move, RubiksCube};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Instant;

// --------------------------------------------------------------------------
// Constants
// --------------------------------------------------------------------------

const ALL_MOVES: [Move; 18] = [
    Move::U,  Move::U2,  Move::UPrime,
    Move::D,  Move::D2,  Move::DPrime,
    Move::F,  Move::F2,  Move::FPrime,
    Move::B,  Move::B2,  Move::BPrime,
    Move::R,  Move::R2,  Move::RPrime,
    Move::L,  Move::L2,  Move::LPrime,
];

const FACES: [Face; 6] = [
    Face::Up, Face::Down, Face::Front, Face::Back, Face::Right, Face::Left,
];

/// Bidirectional BFS stops when total explored states exceed this.
/// At depth-5 per direction: ~1.2 M states.  Keep generous headroom.
const BFS_MAX_STATES: usize = 6_000_000;

const ENDGAME_DEPTH: u8 = 5;

/// Node budget for the IDA* sub-tree of each thread (root-splitting).
const IDA_NODES_PER_THREAD: u64 = 30_000_000;

const IDA_MAX_DEPTH: u32 = 50;

// --------------------------------------------------------------------------
// Public types
// --------------------------------------------------------------------------

pub struct SolverStats {
    pub status: &'static str,
    /// Wall-clock ms since solver started (updated live by solver thread).
    pub elapsed_ms: u64,
    /// Current BFS depth (bidir) or IDA* threshold.
    pub depth: u32,
    pub nodes_explored: u64,
    pub solution_len: usize,
    /// Logical CPU threads available on this machine.
    pub cpu_logical: usize,
    /// Threads actually used by the solver (80 % of logical, capped at 18).
    pub solver_threads: usize,
}

impl SolverStats {
    pub fn new() -> Self {
        let cpu_logical = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let solver_threads = Solver::compute_solver_threads_from(cpu_logical);
        Self {
            status: "starting",
            elapsed_ms: 0,
            depth: 0,
            nodes_explored: 0,
            solution_len: 0,
            cpu_logical,
            solver_threads,
        }
    }
}

// --------------------------------------------------------------------------
// Endgame look-up table  (BFS from solved, exact distances ≤ ENDGAME_DEPTH)
// --------------------------------------------------------------------------

struct EndgameTable {
    dist: HashMap<u64, u8>,
    max_d: u8,
}

impl EndgameTable {
    fn build(max_d: u8) -> Arc<Self> {
        let mut dist: HashMap<u64, u8> = HashMap::new();
        let solved = RubiksCube::new();
        dist.insert(Solver::cube_hash(&solved), 0);

        let mut frontier = vec![solved];
        for d in 1..=max_d {
            let mut next = Vec::new();
            for state in &mut frontier {
                for &m in &ALL_MOVES {
                    state.apply_move(m);
                    let h = Solver::cube_hash(state);
                    if !dist.contains_key(&h) {
                        dist.insert(h, d);
                        next.push(state.clone());
                    }
                    state.apply_move(m.inverse());
                }
            }
            frontier = next;
        }
        Arc::new(Self { dist, max_d })
    }

    #[inline]
    fn lookup(&self, h: u64) -> u32 {
        self.dist.get(&h).copied().unwrap_or(self.max_d + 1) as u32
    }
}

// --------------------------------------------------------------------------
// Solver
// --------------------------------------------------------------------------

pub struct Solver;

enum SearchResult {
    Found,
    NotFound(u32),
}

// BFS parent map entry: (parent_hash, move_index as u8)
// Root sentinel: parent_hash == own_hash, move_index == 255
type ParentMap = HashMap<u64, (u64, u8)>;

impl Solver {
    // -----------------------------------------------------------------------
    // Thread budget computation
    // -----------------------------------------------------------------------

    /// Returns 80 % of `n` logical threads, rounded up, min 1, max 18.
    ///
    /// The 80 % rule leaves headroom for the OS, the render thread, and
    /// E-cores / SMT siblings on hybrid or HyperThreaded CPUs.
    /// The cap of 18 matches the root-splitting branching factor (one per
    /// Rubik's move), so extra threads would be wasted.
    pub fn compute_solver_threads_from(logical: usize) -> usize {
        // ceil(logical * 0.8) = (logical * 4 + 4) / 5
        ((logical * 4 + 4) / 5).max(1).min(18)
    }

    /// Convenience: reads `available_parallelism` and applies the 80 % rule.
    pub fn compute_solver_threads() -> usize {
        let logical = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self::compute_solver_threads_from(logical)
    }

    // -----------------------------------------------------------------------
    // Entry point (spawns solver thread)
    // -----------------------------------------------------------------------

    pub fn solve_async(
        cube: RubiksCube,
        sender: mpsc::Sender<Move>,
        stats: Arc<Mutex<SolverStats>>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) {
        thread::spawn(move || {
            let start = Instant::now();
            let solution = Self::solve(&cube, &stats, &start, &cancel);

            let elapsed = start.elapsed();
            if let Ok(mut s) = stats.lock() {
                s.status = if solution.is_empty() { "failed" } else { "done" };
                s.elapsed_ms = elapsed.as_millis() as u64;
                s.solution_len = solution.len();
            }

            let names: Vec<String> = solution.iter().map(|m| format!("{m}")).collect();
            println!(
                "Solver: {} moves in {:.2?}  [{}]",
                names.len(),
                elapsed,
                names.join(" ")
            );

            for m in solution {
                if sender.send(m).is_err() {
                    break;
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Master solve routine
    // -----------------------------------------------------------------------

    fn solve(
        cube: &RubiksCube,
        stats: &Arc<Mutex<SolverStats>>,
        start: &Instant,
        cancel: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Vec<Move> {
        if cube.is_solved() {
            return vec![];
        }

        // Phase 1 ── Bidirectional BFS  (O(b^(d/2)) instead of O(b^d))
        println!("Solver: bidirectional BFS  (budget={} states)", BFS_MAX_STATES);
        if let Ok(mut s) = stats.lock() {
            s.status = "BFS";
        }

        let sol = Self::bidir_bfs(cube, stats, start, cancel.as_ref());
        if !sol.is_empty() || cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return sol;
        }

        // Phase 2 ── Parallel IDA* with endgame table & root-splitting
        println!("Solver: building endgame table (depth {})…", ENDGAME_DEPTH);
        if let Ok(mut s) = stats.lock() {
            s.status = "endgame";
        }
        let table = EndgameTable::build(ENDGAME_DEPTH);
        println!(
            "Solver: endgame table ready  ({} states, {:.2?})",
            table.dist.len(),
            start.elapsed()
        );

        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return vec![];
        }

        if let Ok(mut s) = stats.lock() {
            s.status = "IDA*";
        }
        println!("Solver: parallel IDA* with {} threads", Solver::compute_solver_threads());
        Self::parallel_ida_star(cube, table, stats, start, cancel.clone())
    }

    // -----------------------------------------------------------------------
    // Bidirectional BFS
    // -----------------------------------------------------------------------

    fn bidir_bfs(
        cube: &RubiksCube,
        stats: &Arc<Mutex<SolverStats>>,
        start: &Instant,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Vec<Move> {
        let solved = RubiksCube::new();
        let sh = Self::cube_hash(cube);
        let eh = Self::cube_hash(&solved);

        if sh == eh {
            return vec![];
        }

        let mut fwd: ParentMap = HashMap::new();
        let mut bwd: ParentMap = HashMap::new();
        fwd.insert(sh, (sh, 255));
        bwd.insert(eh, (eh, 255));

        let mut fwd_front: Vec<(RubiksCube, u64)> = vec![(cube.clone(), sh)];
        let mut bwd_front: Vec<(RubiksCube, u64)> = vec![(solved, eh)];

        let mut total_nodes: u64 = 0;
        let mut depth: u32 = 0;

        loop {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if fwd.len() + bwd.len() >= BFS_MAX_STATES {
                println!(
                    "Solver: BFS budget exhausted ({} states, {:.2?})",
                    fwd.len() + bwd.len(),
                    start.elapsed()
                );
                break;
            }
            if fwd_front.is_empty() || bwd_front.is_empty() {
                break;
            }

            depth += 1;

            // Always expand the smaller frontier to keep both sides balanced.
            let expand_fwd = fwd_front.len() <= bwd_front.len();

            if expand_fwd {
                if let Some(meeting) =
                    Self::expand_bfs_level(&mut fwd_front, &mut fwd, &bwd, &mut total_nodes)
                {
                    let path = Self::reconstruct(&fwd, &bwd, meeting, sh, eh);
                    println!(
                        "Solver: BFS found {} moves  depth={} nodes={} {:.2?}",
                        path.len(), depth, total_nodes, start.elapsed()
                    );
                    return path;
                }
            } else {
                if let Some(meeting) =
                    Self::expand_bfs_level(&mut bwd_front, &mut bwd, &fwd, &mut total_nodes)
                {
                    let path = Self::reconstruct(&fwd, &bwd, meeting, sh, eh);
                    println!(
                        "Solver: BFS found {} moves  depth={} nodes={} {:.2?}",
                        path.len(), depth, total_nodes, start.elapsed()
                    );
                    return path;
                }
            }

            if let Ok(mut s) = stats.lock() {
                s.depth = depth;
                s.nodes_explored = total_nodes;
                s.elapsed_ms = start.elapsed().as_millis() as u64;
            }
        }

        vec![]
    }

    /// Expand one BFS level.  Returns the meeting hash if an intersection
    /// is found between `own_map` (after inserting new states) and `other`.
    fn expand_bfs_level(
        frontier: &mut Vec<(RubiksCube, u64)>,
        own_map: &mut ParentMap,
        other_map: &ParentMap,
        total_nodes: &mut u64,
    ) -> Option<u64> {
        let mut new_frontier = Vec::new();

        for (state, parent_hash) in frontier.drain(..) {
            for (mi, &m) in ALL_MOVES.iter().enumerate() {
                let mut child = state.clone();
                child.apply_move(m);
                let ch = Self::cube_hash(&child);
                *total_nodes += 1;

                if own_map.contains_key(&ch) {
                    continue;
                }
                own_map.insert(ch, (parent_hash, mi as u8));

                if other_map.contains_key(&ch) {
                    return Some(ch);
                }
                new_frontier.push((child, ch));
            }
        }

        *frontier = new_frontier;
        None
    }

    /// Reconstruct the full solution from the meeting hash.
    ///
    /// `fwd` was BFS-ed from *start*; `bwd` was BFS-ed from *solved*.
    /// Forward path: start →moves→ meeting
    /// Backward path: meeting →inverse-moves→ solved
    fn reconstruct(
        fwd: &ParentMap,
        bwd: &ParentMap,
        meeting: u64,
        start_hash: u64,
        solved_hash: u64,
    ) -> Vec<Move> {
        // Forward: trace meeting → start through parent links, then reverse
        let mut fwd_path: Vec<Move> = Vec::new();
        let mut curr = meeting;
        while curr != start_hash {
            let (parent, mi) = fwd[&curr];
            fwd_path.push(ALL_MOVES[mi as usize]);
            curr = parent;
        }
        fwd_path.reverse(); // now start → meeting

        // Backward: trace meeting → solved, inverse each move (keep order)
        // bwd path was  solved →n0→ B1 →n1→ … →nj→ meeting
        // To undo:  meeting → apply nj⁻¹ → Bj-1 → … → n0⁻¹ → solved
        let mut bwd_path: Vec<Move> = Vec::new();
        let mut curr = meeting;
        while curr != solved_hash {
            let (parent, mi) = bwd[&curr];
            bwd_path.push(ALL_MOVES[mi as usize].inverse());
            curr = parent;
        }

        let mut result = fwd_path;
        result.extend(bwd_path);
        result
    }

    // -----------------------------------------------------------------------
    // Parallel IDA* with root-splitting
    // -----------------------------------------------------------------------

    fn parallel_ida_star(
        cube: &RubiksCube,
        table: Arc<EndgameTable>,
        stats: &Arc<Mutex<SolverStats>>,
        start: &Instant,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Vec<Move> {
        let n_threads = Solver::compute_solver_threads();
        println!("Solver: IDA* using {} threads", n_threads);

        // Global best solution length.  Threads update atomically.
        let best_len = Arc::new(AtomicU32::new(IDA_MAX_DEPTH));
        let best_solution: Arc<Mutex<Vec<Move>>> = Arc::new(Mutex::new(Vec::new()));

        // Distribute 18 first-moves across n_threads using round-robin so every
        // move is covered even when n_threads < 18.
        // Thread ti handles first-moves at indices ti, ti+n, ti+2n, …
        let handles: Vec<_> = (0..n_threads)
            .map(|ti| {
                let my_moves: Vec<Move> = ALL_MOVES
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(i, _)| i % n_threads == ti)
                    .map(|(_, m)| m)
                    .collect();

                let cube = cube.clone();
                let table = Arc::clone(&table);
                let best_len = Arc::clone(&best_len);
                let best_sol = Arc::clone(&best_solution);
                let stats = Arc::clone(stats);
                let start_instant = *start;
                let cancel_t = Arc::clone(&cancel);

                thread::spawn(move || {
                    // Each thread iterates over its assigned first-moves.
                    // Once the global best is known, sub-trees that can't
                    // improve are skipped immediately.
                    for first_move in my_moves {
                        if cancel_t.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                        let mut state = cube.clone();
                        state.apply_move(first_move);

                        let mut threshold =
                            1 + Self::heuristic_with_table(&state, &table);
                        let mut total_nodes: u64 = 0;

                        loop {
                            let global_best = best_len.load(Ordering::Relaxed);
                            if threshold >= global_best
                                || total_nodes >= IDA_NODES_PER_THREAD
                                || cancel_t.load(std::sync::atomic::Ordering::Relaxed)
                            {
                                break;
                            }

                            let mut path = vec![first_move];
                            let mut iter_nodes: u64 = 0;

                            let result = Self::ida_search_parallel(
                                &mut state.clone(),
                                1,
                                threshold,
                                &mut path,
                                Some(first_move),
                                &mut iter_nodes,
                                &best_len,
                                &table,
                            );

                            total_nodes += iter_nodes;

                            match result {
                                SearchResult::Found => {
                                    let len = path.len() as u32;
                                    let prev = best_len.fetch_min(len, Ordering::SeqCst);
                                    if len <= prev {
                                        *best_sol.lock().unwrap() = path;
                                    }
                                    break;
                                }
                                SearchResult::NotFound(next_t) => {
                                    if next_t == u32::MAX {
                                        break;
                                    }
                                    println!(
                                        "Solver[t{} {}]: threshold {} -> {} ({} nodes, {:.2?})",
                                        ti, first_move, threshold, next_t,
                                        total_nodes, start_instant.elapsed()
                                    );
                                    if let Ok(mut s) = stats.lock() {
                                        s.depth = next_t;
                                        s.nodes_explored =
                                            s.nodes_explored.saturating_add(iter_nodes);
                                        s.elapsed_ms =
                                            start_instant.elapsed().as_millis() as u64;
                                    }
                                    threshold = next_t;
                                }
                            }
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            let _ = h.join();
        }

        Arc::try_unwrap(best_solution)
            .ok()
            .and_then(|m| m.into_inner().ok())
            .unwrap_or_default()
    }

    fn ida_search_parallel(
        cube: &mut RubiksCube,
        g: u32,
        threshold: u32,
        path: &mut Vec<Move>,
        last_move: Option<Move>,
        nodes: &mut u64,
        best_len: &Arc<AtomicU32>,
        table: &Arc<EndgameTable>,
    ) -> SearchResult {
        *nodes += 1;
        if *nodes >= IDA_NODES_PER_THREAD {
            return SearchResult::NotFound(u32::MAX);
        }

        let h = Self::heuristic_with_table(cube, table);
        let f = g + h;

        // Prune by threshold and by global best found so far.
        let global_best = best_len.load(Ordering::Relaxed);
        if f > threshold || f >= global_best {
            return SearchResult::NotFound(f);
        }
        if cube.is_solved() {
            return SearchResult::Found;
        }

        // Ordered children (lower heuristic first)
        let mut children = [(ALL_MOVES[0], 0u32); 18];
        let mut count = 0usize;
        for &m in &ALL_MOVES {
            if Self::should_prune(m, last_move) {
                continue;
            }
            cube.apply_move(m);
            let ch = Self::heuristic_with_table(cube, table);
            cube.apply_move(m.inverse());
            children[count] = (m, ch);
            count += 1;
        }
        children[..count].sort_unstable_by_key(|&(_, h)| h);

        let mut min_next = u32::MAX;
        for i in 0..count {
            let (m, _) = children[i];
            cube.apply_move(m);
            path.push(m);
            match Self::ida_search_parallel(
                cube, g + 1, threshold, path, Some(m), nodes, best_len, table,
            ) {
                SearchResult::Found => return SearchResult::Found,
                SearchResult::NotFound(t) => min_next = min_next.min(t),
            }
            path.pop();
            cube.apply_move(m.inverse());
        }

        SearchResult::NotFound(min_next)
    }

    // -----------------------------------------------------------------------
    // Heuristics
    // -----------------------------------------------------------------------

    fn raw_distance(cube: &RubiksCube) -> u32 {
        let mut total = 0u32;
        for &face in &FACES {
            let grid = cube.get_face(face);
            for row in grid {
                for &cell in row {
                    if cell != face {
                        total += if Self::faces_opposite(cell, face) { 2 } else { 1 };
                    }
                }
            }
        }
        total
    }

    /// Combined admissible heuristic: max(endgame exact, raw_distance/4).
    /// /4 instead of /12 for much tighter pruning (inadmissible but fast).
    #[inline]
    fn heuristic_with_table(cube: &RubiksCube, table: &EndgameTable) -> u32 {
        let h_table = table.lookup(Self::cube_hash(cube));
        let h_raw = (Self::raw_distance(cube) + 3) / 4;
        h_table.max(h_raw)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    pub fn cube_hash(cube: &RubiksCube) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325; // FNV-1a
        for &face in &FACES {
            let grid = cube.get_face(face);
            for row in grid {
                for &cell in row {
                    h ^= cell as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
            }
        }
        h
    }

    fn faces_opposite(a: Face, b: Face) -> bool {
        matches!(
            (a, b),
            (Face::Up, Face::Down)
                | (Face::Down, Face::Up)
                | (Face::Front, Face::Back)
                | (Face::Back, Face::Front)
                | (Face::Right, Face::Left)
                | (Face::Left, Face::Right)
        )
    }

    #[inline]
    fn should_prune(m: Move, last: Option<Move>) -> bool {
        if let Some(last) = last {
            let mf = m.face_id();
            let lf = last.face_id();
            if mf == lf {
                return true;
            }
            if Self::ids_opposite(mf, lf) && mf < lf {
                return true;
            }
        }
        false
    }

    #[inline]
    fn ids_opposite(a: u8, b: u8) -> bool {
        matches!((a, b), (0,1)|(1,0)|(2,3)|(3,2)|(4,5)|(5,4))
    }
}

// --------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(scramble: &[Move]) {
        let mut cube = RubiksCube::new();
        for &m in scramble {
            cube.apply_move(m);
        }
        assert!(!cube.is_solved(), "scramble must produce unsolved cube");

        let stats = Arc::new(Mutex::new(SolverStats::new()));
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let start = Instant::now();
        let solution = Solver::solve(&cube, &stats, &start, &cancel);

        assert!(!solution.is_empty(), "solver must find a solution");
        for &m in &solution {
            cube.apply_move(m);
        }
        assert!(cube.is_solved(), "cube must be solved after solution");
        println!("  solved in {:.2?}", start.elapsed());
    }

    #[test]
    fn solve_3_moves() {
        run(&[Move::R, Move::U, Move::F]);
    }

    #[test]
    fn solve_5_moves() {
        run(&[Move::R, Move::U, Move::F, Move::D, Move::L]);
    }

    #[test]
    fn solve_10_moves() {
        run(&[
            Move::R, Move::U, Move::F, Move::D, Move::L,
            Move::B, Move::R2, Move::UPrime, Move::FPrime, Move::D2,
        ]);
    }
}
