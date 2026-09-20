<!-- llmlint: ignore[instruction_layer_localized] Reviews of this subtree are routed: `.github/CODEOWNERS` assigns every path, this one included, with `* @nickderobertis`. A run over a diff is handed only the files that changed and that file is not one of them, so a judge of this diff can read this crate's file and not the routing it asks for. -->
# printobserver

What this crate's own suite needs from the host that runs it, where that is not
what every other crate needs.

## The service-manager journey needs Docker on Linux, `sudo` on macOS, and is elsewhere on Windows

`tests/service_manager.rs` activates what the committed installer wrote through
the service manager itself, kills the process and waits for the manager to bring
it back. Under systemd it runs systemd as the first process of a throwaway
container over the host's read-only `/usr`, so the host gains no user, unit or
service. Nothing on macOS isolates a system daemon, so under launchd it runs
through a password-free `sudo`, refuses a Mac that already carries an
installation, and removes what it installed.

It is Unix alone. The same journey over the Windows service is
`tests/repo-e2e/tests/test_service_manager_journey.py`'s `windows-service` back
end, which drives the service control manager through `sc.exe` from a shell
that may register a service, and skips a host that already carries one that is
not its own.
