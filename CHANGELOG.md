# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

- Add release checklists and release package workflow documentation.
- Add local release helper script under `scripts/release.ps1`.
- Automate GitHub Release artifact packaging with per-target checksums.
- Add npm package entrypoint scaffold (`package.json`, `bin/octo.js`) for publishing `octo` CLI.

## [0.1.0] - 2026-05-18

### Added

- Introduced the `octocode` and `octo` Rust CLI binaries with multi-agent and local policy workflow.
- Added preview and parity checks in Windows CI.
- Added initial release packaging for GitHub Releases.
- Added npm distribution bootstrap script to download matching GitHub release binary on first run.

### Changed

- Improved TUI command tool rendering for `run_command` outputs and progress lines.
- Added npm package metadata scaffold for `octo`.

### Fixed

- Stabilized release pipeline artifacts naming and checksum generation.
