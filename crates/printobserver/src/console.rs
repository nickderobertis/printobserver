//! The console window a Windows server hides.
//!
//! Windows gives a console program a console when it starts, and when nothing
//! that started it had one to share — which is how Task Scheduler starts the
//! per-user server at logon — that console is a window of its own on the
//! user's desktop for as long as the program runs. A server needs no window:
//! everything it says, it says to the clients that ask. So the server hides a
//! console's window when this process is the only one attached to it, and
//! leaves alone one it shares with the shell that started it, whose window is
//! the person's own. The console itself stays: a console control event, which
//! is how a console server is asked to stop, still reaches it.

use windows_sys::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

/// Hide this process's console window when the console was made for this
/// process alone.
///
/// One process attached is this one: a shell's console has the shell attached
/// too. No console at all answers zero, and nothing is hidden.
// Three Win32 calls nothing safe wraps; suppressions.toml has the reason.
#[expect(unsafe_code)]
pub fn hide_a_console_of_its_own() {
    let mut attached = [0_u32; 2];
    let length = u32::try_from(attached.len()).unwrap_or(0);
    // SAFETY: `attached` is a writable buffer of `length` process identifiers,
    // which is the most `GetConsoleProcessList` writes into it.
    let count = unsafe { GetConsoleProcessList(attached.as_mut_ptr(), length) };
    if count != 1 {
        return;
    }
    // SAFETY: `GetConsoleWindow` takes nothing and answers this process's
    // console window, or null where the console has none.
    let window = unsafe { GetConsoleWindow() };
    if window.is_null() {
        return;
    }
    // SAFETY: `window` is the console window Windows just answered for this
    // process; hiding it changes nothing but whether it is shown.
    unsafe {
        ShowWindow(window, SW_HIDE);
    }
}
