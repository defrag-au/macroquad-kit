//! The mouse cursor over something that can be pressed: a pointer, as the web shows one.
//!
//! A canvas has no elements for the browser to give a `cursor: pointer`, so nothing an app drew
//! ever showed one (reported 2026-11-15: readers could not tell a button from a label). Every
//! press a widget offers goes through [`Painter::interact`](crate::painter::Painter::interact) or
//! [`Painter::tapped`](crate::painter::Painter::tapped); each marks the frame when the mouse is
//! over its rect, and the frame loop ([`finish_frame`]) sets the cursor once from that. A host
//! that runs its own loop calls [`finish_frame`] after drawing.
//!
//! Set only on a change: on the web it is a style write, and natively a system call.

use std::cell::Cell;

use macroquad::miniquad::{CursorIcon, window};

thread_local! {
    /// Something under the mouse could be pressed this frame.
    static WANTED: Cell<bool> = const { Cell::new(false) };
    /// What the cursor was last set to: `true` the pointer.
    static SHOWN: Cell<bool> = const { Cell::new(false) };
}

/// The mouse is over something that can be pressed: show the pointer this frame.
pub fn want_pointer() {
    WANTED.with(|wanted| wanted.set(true));
}

/// Set the cursor from what this frame drew, and start the next frame's reading. Once a frame,
/// after drawing.
pub fn finish_frame() {
    let wanted = WANTED.with(|wanted| wanted.replace(false));
    let changed = SHOWN.with(|shown| shown.replace(wanted) != wanted);
    if changed {
        window::set_mouse_cursor(match wanted {
            true => CursorIcon::Pointer,
            false => CursorIcon::Default,
        });
    }
}
