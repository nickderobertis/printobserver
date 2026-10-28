//! The console a Windows server lets go of.
//!
//! Windows gives a console program a console when it starts, and when nothing
//! that started it had one to share — which is how Task Scheduler starts the
//! per-user server at logon — that console is a window of its own on the
//! user's desktop for as long as the program runs. A server needs no window:
//! everything it says, it says to the clients that ask. So the server lets a
//! console go when this process is the only one attached to it, and keeps one
//! it shares with the shell that started it, whose window is the person's own.

use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleProcessList};

/// Let go of this process's console when it was made for this process alone.
///
/// One process attached is this one: a shell's console has the shell attached
/// too. No console at all answers zero, and nothing is let go.
// Two Win32 calls nothing safe wraps; suppressions.toml has the reason.
#[expect(unsafe_code)]
pub fn release_a_console_of_its_own() {
    let mut attached = [0_u32; 2];
    let length = u32::try_from(attached.len()).unwrap_or(0);
    // SAFETY: `attached` is a writable buffer of `length` process identifiers,
    // which is the most `GetConsoleProcessList` writes into it.
    let count = unsafe { GetConsoleProcessList(attached.as_mut_ptr(), length) };
    if count != 1 {
        return;
    }
    // SAFETY: `FreeConsole` takes nothing and only detaches this process from
    // its console; every standard handle after it writes nowhere, which the
    // standard library's own streams read as written.
    unsafe {
        FreeConsole();
    }
}
