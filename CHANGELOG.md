# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- An ordinary `GpuiPlugin` that preserves Bevy's runner, windows, renderer,
  schedules, presentation, and WGPU device/queue ownership.
- Embedded GPUI lifecycle and external-target WGPU renderer seams in the pinned
  `gpui-ce` fork, including sendable retained-scene snapshots.
- Camera-bound typed GPUI roots, primary-context helpers, multi-window and
  viewport routing, and Bevy `Image` render-target support.
- Mouse, wheel, keyboard, physical-key, modifier, text, IME, focus, pinch, and
  file-drop translation with aggregate input claims and gameplay run conditions.
- A deferred GPUI-to-Bevy command/message bridge with no retained `World`
  references.
- Main/background task dispatch, delayed timers, and Bevy event-loop wakeups.
- Zero-copy sampling of prepared Bevy `Image` assets inside GPUI through
  `GpuiBevyImage` and `bevy_image`.
- Backdrop/content filters over live Bevy color through `ViewTarget`
  post-processing, with explicit rejection of filtered cropped viewports.
- GPU-device generation tracking that rebuilds GPUI atlas, renderers, embedded
  application, and retained roots after Bevy renderer recovery.
- Executable getting-started, 3D overlay, text/IME, multi-window,
  render-to-texture, lifecycle, and HDR native smoke examples with automatic
  success instrumentation where practical.
- Diataxis documentation covering first use, task guides, public API,
  architecture, compatibility, troubleshooting, contribution, and maintenance.
- A prioritized future-work backlog derived from adversarial API, adopter, and
  maintainer reviews, with measurable completion gates for each open gap.

### Changed

- Replaced the inherited GPUI-owned runner architecture with the Bevy-owned
  integration specified in `docs/integration-spec.md`.
- Unified GPUI and Bevy on one crates.io `wgpu` package identity.

### Removed

- The custom Bevy runner, GPUI native-window ownership, and scoped direct-world
  access APIs from the rejected prototype.
