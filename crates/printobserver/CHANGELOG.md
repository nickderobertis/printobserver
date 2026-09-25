# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/nickderobertis/printobserver/compare/v0.2.0...v0.3.0) - 2026-09-25

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
