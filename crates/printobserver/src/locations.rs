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
//! and Windows installers' own paths, the real-printer smoke test's
//! configuration defaults and the README's table of them among them — is held
//! to [`LINUX`], [`MACOS`] or [`WINDOWS`] by this crate's tests.
//!
//! # The operator's own configuration
//!
//! A fourth answer is not the install's but the operator's: the client
//! configuration `printobserver credential issue` writes and every command
//! reads first, at [`OPERATOR_CLIENT_CONFIG`] under the operator's own
//! configuration home. Where that home is is each platform's own convention,
//! read from that platform's own variables by [`config_home_of`] — `XDG_CONFIG_HOME`,
//! else `$HOME/.config`, on Linux and every other Unix;
//! `$HOME/Library/Application Support` on macOS; `%APPDATA%` on Windows.

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
/// The Linux answer, byte for byte: `scripts/install-service.sh` serves both
/// platforms and puts things in the same places on each, so an operator moving
/// between the two has one set of paths to know.
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

/// The family of platform a configuration home is resolved for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Linux, and every Unix that is not macOS: the XDG base directories.
    Unix,
    /// macOS: the user's `Library/Application Support`.
    MacOs,
    /// Windows: the roaming application data folder.
    Windows,
}

/// The family this build resolves for.
#[cfg(windows)]
pub const FAMILY: Family = Family::Windows;

/// The family this build resolves for.
#[cfg(target_os = "macos")]
pub const FAMILY: Family = Family::MacOs;

/// The family this build resolves for.
#[cfg(not(any(windows, target_os = "macos")))]
pub const FAMILY: Family = Family::Unix;

/// The operator's own client configuration, relative to their configuration
/// home.
pub const OPERATOR_CLIENT_CONFIG: [&str; 2] = ["printobserver", "client.toml"];

/// One variable's value, when it is set to anything but nothing.
fn set_to_something(
    read: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    name: &str,
) -> Option<std::path::PathBuf> {
    read(name)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

/// Where one family keeps a user's configuration, read from that family's own
/// variables through `read`.
///
/// On Unix, `XDG_CONFIG_HOME` when it names an absolute path — the base
/// directory specification has a relative one ignored — and `$HOME/.config`
/// otherwise. On macOS, `$HOME/Library/Application Support`. On Windows,
/// `%APPDATA%`. Absent when the variable it needs is not set.
#[must_use]
pub fn config_home_of(
    family: Family,
    read: &dyn Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    match family {
        // Rooted rather than `is_absolute`, so that the Unix answer reads the
        // same on a Windows host resolving it — where `/srv` has no drive.
        Family::Unix => set_to_something(read, "XDG_CONFIG_HOME")
            .filter(|home| home.has_root())
            .or_else(|| set_to_something(read, "HOME").map(|home| home.join(".config"))),
        Family::MacOs => set_to_something(read, "HOME")
            .map(|home| home.join("Library").join("Application Support")),
        Family::Windows => set_to_something(read, "APPDATA"),
    }
}

/// This user's configuration home, on this platform, from this process's own
/// environment.
#[must_use]
pub fn config_home() -> Option<std::path::PathBuf> {
    config_home_of(FAMILY, &|name| std::env::var_os(name))
}

/// This user's own client configuration: [`OPERATOR_CLIENT_CONFIG`] under
/// [`config_home`].
#[must_use]
pub fn operator_client_config() -> Option<std::path::PathBuf> {
    config_home().map(|home| {
        OPERATOR_CLIENT_CONFIG
            .iter()
            .fold(home, |path, segment| path.join(segment))
    })
}

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
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{FAMILY, Family, HERE, LINUX, Locations, MACOS, WINDOWS, config_home_of};

    /// An environment holding exactly these variables.
    fn holding(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let held: Vec<(String, String)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| {
            held.iter()
                .find(|(named, _)| named == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    /// Linux and every other Unix read `XDG_CONFIG_HOME`, and `$HOME/.config`
    /// when it is unset, empty or relative.
    #[test]
    fn unix_reads_xdg_config_home_and_falls_back_to_dot_config() {
        let resolve = |pairs: &[(&str, &str)]| config_home_of(Family::Unix, &holding(pairs));

        assert_eq!(
            resolve(&[
                ("XDG_CONFIG_HOME", "/srv/operator/config"),
                ("HOME", "/home/op")
            ]),
            Some(PathBuf::from("/srv/operator/config"))
        );
        for unusable in ["", "relative/config"] {
            assert_eq!(
                resolve(&[("XDG_CONFIG_HOME", unusable), ("HOME", "/home/op")]),
                Some(PathBuf::from("/home/op").join(".config")),
                "XDG_CONFIG_HOME={unusable:?} was not passed over"
            );
        }
        assert_eq!(
            resolve(&[("HOME", "/home/op")]),
            Some(PathBuf::from("/home/op").join(".config"))
        );
        assert_eq!(resolve(&[]), None);
    }

    /// macOS keeps a user's configuration under `Library/Application Support`.
    #[test]
    fn macos_reads_library_application_support() {
        assert_eq!(
            config_home_of(
                Family::MacOs,
                &holding(&[("HOME", "/Users/op"), ("XDG_CONFIG_HOME", "/elsewhere")])
            ),
            Some(
                PathBuf::from("/Users/op")
                    .join("Library")
                    .join("Application Support")
            )
        );
        assert_eq!(config_home_of(Family::MacOs, &holding(&[])), None);
    }

    /// Windows keeps it in the roaming application data folder.
    #[test]
    fn windows_reads_appdata() {
        assert_eq!(
            config_home_of(
                Family::Windows,
                &holding(&[
                    ("APPDATA", r"C:\Users\op\AppData\Roaming"),
                    ("HOME", "/home/op")
                ])
            ),
            Some(PathBuf::from(r"C:\Users\op\AppData\Roaming"))
        );
        assert_eq!(config_home_of(Family::Windows, &holding(&[])), None);
    }

    /// This build resolves for its own platform's family.
    #[test]
    fn this_build_resolves_for_its_own_family() {
        let expected = if cfg!(windows) {
            Family::Windows
        } else if cfg!(target_os = "macos") {
            Family::MacOs
        } else {
            Family::Unix
        };
        assert_eq!(FAMILY, expected);
    }

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

    /// One installer serves Linux and macOS, so macOS's answer is Linux's.
    #[test]
    fn macos_keeps_the_linux_answer() {
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

    /// The README tells a reader where their platform's install keeps things
    /// in a table, one row per platform, and each row is exactly the answers
    /// here: the one place a person meets these paths is held to the one place
    /// they are declared.
    #[test]
    fn the_readme_tells_each_platforms_own_locations() {
        let readme =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"))
                .expect("the committed README reads");

        for (platform, locations, program) in [
            ("Linux", LINUX, format!("{}/printobserver", LINUX.home)),
            ("macOS", MACOS, format!("{}/printobserver", MACOS.home)),
            (
                "Windows",
                WINDOWS,
                format!("{}\\printobserver.exe", WINDOWS.home),
            ),
        ] {
            let row = format!(
                "| {platform} | `{}` | `{}` | `{program}` |",
                locations.config, locations.state
            );
            assert!(
                readme.lines().any(|line| line == row),
                "the README's table carries no row `{row}`"
            );
        }
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
