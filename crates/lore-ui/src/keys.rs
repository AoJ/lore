//! All keyboard shortcuts in one place.
//! Format: (key_char, requires_ctrl). Cmd/Shift are checked at the call site.

// Navigation in list
pub const NAV_DOWN: (&str, bool) = ("j", true); // Ctrl+J
pub const NAV_UP: (&str, bool) = ("k", true); // Ctrl+K
