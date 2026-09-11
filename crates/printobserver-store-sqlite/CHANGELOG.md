# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/nickderobertis/printobserver/releases/tag/v0.2.0) - 2026-09-11

### Added

- *(server)* serve the normalized API, ingest Obico alerts, and run as a service ([#16](https://github.com/nickderobertis/printobserver/pull/16))
- *(obico)* normalize and persist Obico failure alerts and their images ([#12](https://github.com/nickderobertis/printobserver/pull/12))
- *(store)* persist events, images, actions, interventions and manifests in SQLite ([#8](https://github.com/nickderobertis/printobserver/pull/8))
- *(repo)* stand up the repository on the create-repo baseline ([#1](https://github.com/nickderobertis/printobserver/pull/1))

### Fixed

- *(release)* release the workspace coherently, and publish only what was cut ([#24](https://github.com/nickderobertis/printobserver/pull/24))
