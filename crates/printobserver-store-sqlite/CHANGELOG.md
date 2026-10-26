# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0](https://github.com/nickderobertis/printobserver/compare/v0.5.0...v0.6.0) - 2026-10-26

### Added

- *(watch)* stream the printer's events, filtered, through one watch command ([#258](https://github.com/nickderobertis/printobserver/pull/258))

## [0.5.0](https://github.com/nickderobertis/printobserver/compare/v0.4.3...v0.5.0) - 2026-10-26

### Fixed

- *(printer)* [**breaking**] refuse malformed replies, keep unknown states and keep bounds across clock steps ([#262](https://github.com/nickderobertis/printobserver/pull/262))

## [0.4.0](https://github.com/nickderobertis/printobserver/compare/v0.3.0...v0.4.0) - 2026-10-12

### Added

- *(store)* [**breaking**] persist spools, charges and the printer timeline ([#180](https://github.com/nickderobertis/printobserver/pull/180))

### Fixed

- *(release)* release every crate together in one version group ([#189](https://github.com/nickderobertis/printobserver/pull/189))
- *(prints)* close ended jobs outside turns and open a new record for a later job ([#163](https://github.com/nickderobertis/printobserver/pull/163))

## [0.3.0](https://github.com/nickderobertis/printobserver/compare/v0.2.0...v0.3.0) - 2026-09-27

### Added

- *(windows)* bring the gate and the printer tier up on Windows ([#57](https://github.com/nickderobertis/printobserver/pull/57))
- *(api)* list prints and adopt the job OctoPrint is running ([#42](https://github.com/nickderobertis/printobserver/pull/42))
- *(crates)* [**breaking**] split the crate graph by domain and open the event log ([#29](https://github.com/nickderobertis/printobserver/pull/29))
