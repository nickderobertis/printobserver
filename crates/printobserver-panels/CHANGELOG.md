# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.17.0](https://github.com/nickderobertis/printobserver/compare/v0.15.1...v0.17.0) - 2026-11-28

### Added

- *(supervisor)* recover camera alignment and drift from a shifted view, and say why when the plain frame is shown ([#397](https://github.com/nickderobertis/printobserver/pull/397))
## [0.15.1](https://github.com/nickderobertis/printobserver/compare/v0.14.0...v0.15.1) - 2026-11-24

### Fixed

- *(supervisor)* pause a print whose parts stopped growing every time ([#368](https://github.com/nickderobertis/printobserver/pull/368))
## [0.14.0](https://github.com/nickderobertis/printobserver/compare/v0.13.1...v0.14.0) - 2026-11-21

### Added

- *(supervisor)* give each print check the three-panel image once its alignment passes ([#351](https://github.com/nickderobertis/printobserver/pull/351))
- *(supervisor)* [**breaking**] put each check's frame and facts in its prompt, on a lean harness ([#349](https://github.com/nickderobertis/printobserver/pull/349))
- *(panels)* draw the planned toolpath on the camera frame and compose the three-panel check image ([#346](https://github.com/nickderobertis/printobserver/pull/346))
