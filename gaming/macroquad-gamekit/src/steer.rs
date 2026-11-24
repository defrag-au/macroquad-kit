//! `Steer` — the mouse, a trackpad and fingers read as a camera's intents: carry the ground under the pointer, zoom toward it, orbit; as data, so any camera (shared-crates' `view-camera`, a globe) takes them.
//!
//! | Input | Intent |
//! |---|---|
//! | left button, or one finger | grab the ground and carry it ([`Steering::Grab`], [`Steering::Drag`], [`Steering::Release`]) |
//! | right button, or shift with the left | orbit: across turns, up and down tilts ([`Steering::Orbit`]) |
//! | wheel, or a trackpad's two-finger scroll | zoom toward the pointer, a step a notch's worth of points ([`Steering::Zoom`]) |
//! | two fingers | zoom toward their middle as they spread, turn as they twist, tilt as they move up or down together |
//!
//! ```ignore
//! for s in steer.read(vec2(screen_width(), screen_height())) {
//!     match s {
//!         Steering::Grab(at) => camera.grab(at),
//!         Steering::Drag(at) => camera.drag(at),
//!         Steering::Release => camera.release(),
//!         Steering::Zoom { steps, at } => camera.zoom(steps, at),
//!         Steering::Orbit { turn, tilt } => camera.orbit(turn, tilt),
//!     }
//! }
//! ```
//!
//! Points are in normalised device coordinates: −1 to 1 across the screen,
//! y up. Turn is radians clockwise seen from above (a drag across the whole
//! screen is half a turn); tilt is radians toward looking down (a drag up the
//! whole screen is a quarter turn). Held keys are the client's: games bind
//! their own, and a camera's `Drive` takes them.
//!
//! The mapping is `unify-space`'s globe viewer's, lifted out so a flat scene
//! and the globe steer alike.

use macroquad::prelude::*;

use crate::wheel_points;

/// One intent read from this frame's input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Steering {
    /// Take hold of the ground under this point.
    Grab(Vec2),
    /// The held ground follows the pointer here.
    Drag(Vec2),
    /// Let go.
    Release,
    /// Zoom in by `steps` (out, negative), toward `at` where given.
    Zoom { steps: f32, at: Option<Vec2> },
    /// Turn and tilt about the middle, radians.
    Orbit { turn: f32, tilt: f32 },
}

#[derive(Clone, Copy, Debug)]
struct Pinch {
    middle: Vec2,
    apart: f32,
    angle: f32,
}

/// Points of wheel to a step of zoom.
const POINTS_PER_STEP: f32 = 40.0;

/// What the pointer was doing last frame, so this frame's input reads as a
/// change.
#[derive(Clone, Debug, Default)]
pub struct Steer {
    holding: bool,
    orbiting: Option<Vec2>,
    pinch: Option<Pinch>,
}

impl Steer {
    pub fn new() -> Self {
        Self::default()
    }

    /// This frame's intents, from macroquad's input, on a screen `size`
    /// pixels (the units `mouse_position` reports in).
    pub fn read(&mut self, size: Vec2) -> Vec<Steering> {
        let fingers: Vec<Vec2> = touches()
            .into_iter()
            .filter(|t| !matches!(t.phase, TouchPhase::Ended | TouchPhase::Cancelled))
            .map(|t| t.position)
            .collect();
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        self.step(Input {
            size,
            pointer: Vec2::from(mouse_position()),
            left: is_mouse_button_down(MouseButton::Left),
            right: is_mouse_button_down(MouseButton::Right),
            shift,
            fingers: &fingers,
            wheel_points: wheel_points(mouse_wheel().1),
        })
    }

    /// Let go of anything held: the window lost focus, or the client took
    /// the pointer for something else.
    pub fn reset(&mut self) -> Vec<Steering> {
        self.orbiting = None;
        self.pinch = None;
        if std::mem::take(&mut self.holding) {
            vec![Steering::Release]
        } else {
            Vec::new()
        }
    }

