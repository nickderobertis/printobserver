# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.17.0](https://github.com/nickderobertis/printobserver/compare/v0.16.1...v0.17.0) - 2026-11-28

### Added

- *(status)* show the operator when check images fall back to the plain frame, and why ([#411](https://github.com/nickderobertis/printobserver/pull/411))
- *(supervisor)* recover camera alignment and drift from a shifted view, and say why when the plain frame is shown ([#397](https://github.com/nickderobertis/printobserver/pull/397))

### Fixed

- *(test)* gate the server journeys on their feature again and drive the inconclusive fallback deterministically ([#412](https://github.com/nickderobertis/printobserver/pull/412))
## [0.16.1](https://github.com/nickderobertis/printobserver/compare/v0.16.0...v0.16.1) - 2026-11-26

### Added

- *(supervisor)* [**breaking**] supervise planned filament colour changes ([#384](https://github.com/nickderobertis/printobserver/pull/384))
## [0.16.0](https://github.com/nickderobertis/printobserver/compare/v0.15.5...v0.16.0) - 2026-11-26

### Added

- *(supervisor)* [**breaking**] supervise planned filament colour changes ([#384](https://github.com/nickderobertis/printobserver/pull/384))
## [0.15.5](https://github.com/nickderobertis/printobserver/compare/v0.15.4...v0.15.5) - 2026-11-25

### Fixed

- *(test)* close the print only once the journey's print is open ([#377](https://github.com/nickderobertis/printobserver/pull/377))
## [0.15.4](https://github.com/nickderobertis/printobserver/compare/v0.15.3...v0.15.4) - 2026-11-25

### Fixed

- *(cli)* print a served --json command's document every time ([#378](https://github.com/nickderobertis/printobserver/pull/378))
## [0.15.3](https://github.com/nickderobertis/printobserver/compare/v0.15.2...v0.15.3) - 2026-11-25

### Fixed

- *(cli)* print a served --json command's document every time ([#378](https://github.com/nickderobertis/printobserver/pull/378))
## [0.15.2](https://github.com/nickderobertis/printobserver/compare/v0.15.0...v0.15.2) - 2026-11-25

### Fixed

- *(test)* open a composite check's store only once the server has released it ([#375](https://github.com/nickderobertis/printobserver/pull/375))
## [0.15.0](https://github.com/nickderobertis/printobserver/compare/v0.14.4...v0.15.0) - 2026-11-24

### Fixed

- *(agent)* decide each check on its own image, with no look, so a moved part is paused ([#364](https://github.com/nickderobertis/printobserver/pull/364))

### Performance

- *(supervisor)* [**breaking**] cut each check's input tokens, on Claude Code's prompt replaced and oneharness's schema ([#363](https://github.com/nickderobertis/printobserver/pull/363))
## [0.14.4](https://github.com/nickderobertis/printobserver/compare/v0.14.3...v0.14.4) - 2026-11-22

### Fixed

- *(ci)* measure a re-run attempt's check time and job count correctly ([#343](https://github.com/nickderobertis/printobserver/pull/343))
## [0.14.3](https://github.com/nickderobertis/printobserver/compare/v0.14.0...v0.14.3) - 2026-11-22

### Fixed

- *(ci)* measure a re-run attempt's check time and job count correctly ([#343](https://github.com/nickderobertis/printobserver/pull/343))
## [0.14.0](https://github.com/nickderobertis/printobserver/compare/v0.13.1...v0.14.0) - 2026-11-21

### Added

- *(skill)* require an outside supervisor's own frames at set points ([#353](https://github.com/nickderobertis/printobserver/pull/353))
- *(supervisor)* compact each print's conversation while idle, and at 200k as a backstop ([#352](https://github.com/nickderobertis/printobserver/pull/352))
- *(supervisor)* [**breaking**] put each check's frame and facts in its prompt, on a lean harness ([#349](https://github.com/nickderobertis/printobserver/pull/349))
- report Obico scoring as detector status and add bed-clear ([#327](https://github.com/nickderobertis/printobserver/pull/327))

## [0.13.1](https://github.com/nickderobertis/printobserver/compare/v0.12.0...v0.13.1) - 2026-11-16

### Added

- *(context)* tell the agent the layer, height and features it prints ([#329](https://github.com/nickderobertis/printobserver/pull/329))

## [0.13.0](https://github.com/nickderobertis/printobserver/compare/v0.12.0...v0.13.0) - 2026-11-16

### Added

- *(context)* tell the agent the layer, height and features it prints ([#329](https://github.com/nickderobertis/printobserver/pull/329))

## [0.12.0](https://github.com/nickderobertis/printobserver/compare/v0.11.1...v0.12.0) - 2026-11-07

### Added

- *(preflight)* [**breaking**] refuse a start whose material or build sheet is wrong ([#317](https://github.com/nickderobertis/printobserver/pull/317))

## [0.11.1](https://github.com/nickderobertis/printobserver/compare/v0.10.5...v0.11.1) - 2026-11-03

### Added

- *(look)* frames with no print, camera 5xx retries, frames on checks ([#316](https://github.com/nickderobertis/printobserver/pull/316))

## [0.11.0](https://github.com/nickderobertis/printobserver/compare/v0.10.5...v0.11.0) - 2026-11-03

### Added

- *(look)* frames with no print, camera 5xx retries, frames on checks ([#316](https://github.com/nickderobertis/printobserver/pull/316))

## [0.10.5](https://github.com/nickderobertis/printobserver/compare/v0.10.3...v0.10.5) - 2026-11-02

### Added

- *(install)* make a per-user, no-admin install the recommended path on Linux and macOS ([#296](https://github.com/nickderobertis/printobserver/pull/296))

## [0.10.4](https://github.com/nickderobertis/printobserver/compare/v0.10.3...v0.10.4) - 2026-11-02

### Added

- *(install)* make a per-user, no-admin install the recommended path on Linux and macOS ([#296](https://github.com/nickderobertis/printobserver/pull/296))

## [0.10.3](https://github.com/nickderobertis/printobserver/compare/v0.10.1...v0.10.3) - 2026-10-28

### Added

- *(install)* make a per-user, no-admin install the recommended path on Windows ([#293](https://github.com/nickderobertis/printobserver/pull/293))

## [0.10.2](https://github.com/nickderobertis/printobserver/compare/v0.10.1...v0.10.2) - 2026-10-28

### Added

- *(install)* make a per-user, no-admin install the recommended path on Windows ([#293](https://github.com/nickderobertis/printobserver/pull/293))

## [0.10.1](https://github.com/nickderobertis/printobserver/compare/v0.9.0...v0.10.1) - 2026-10-27

### Added

- *(recover)* move the printer after a failure through a closed, checked recovery vocabulary ([#275](https://github.com/nickderobertis/printobserver/pull/275))

## [0.10.0](https://github.com/nickderobertis/printobserver/compare/v0.9.0...v0.10.0) - 2026-10-27

### Added

- *(recover)* move the printer after a failure through a closed, checked recovery vocabulary ([#275](https://github.com/nickderobertis/printobserver/pull/275))

## [0.9.0](https://github.com/nickderobertis/printobserver/compare/v0.8.1...v0.9.0) - 2026-10-27

### Added

- *(supervision)* [**breaking**] record resets, disconnects, unattended heating, detector outages and blank frames ([#272](https://github.com/nickderobertis/printobserver/pull/272))

## [0.8.1](https://github.com/nickderobertis/printobserver/compare/v0.7.1...v0.8.1) - 2026-10-26

### Added

- *(prints)* close a stale print record by operator command ([#261](https://github.com/nickderobertis/printobserver/pull/261))

## [0.8.0](https://github.com/nickderobertis/printobserver/compare/v0.7.1...v0.8.0) - 2026-10-26

### Added

- *(prints)* close a stale print record by operator command ([#261](https://github.com/nickderobertis/printobserver/pull/261))

## [0.7.1](https://github.com/nickderobertis/printobserver/compare/v0.6.1...v0.7.1) - 2026-10-26

### Added

- *(spools)* [**breaking**] make incoherent spool and charge records unrepresentable ([#260](https://github.com/nickderobertis/printobserver/pull/260))

## [0.7.0](https://github.com/nickderobertis/printobserver/compare/v0.6.1...v0.7.0) - 2026-10-26

### Added

- *(spools)* [**breaking**] make incoherent spool and charge records unrepresentable ([#260](https://github.com/nickderobertis/printobserver/pull/260))

## [0.6.1](https://github.com/nickderobertis/printobserver/compare/v0.5.0...v0.6.1) - 2026-10-26

### Added

- *(watch)* stream the printer's events, filtered, through one watch command ([#258](https://github.com/nickderobertis/printobserver/pull/258))

## [0.6.0](https://github.com/nickderobertis/printobserver/compare/v0.5.0...v0.6.0) - 2026-10-26

### Added

- *(watch)* stream the printer's events, filtered, through one watch command ([#258](https://github.com/nickderobertis/printobserver/pull/258))

## [0.5.0](https://github.com/nickderobertis/printobserver/compare/v0.4.3...v0.5.0) - 2026-10-26

### Fixed

- *(printer)* [**breaking**] refuse malformed replies, keep unknown states and keep bounds across clock steps ([#262](https://github.com/nickderobertis/printobserver/pull/262))

## [0.4.3](https://github.com/nickderobertis/printobserver/compare/v0.4.1...v0.4.3) - 2026-10-16

### Fixed

- record a cancel under the detectors' pause in a deterministic order ([#233](https://github.com/nickderobertis/printobserver/pull/233))

## [0.4.2](https://github.com/nickderobertis/printobserver/compare/v0.4.1...v0.4.2) - 2026-10-16

### Fixed

- record a cancel under the detectors' pause in a deterministic order ([#233](https://github.com/nickderobertis/printobserver/pull/233))

## [0.4.0](https://github.com/nickderobertis/printobserver/compare/v0.3.0...v0.4.0) - 2026-10-12

### Added

- *(spools)* [**breaking**] serve spool commands, printer history and preflight ([#190](https://github.com/nickderobertis/printobserver/pull/190))
- *(octoprint)* read job filament, file tallies and G-code extrusion ([#181](https://github.com/nickderobertis/printobserver/pull/181))
- *(spools)* [**breaking**] declare spool records, events and printer-port reads ([#178](https://github.com/nickderobertis/printobserver/pull/178))
- *(supervisor)* camera looks, one agent per print, and a resume after an adjustment under the detector's pause ([#147](https://github.com/nickderobertis/printobserver/pull/147))

### Fixed

- *(release)* release every crate together in one version group ([#189](https://github.com/nickderobertis/printobserver/pull/189))
- *(api)* [**breaking**] bind each request to its credential and keep only the operator credential's verifier ([#165](https://github.com/nickderobertis/printobserver/pull/165))
- *(prints)* close ended jobs outside turns and open a new record for a later job ([#163](https://github.com/nickderobertis/printobserver/pull/163))
- *(supervisor)* let supervision turns act on Windows through printobserver's own commands ([#146](https://github.com/nickderobertis/printobserver/pull/146))

## [0.3.0](https://github.com/nickderobertis/printobserver/compare/v0.2.0...v0.3.0) - 2026-09-27

### Added

- *(docs)* read every document as a Windows and a macOS user ([#85](https://github.com/nickderobertis/printobserver/pull/85))
- *(platforms)* bring macOS on Apple silicon up as a first-class platform ([#58](https://github.com/nickderobertis/printobserver/pull/58))
- *(windows)* run printobserver as a Windows service ([#70](https://github.com/nickderobertis/printobserver/pull/70))
- *(printer)* name, open and refuse a serial device the way each platform does ([#68](https://github.com/nickderobertis/printobserver/pull/68))
- *(windows)* bring the gate and the printer tier up on Windows ([#57](https://github.com/nickderobertis/printobserver/pull/57))
- *(api)* list prints and adopt the job OctoPrint is running ([#42](https://github.com/nickderobertis/printobserver/pull/42))
- *(server)* [**breaking**] require the documented API credential on every request ([#37](https://github.com/nickderobertis/printobserver/pull/37))
- *(service)* sign the agent's harness in once as the service user ([#38](https://github.com/nickderobertis/printobserver/pull/38))
- *(crates)* [**breaking**] split the crate graph by domain and open the event log ([#29](https://github.com/nickderobertis/printobserver/pull/29))

### Fixed

- *(skill)* [**breaking**] ship the agent skill through gh skill rather than the crate ([#118](https://github.com/nickderobertis/printobserver/pull/118))
