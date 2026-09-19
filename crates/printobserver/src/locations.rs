//! Where this program keeps its configuration, its state and itself, per platform.
//!
//! Three defaults, and one answer for each platform: the configuration file a
//! command reads when nothing names one, the state directory the installed
//! service keeps its record in, and the directory the installed program lives
//! in. Each platform's service installer writes exactly these, so a caller on
//! the host beside the printer needs no option at all.
//!
//! This module is the one place those answers are spelled. [`HERE`] is the one
//! this build answers with, chosen by its target; every other copy — the Linux
//! and Windows installers' own paths and the real-printer smoke test's
//! configuration defaults among them — is held to [`LINUX`], [`MACOS`] or
//! [`WINDOWS`] by this crate's tests.

/// The three places one platform's install keeps this program and what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locations {
    /// The configuration file read when no command names one.
    pub config: &'static str,
    /// The state directory the installed service keeps its record in.
    pub state: &'static str,
    /// The directory the installed program lives in.
    pub home: &'static str,
}

/// Where an install keeps them on Linux, under the filesystem hierarchy.
pub const LINUX: Locations = Locations {
    config: "/etc/printobserver/config.toml",
    state: "/var/lib/printobserver",
    home: "/usr/local/lib/printobserver",
};

/// Where an install keeps them on macOS.
///
/// Provisional: this is the Linux answer, byte for byte, which is what macOS
/// has always been given. Choosing macOS's own — and making its installer's
/// placement agree with it — is the `macos-first-class` node's, which changes
/// these values and nothing else here.
pub const MACOS: Locations = Locations {
    config: "/etc/printobserver/config.toml",
    state: "/var/lib/printobserver",
    home: "/usr/local/lib/printobserver",
};

/// Where an install keeps them on Windows.
///
/// Machine-wide data belongs under `ProgramData` and programs under
/// `Program Files`, which is where a Windows service's own files are looked for.
pub const WINDOWS: Locations = Locations {
    config: r"C:\ProgramData\printobserver\config.toml",
    state: r"C:\ProgramData\printobserver\state",
    home: r"C:\Program Files\printobserver",
};

/// The answer this build gives: its own platform's.
#[cfg(windows)]
pub const HERE: Locations = WINDOWS;

/// The answer this build gives: its own platform's.
#[cfg(target_os = "macos")]
pub const HERE: Locations = MACOS;

/// The answer this build gives: its own platform's, and the Linux answer on any
/// other Unix.
#[cfg(not(any(windows, target_os = "macos")))]
pub const HERE: Locations = LINUX;

#[cfg(test)]
mod tests {
    use super::{HERE, LINUX, Locations, MACOS, WINDOWS};

    /// The value one assignment in the Linux installer makes, as it writes it.
    fn installer_value(installer: &str, variable: &str) -> String {
        installer
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{variable}=\"")))
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or_else(|| panic!("the installer assigns no {variable}"))
            .replace("$PROGRAM", "printobserver")
    }

    /// Linux keeps the answers this program has always given.
    #[test]
    fn linux_keeps_the_filesystem_hierarchy_answers() {
        assert_eq!(
            LINUX,
            Locations {
                config: "/etc/printobserver/config.toml",
                state: "/var/lib/printobserver",
                home: "/usr/local/lib/printobserver",
            }
        );
    }

    /// macOS is given the Linux answer until its own is chosen.
    #[test]
    fn macos_is_given_the_linux_answer_until_its_own_is_chosen() {
        assert_eq!(MACOS, LINUX);
    }

    /// Windows keeps data under `ProgramData` and the program under `Program Files`.
    #[test]
    fn windows_keeps_data_under_programdata_and_the_program_under_program_files() {
        assert_eq!(
            WINDOWS,
            Locations {
                config: r"C:\ProgramData\printobserver\config.toml",
                state: r"C:\ProgramData\printobserver\state",
                home: r"C:\Program Files\printobserver",
            }
        );
    }

    /// This build answers with its own platform's locations, and the
    /// configuration default a command reads is that answer's.
    #[test]
    fn this_build_answers_with_its_own_platforms_locations() {
        let expected = if cfg!(windows) {
            WINDOWS
        } else if cfg!(target_os = "macos") {
            MACOS
        } else {
            LINUX
        };
        assert_eq!(HERE, expected);
        assert_eq!(crate::config::DEFAULT_CONFIG_PATH, expected.config);
    }

    /// The value one assignment in the Windows installer makes, as it writes it:
    /// `$Name = 'value'`.
    fn windows_installer_value(installer: &str, variable: &str) -> String {
        installer
            .lines()
            .find_map(|line| line.strip_prefix(&format!("${variable} = '")))
            .and_then(|rest| rest.strip_suffix('\''))
            .unwrap_or_else(|| panic!("the Windows installer assigns no {variable}"))
            .to_owned()
    }

    /// The Windows installer writes the service to exactly the Windows answer.
    #[test]
    fn the_windows_installer_writes_the_windows_answer() {
        let installer = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/install-service.ps1"
        ))
        .expect("the committed Windows installer reads");

        assert_eq!(
            windows_installer_value(&installer, "ConfigPath"),
            WINDOWS.config
        );
        assert_eq!(
            windows_installer_value(&installer, "StateDirectory"),
            WINDOWS.state
        );
        assert_eq!(
            windows_installer_value(&installer, "ProgramDirectory"),
            WINDOWS.home
        );
    }

    /// The value one module-level assignment in the smoke test makes, as it
    /// writes it: a plain or a raw Python string literal.
    fn smoke_value(smoke: &str, constant: &str) -> String {
        smoke
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{constant} = ")))
            .and_then(|rest| rest.strip_prefix('r').or(Some(rest)))
            .and_then(|rest| rest.strip_prefix('"'))
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or_else(|| panic!("the smoke test assigns no {constant}"))
            .to_owned()
    }

    /// The real-printer smoke test reads the supervisor's own configuration,
    /// so the file it defaults to on each platform is the one that platform's
    /// installer writes.
    #[test]
    fn the_smoke_test_defaults_to_each_platforms_own_configuration() {
        let smoke = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/printer-smoke/printer_smoke.py"
        ))
        .expect("the committed smoke test reads");

        assert_eq!(smoke_value(&smoke, "LINUX_CONFIG"), LINUX.config);
        assert_eq!(smoke_value(&smoke, "MACOS_CONFIG"), MACOS.config);
        assert_eq!(smoke_value(&smoke, "WINDOWS_CONFIG"), WINDOWS.config);
    }

    /// The Linux installer writes the service to exactly the Linux answer.
    #[test]
    fn the_linux_installer_writes_the_linux_answer() {
        let installer = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/install-service.sh"
        ))
        .expect("the committed installer reads");

        assert_eq!(installer_value(&installer, "RUNTIME_CONFIG"), LINUX.config);
        assert_eq!(installer_value(&installer, "RUNTIME_STATE"), LINUX.state);
        assert_eq!(
            installer_value(&installer, "RUNTIME_BINARY"),
            format!("{}/printobserver", LINUX.home)
        );
    }
}
