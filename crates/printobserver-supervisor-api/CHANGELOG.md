# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.14.0](https://github.com/nickderobertis/printobserver/compare/v0.12.0...v0.14.0) - 2026-11-21

### Added

- *(supervisor)* compact each print's conversation while idle, and at 200k as a backstop ([#352](https://github.com/nickderobertis/printobserver/pull/352))
- *(supervisor)* give each print check the three-panel image once its alignment passes ([#351](https://github.com/nickderobertis/printobserver/pull/351))
- *(supervisor)* [**breaking**] put each check's frame and facts in its prompt, on a lean harness ([#349](https://github.com/nickderobertis/printobserver/pull/349))

## [0.12.0](https://github.com/nickderobertis/printobserver/compare/v0.11.1...v0.12.0) - 2026-11-07

### Added

- *(skill)* prove supervision on real prints replayed as continued conversations ([#326](https://github.com/nickderobertis/printobserver/pull/326))

## [0.9.0](https://github.com/nickderobertis/printobserver/compare/v0.8.1...v0.9.0) - 2026-10-27

### Added

- *(supervision)* [**breaking**] record resets, disconnects, unattended heating, detector outages and blank frames ([#272](https://github.com/nickderobertis/printobserver/pull/272))

## [0.5.0](https://github.com/nickderobertis/printobserver/compare/v0.4.3...v0.5.0) - 2026-10-26

### Fixed

- *(printer)* [**breaking**] refuse malformed replies, keep unknown states and keep bounds across clock steps ([#262](https://github.com/nickderobertis/printobserver/pull/262))

## [0.4.0](https://github.com/nickderobertis/printobserver/compare/v0.3.0...v0.4.0) - 2026-10-12

### Added

- *(supervisor)* camera looks, one agent per print, and a resume after an adjustment under the detector's pause ([#147](https://github.com/nickderobertis/printobserver/pull/147))

### Fixed

- *(api)* [**breaking**] bind each request to its credential and keep only the operator credential's verifier ([#165](https://github.com/nickderobertis/printobserver/pull/165))

## [0.3.0](https://github.com/nickderobertis/printobserver/compare/v0.2.0...v0.3.0) - 2026-09-27

### Added

- *(windows)* bring the gate and the printer tier up on Windows ([#57](https://github.com/nickderobertis/printobserver/pull/57))
- *(crates)* [**breaking**] split the crate graph by domain and open the event log ([#29](https://github.com/nickderobertis/printobserver/pull/29))
