# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/nickderobertis/printobserver/compare/v0.3.0...v0.4.0) - 2026-10-12

### Added

- *(supervisor)* camera looks, one agent per print, and a resume after an adjustment under the detector's pause ([#147](https://github.com/nickderobertis/printobserver/pull/147))

### Fixed

- *(api)* [**breaking**] bind each request to its credential and keep only the operator credential's verifier ([#165](https://github.com/nickderobertis/printobserver/pull/165))

## [0.3.0](https://github.com/nickderobertis/printobserver/compare/v0.2.0...v0.3.0) - 2026-09-27

### Added

- *(windows)* bring the gate and the printer tier up on Windows ([#57](https://github.com/nickderobertis/printobserver/pull/57))
- *(crates)* [**breaking**] split the crate graph by domain and open the event log ([#29](https://github.com/nickderobertis/printobserver/pull/29))
