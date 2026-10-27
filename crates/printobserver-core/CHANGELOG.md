# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

## [0.4.1](https://github.com/nickderobertis/printobserver/compare/v0.4.0...v0.4.1) - 2026-10-15

### Fixed

- time the detector-pause resume grace from the second adjustment ([#225](https://github.com/nickderobertis/printobserver/pull/225))

## [0.4.0](https://github.com/nickderobertis/printobserver/compare/v0.3.0...v0.4.0) - 2026-10-12

### Added

- *(supervision)* charge spools for every print and check them at start ([#182](https://github.com/nickderobertis/printobserver/pull/182))
- *(store)* [**breaking**] persist spools, charges and the printer timeline ([#180](https://github.com/nickderobertis/printobserver/pull/180))
- *(spools)* [**breaking**] declare spool records, events and printer-port reads ([#178](https://github.com/nickderobertis/printobserver/pull/178))
- *(supervision)* observe an active print every two minutes without waiting for a detector alert ([#171](https://github.com/nickderobertis/printobserver/pull/171))
- *(supervisor)* camera looks, one agent per print, and a resume after an adjustment under the detector's pause ([#147](https://github.com/nickderobertis/printobserver/pull/147))

### Fixed

- *(release)* release every crate together in one version group ([#189](https://github.com/nickderobertis/printobserver/pull/189))
- *(api)* [**breaking**] bind each request to its credential and keep only the operator credential's verifier ([#165](https://github.com/nickderobertis/printobserver/pull/165))
- *(prints)* close ended jobs outside turns and open a new record for a later job ([#163](https://github.com/nickderobertis/printobserver/pull/163))
- *(supervisor)* let supervision turns act on Windows through printobserver's own commands ([#146](https://github.com/nickderobertis/printobserver/pull/146))

## [0.3.0](https://github.com/nickderobertis/printobserver/compare/v0.2.0...v0.3.0) - 2026-09-27

### Added

- *(windows)* bring the gate and the printer tier up on Windows ([#57](https://github.com/nickderobertis/printobserver/pull/57))
- *(api)* list prints and adopt the job OctoPrint is running ([#42](https://github.com/nickderobertis/printobserver/pull/42))
- *(crates)* [**breaking**] split the crate graph by domain and open the event log ([#29](https://github.com/nickderobertis/printobserver/pull/29))