    fn step(&mut self, input: Input) -> Vec<Steering> {
        let mut out = Vec::new();
        let size = input.size.max(Vec2::ONE);
        let ndc = |p: Vec2| Vec2::new(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0);

        // Two fingers: a pinch, and nothing else.
        if let [a, b, ..] = input.fingers[..] {
            if std::mem::take(&mut self.holding) {
                out.push(Steering::Release);
            }
            self.orbiting = None;
            let now = Pinch {
                middle: (a + b) * 0.5,
                apart: a.distance(b).max(1.0),
                angle: (b.y - a.y).atan2(b.x - a.x),
            };
            if let Some(last) = self.pinch {
                // A step of zoom is a fifth of the view (`view-camera`).
                let steps = (now.apart / last.apart).ln() / 0.2;
                if steps.abs() > 1e-4 {
                    out.push(Steering::Zoom {
                        steps,
                        at: Some(ndc(now.middle)),
                    });
                }
                let twist = (now.angle - last.angle + std::f32::consts::PI)
                    .rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                let tilt = (now.middle.y - last.middle.y) / size.y * std::f32::consts::PI;
                out.push(Steering::Orbit {
                    turn: -twist,
                    tilt: -tilt,
                });
            }
            self.pinch = Some(now);
            return out;
        }
        self.pinch = None;

        // The right button, or shift with the left: turn and tilt.
        if input.right || (input.left && input.shift) {
            if std::mem::take(&mut self.holding) {
                out.push(Steering::Release);
            }
            if let Some(last) = self.orbiting {
                let d = input.pointer - last;
                if d != Vec2::ZERO {
                    out.push(Steering::Orbit {
                        turn: d.x / size.x * std::f32::consts::PI,
                        tilt: d.y / size.y * std::f32::consts::FRAC_PI_2,
                    });
                }
            }
            self.orbiting = Some(input.pointer);
        } else {
            self.orbiting = None;
            // The left button (or one finger): carry the ground.
            match (input.left, self.holding) {
                (true, false) => {
                    self.holding = true;
                    out.push(Steering::Grab(ndc(input.pointer)));
                }
                (true, true) => out.push(Steering::Drag(ndc(input.pointer))),
                (false, true) => {
                    self.holding = false;
                    out.push(Steering::Release);
                }
                (false, false) => {}
            }
        }

        // The wheel: a notch (or its worth of a trackpad's points) is a step
        // of zoom, toward the pointer.
        if input.wheel_points != 0.0 {
            out.push(Steering::Zoom {
                steps: input.wheel_points / POINTS_PER_STEP,
                at: Some(ndc(input.pointer)),
            });
        }
        out
    }
}

/// One frame's input, as [`Steer::step`] reads it.
struct Input<'a> {
    size: Vec2,
    pointer: Vec2,
    left: bool,
    right: bool,
    shift: bool,
    fingers: &'a [Vec2],
    wheel_points: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(pointer: Vec2, left: bool) -> Input<'static> {
        Input {
            size: Vec2::new(800.0, 600.0),
            pointer,
            left,
            right: false,
            shift: false,
            fingers: &[],
            wheel_points: 0.0,
        }
    }

    #[test]
    fn a_press_grabs_a_move_drags_a_lift_releases() {
        let mut s = Steer::new();
        assert_eq!(
            s.step(input(Vec2::new(400.0, 300.0), true)),
            vec![Steering::Grab(Vec2::ZERO)]
        );
        assert_eq!(
            s.step(input(Vec2::new(600.0, 150.0), true)),
            vec![Steering::Drag(Vec2::new(0.5, 0.5))]
        );
        assert_eq!(
            s.step(input(Vec2::new(600.0, 150.0), false)),
            vec![Steering::Release]
        );
        assert!(s.step(input(Vec2::new(600.0, 150.0), false)).is_empty());
    }

    #[test]
    fn the_wheel_zooms_toward_the_pointer() {
        let mut s = Steer::new();
        let mut i = input(Vec2::new(0.0, 0.0), false);
        i.wheel_points = 80.0;
        assert_eq!(
            s.step(i),
            vec![Steering::Zoom {
                steps: 2.0,
                at: Some(Vec2::new(-1.0, 1.0)),
            }]
        );
    }

    #[test]
    fn shift_drag_orbits_and_lets_go_of_a_hold() {
        let mut s = Steer::new();
        s.step(input(Vec2::new(400.0, 300.0), true));
        let mut i = input(Vec2::new(400.0, 300.0), true);
        i.shift = true;
        assert_eq!(s.step(i), vec![Steering::Release]);
        let mut i = input(Vec2::new(800.0, 300.0), true);
        i.shift = true;
        assert_eq!(
            s.step(i),
            vec![Steering::Orbit {
                turn: std::f32::consts::FRAC_PI_2,
                tilt: 0.0,
            }]
        );
    }

    #[test]
    fn two_fingers_spreading_zoom_toward_their_middle() {
        let mut s = Steer::new();
        let mut i = input(Vec2::ZERO, false);
        let near = [Vec2::new(300.0, 300.0), Vec2::new(500.0, 300.0)];
        i.fingers = &near;
        assert!(s.step(i).is_empty());
        let mut i = input(Vec2::ZERO, false);
        let far = [Vec2::new(200.0, 300.0), Vec2::new(600.0, 300.0)];
        i.fingers = &far;
        let out = s.step(i);
        let Steering::Zoom { steps, at } = out[0] else {
            panic!("{out:?}");
        };
        assert!((steps - 2f32.ln() / 0.2).abs() < 1e-4, "{steps}");
        assert_eq!(at, Some(Vec2::ZERO));
    }
}
