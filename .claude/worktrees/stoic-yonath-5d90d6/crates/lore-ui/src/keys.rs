/// All keyboard shortcuts in one place.
/// Format: (key_char, requires_ctrl, requires_cmd, requires_alt)

// Navigation in list
pub const NAV_DOWN: (&str, bool) = ("j", true);       // Ctrl+J
pub const NAV_UP: (&str, bool) = ("k", true);         // Ctrl+K

// Actions
pub const TRASH_SELECTED: (&str, bool) = ("d", false); // Cmd+D (cmd checked separately)
pub const NEW_NOTE: (&str, bool) = ("n", false);       // Cmd+N
pub const NEW_SPACE: (&str, bool) = ("s", false);      // Cmd+Shift+S (shift checked separately)
pub const NEW_FOLDER: (&str, bool) = ("f", false);     // Cmd+Shift+F

// Space switching (Ctrl+1..9 for first 9 spaces)
pub const SPACE_PREFIX_CTRL: bool = true; // Ctrl+1, Ctrl+2, etc.
