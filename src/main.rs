#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cube;
mod solver;
mod renderer;

use cube::{RubiksCube, Move};
use solver::{Solver, SolverStats};
use renderer::{Renderer, HudInfo, ButtonInfo, button_rects};
use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use winit::{
    application::ApplicationHandler,
    event::{WindowEvent, MouseButton, ElementState},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId, CursorIcon},
};

const ORBIT_SENSITIVITY: f32 = 0.007; // radians per pixel

// ─────────────────────────────────────────────
// Button identifiers (must match BUTTON_LABELS order)
// ─────────────────────────────────────────────
const BUTTON_LABELS: [&str; 5] = ["SCRAMBLE", "SOLVE", "PAUSE", "STOP", "RESET"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SolvePhase {
    /// Cube visible, no solver running (may be solved or scrambled).
    Idle,
    /// Solver thread running in background.
    Solving,
    /// Solver done; animating queued moves.
    Animating,
    /// Animation paused; moves waiting in queue.
    Paused,
}

// ─────────────────────────────────────────────
// App
// ─────────────────────────────────────────────

struct App {
    window:           Arc<Window>,
    cube:             RubiksCube,
    renderer:         Renderer,
    move_receiver:    mpsc::Receiver<Move>,
    solver_stats:     Arc<Mutex<SolverStats>>,
    solver_start:     std::time::Instant,
    cancel_flag:      Arc<AtomicBool>,
    phase:            SolvePhase,
    channel_done:     bool,
    pending_moves:    Vec<Move>,
    current_move:     Option<Move>,
    animation_progress: f32,
    is_animating:     bool,
    pause_timer:      f32,
    moves_done:       usize,
    moves_total:      usize,
    fps_accum:        f32,
    fps_frame_count:  u32,
    current_fps:      u32,
    mouse_pos:        (f32, f32),
    /// Orbit camera drag state
    is_dragging:      bool,
    drag_last:        (f32, f32),
    orbit_yaw:        f32,
    orbit_pitch:      f32,
}

impl App {
    async fn new(
        window: Arc<Window>,
        move_receiver: mpsc::Receiver<Move>,
        cube: RubiksCube,
        solver_stats: Arc<Mutex<SolverStats>>,
    ) -> Self {
        let renderer = Renderer::new(Arc::clone(&window)).await;

        Self {
            window,
            cube,
            renderer,
            move_receiver,
            solver_stats,
            solver_start:      std::time::Instant::now(),
            cancel_flag:       Arc::new(AtomicBool::new(false)),
            phase:             SolvePhase::Idle,
            channel_done:      true,
            pending_moves:     Vec::new(),
            current_move:      None,
            animation_progress: 0.0,
            is_animating:      false,
            pause_timer:       0.0,
            moves_done:        0,
            moves_total:       0,
            fps_accum:         0.0,
            fps_frame_count:   0,
            current_fps:       0,
            mouse_pos:         (0.0, 0.0),
            is_dragging:       false,
            drag_last:         (0.0, 0.0),
            // Match renderer defaults so camera starts at the same angle
            orbit_yaw:         std::f32::consts::PI * 0.25,
            orbit_pitch:       0.611,
        }
    }

    // ── Poll-check: should we request continuous redraws? ─────────────────────
    #[allow(dead_code)]
    fn is_active(&self) -> bool {
        self.phase == SolvePhase::Solving
            || self.is_animating
            || self.pause_timer > 0.0
            || (!self.pending_moves.is_empty() && self.phase == SolvePhase::Animating)
            || self.is_dragging
    }

    // ── Per-frame update ───────────────────────────────────────────────────────
    fn update(&mut self, dt: f32) {
        let dt = dt.min(0.05);

        self.fps_accum += dt;
        self.fps_frame_count += 1;
        if self.fps_accum >= 0.5 {
            self.current_fps = (self.fps_frame_count as f32 / self.fps_accum) as u32;
            self.fps_accum = 0.0;
            self.fps_frame_count = 0;
        }

        if self.pause_timer > 0.0 {
            self.pause_timer -= dt;
            return;
        }

        if self.is_animating {
            self.animation_progress += dt * 2.0;
            if self.animation_progress >= 1.0 {
                if let Some(m) = self.current_move.take() {
                    self.cube.apply_move(m);
                    self.moves_done += 1;
                    if self.cube.is_solved() {
                        println!("Cube solved visually!");
                    }
                }
                self.is_animating = false;
                self.animation_progress = 0.0;
                // Only add inter-move pause if not paused
                if self.phase != SolvePhase::Paused {
                    self.pause_timer = 0.15;
                }
            }
        } else if !self.pending_moves.is_empty() && self.phase == SolvePhase::Animating {
            let next_move = self.pending_moves.remove(0);
            self.current_move = Some(next_move);
            self.is_animating = true;
            self.animation_progress = 0.0;
        }
    }

    // ── Build HUD data for the renderer ───────────────────────────────────────
    fn build_hud(&self) -> HudInfo {
        let (status, depth, nodes, final_ms, cpu_logical, solver_threads) =
            if let Ok(s) = self.solver_stats.lock() {
                (
                    s.status.to_string(),
                    s.depth,
                    s.nodes_explored,
                    s.elapsed_ms,
                    s.cpu_logical as u32,
                    s.solver_threads as u32,
                )
            } else {
                ("lock error".to_string(), 0, 0, 0, 0, 0)
            };

        let elapsed_ms = if self.channel_done {
            final_ms
        } else {
            self.solver_start.elapsed().as_millis() as u64
        };

        let move_name = match self.current_move {
            Some(m) => format!("{}", m),
            None => String::new(),
        };

        // ── Buttons ──────────────────────────────────────────────────────────
        let win_h = self.renderer.size().height as f32;
        let rects = button_rects(win_h);
        let phase = self.phase;

        // PAUSE label swaps to RESUME when paused
        let pause_label: &'static str = if phase == SolvePhase::Paused { "RESUME" } else { "PAUSE" };
        let labels: [&'static str; 5] = [
            BUTTON_LABELS[0], BUTTON_LABELS[1], pause_label, BUTTON_LABELS[3], BUTTON_LABELS[4],
        ];

        // Which buttons are enabled by phase
        let enabled: [bool; 5] = [
            // SCRAMBLE: always
            true,
            // SOLVE: only in Idle and cube not already solved
            phase == SolvePhase::Idle && !self.cube.is_solved(),
            // PAUSE/RESUME: only while animating or paused
            phase == SolvePhase::Animating || phase == SolvePhase::Paused,
            // STOP: while solving, animating, or paused
            phase == SolvePhase::Solving
                || phase == SolvePhase::Animating
                || phase == SolvePhase::Paused,
            // RESET: always
            true,
        ];

        let (mx, my) = self.mouse_pos;
        let buttons: Vec<ButtonInfo> = rects.iter().enumerate().map(|(i, &(x, y, bw, bh))| {
            let hovered = enabled[i]
                && mx >= x && mx <= x + bw
                && my >= y && my <= y + bh;
            ButtonInfo { label: labels[i], x, y, w: bw, h: bh, enabled: enabled[i], hovered }
        }).collect();

        HudInfo {
            fps: self.current_fps,
            solver_status: status,
            solver_time_ms: elapsed_ms,
            solver_depth: depth,
            solver_nodes: nodes,
            current_move_name: move_name,
            moves_done: self.moves_done,
            moves_total: self.moves_total,
            queue_size: self.pending_moves.len(),
            cpu_logical,
            solver_threads,
            buttons,
        }
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.renderer.set_orbit(self.orbit_yaw, self.orbit_pitch);
        let hud = self.build_hud();
        self.renderer.render(&self.cube, self.current_move, self.animation_progress, &hud)
    }

    // ── Button actions ────────────────────────────────────────────────────────

    fn action_scramble(&mut self) {
        self.stop_solver();
        self.cube = RubiksCube::scrambled(10);
        self.reset_animation();
        self.phase = SolvePhase::Idle;
        self.moves_done = 0;
        self.moves_total = 0;
        self.reset_stats_text("ready");
        println!("Scrambled new cube.");
    }

    fn action_solve(&mut self) {
        if self.phase != SolvePhase::Idle { return; }
        if self.cube.is_solved() {
            println!("Cube is already solved — press SCRAMBLE first.");
            return;
        }

        // Fresh cancel flag and channel for this solve session
        self.cancel_flag = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel::<Move>();
        self.move_receiver = receiver;
        self.channel_done = false;
        self.moves_done = 0;
        self.moves_total = 0;
        self.solver_start = std::time::Instant::now();
        self.reset_stats_text("starting");

        let solver_cube = self.cube.clone();
        Solver::solve_async(
            solver_cube,
            sender,
            Arc::clone(&self.solver_stats),
            Arc::clone(&self.cancel_flag),
        );

        self.phase = SolvePhase::Solving;
        self.window.request_redraw();
        println!("Solver started.");
    }

    fn action_pause_resume(&mut self) {
        match self.phase {
            SolvePhase::Animating => {
                self.phase = SolvePhase::Paused;
                println!("Animation paused.");
            }
            SolvePhase::Paused => {
                self.phase = SolvePhase::Animating;
                self.window.request_redraw();
                println!("Animation resumed.");
            }
            _ => {}
        }
    }

    fn action_stop(&mut self) {
        let was_active = self.phase != SolvePhase::Idle;
        self.stop_solver();
        self.reset_animation();
        self.phase = SolvePhase::Idle;
        self.reset_stats_text("stopped");
        if was_active {
            println!("Solve/animation stopped.");
        }
    }

    fn action_reset(&mut self) {
        self.stop_solver();
        self.cube = RubiksCube::new();
        self.reset_animation();
        self.phase = SolvePhase::Idle;
        self.moves_done = 0;
        self.moves_total = 0;
        self.reset_stats_text("ready");
        println!("Cube reset to solved state.");
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Signal the running solver to stop and replace the channel so old
    /// messages are discarded.  Does NOT change `self.phase`.
    fn stop_solver(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        // Drop old receiver — the sender in the solver thread will get
        // SendError and break out of its send loop naturally.
        let (_, new_rx) = mpsc::channel::<Move>();
        self.move_receiver = new_rx;
        self.channel_done = true;
    }

    fn reset_animation(&mut self) {
        self.pending_moves.clear();
        self.current_move = None;
        self.is_animating = false;
        self.animation_progress = 0.0;
        self.pause_timer = 0.0;
    }

    fn reset_stats_text(&self, status: &'static str) {
        if let Ok(mut s) = self.solver_stats.lock() {
            s.status = status;
            s.elapsed_ms = 0;
            s.depth = 0;
            s.nodes_explored = 0;
            s.solution_len = 0;
        }
    }

    /// Check whether a button was hit and call the appropriate action.
    fn handle_button_click(&mut self, mx: f32, my: f32) {
        let win_h = self.renderer.size().height as f32;
        let rects = button_rects(win_h);
        let phase = self.phase;

        // Mirror the enabled logic from build_hud
        let enabled: [bool; 5] = [
            true,
            phase == SolvePhase::Idle && !self.cube.is_solved(),
            phase == SolvePhase::Animating || phase == SolvePhase::Paused,
            phase == SolvePhase::Solving
                || phase == SolvePhase::Animating
                || phase == SolvePhase::Paused,
            true,
        ];

        for (i, &(x, y, bw, bh)) in rects.iter().enumerate() {
            if enabled[i] && mx >= x && mx <= x + bw && my >= y && my <= y + bh {
                match i {
                    0 => self.action_scramble(),
                    1 => self.action_solve(),
                    2 => self.action_pause_resume(),
                    3 => self.action_stop(),
                    4 => self.action_reset(),
                    _ => {}
                }
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────
// AppState (winit ApplicationHandler)
// ─────────────────────────────────────────────

struct AppState {
    app: Option<App>,
    window_id: Option<WindowId>,
    last_frame_time: std::time::Instant,
}

impl ApplicationHandler for AppState {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title("Rubik's Cube Solver")
                .with_inner_size(winit::dpi::LogicalSize::new(900, 700));

            let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
            self.window_id = Some(window.id());

            // Start with a SOLVED cube.  User presses SCRAMBLE then SOLVE.
            let cube = RubiksCube::new();
            let stats = Arc::new(Mutex::new(SolverStats::new()));

            // Dummy channel — sender dropped immediately, Disconnected on first recv.
            let (_, receiver) = mpsc::channel::<Move>();

            let app = pollster::block_on(App::new(
                Arc::clone(&window),
                receiver,
                cube,
                Arc::clone(&stats),
            ));

            window.request_redraw();

            self.app = Some(app);
            self.last_frame_time = std::time::Instant::now();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(app) = &mut self.app else { return };
        if window_id != self.window_id.unwrap() { return; }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                app.renderer.resize(physical_size);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (mx, my) = (position.x as f32, position.y as f32);
                app.mouse_pos = (mx, my);

                if app.is_dragging {
                    let (lx, ly) = app.drag_last;
                    let dx = mx - lx;
                    let dy = my - ly;
                    app.orbit_yaw   -= dx * ORBIT_SENSITIVITY;
                    app.orbit_pitch -= dy * ORBIT_SENSITIVITY;
                    // Clamp pitch to ±85° (done again in set_orbit, belt-and-suspenders)
                    let max_pitch = std::f32::consts::PI * 0.471;
                    app.orbit_pitch = app.orbit_pitch.clamp(-max_pitch, max_pitch);
                    app.drag_last = (mx, my);
                    app.window.request_redraw();
                } else {
                    // Update hover highlights
                    app.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let (mx, my) = app.mouse_pos;
                // Check if the press lands on a button — if so, it's a click, not a drag.
                let over_button = {
                    let win_h = app.renderer.size().height as f32;
                    let rects = button_rects(win_h);
                    rects.iter().any(|&(x, y, bw, bh)| {
                        mx >= x && mx <= x + bw && my >= y && my <= y + bh
                    })
                };

                if over_button {
                    app.handle_button_click(mx, my);
                } else {
                    app.is_dragging = true;
                    app.drag_last = (mx, my);
                    app.window.set_cursor(CursorIcon::Grabbing);
                }
                app.window.request_redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                if app.is_dragging {
                    app.is_dragging = false;
                    app.window.set_cursor(CursorIcon::Default);
                    app.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let now = std::time::Instant::now();
                let dt = (now - self.last_frame_time).as_secs_f32();
                self.last_frame_time = now;

                app.update(dt);

                match app.render() {
                    Ok(_) => {}
                    Err(wgpu::SurfaceError::Lost) => {}
                    Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(app) = &mut self.app else { return };

        // Drain solver channel
        loop {
            match app.move_receiver.try_recv() {
                Ok(m) => {
                    app.pending_moves.push(m);
                    app.moves_total += 1;
                    // First move arrives → solver found something → switch to Animating
                    if app.phase == SolvePhase::Solving {
                        app.phase = SolvePhase::Animating;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Solver thread finished (or was cancelled)
                    if app.phase == SolvePhase::Solving {
                        app.phase = if !app.pending_moves.is_empty() || app.is_animating {
                            SolvePhase::Animating
                        } else {
                            SolvePhase::Idle
                        };
                    }
                    app.channel_done = true;
                    break;
                }
            }
        }

        // When animation finishes naturally, return to Idle
        if app.phase == SolvePhase::Animating
            && !app.is_animating
            && app.pending_moves.is_empty()
            && app.channel_done
        {
            app.phase = SolvePhase::Idle;
        }

        /*
        BUG - FRZ4
        if app.is_active() {
            app.window.request_redraw();
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        } else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        }*/

        app.window.request_redraw();
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    }
}

// ─────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();
    let mut app_state = AppState {
        app: None,
        window_id: None,
        last_frame_time: std::time::Instant::now(),
    };

    event_loop.run_app(&mut app_state).unwrap();
}
