# `bevy_gpui` documentation

`bevy_gpui` embeds retained GPUI views in ordinary Bevy applications. Bevy
continues to own the event loop, native windows, renderer, GPU, schedules, and
presentation.

## Start here

- [Getting started](getting-started.md): build and run your first camera overlay.
- [Public API reference](reference.md): plugin settings, context handles,
  schedules, input state, images, and the ECS bridge.
- [Compatibility and limitations](compatibility.md): supported versions,
  features, platforms, targets, input, and host services.
- [Future work](future-work.md): prioritized engineering backlog derived from
  adversarial API, newcomer, and maintainer review findings.
- [Troubleshooting](troubleshooting.md): diagnose startup, rendering, input,
  lifecycle, and platform failures.

## How-to guides

- [How to route input and Bevy picking](how-to-input-and-picking.md)
- [How to synchronize GPUI and Bevy ECS](how-to-ecs-bridge.md)
- [How to use render targets and Bevy images](how-to-render-targets.md)
- [How to manage multiple windows and root lifecycles](how-to-multi-window.md)

## Understand the design

- [Architecture](architecture.md): ownership, schedules, rendering, input, and
  recovery.
- [Integration specification](integration-spec.md): the original implementation
  plan, acceptance matrix, and design evidence. The current API reference and
  compatibility page are authoritative for shipped behavior.
- [Vendored GPUI patch](../vendor/gpui-ce/BEVY_GPUI_PATCH.md): exact upstream
  revision and host-neutral embedding changes.
- [Learn GPUI](../README.md#learn-gpui): pinned retained-view learning examples
  and the required `bevy_gpui::gpui` import boundary.

## Maintain the project

- [Contributing](../CONTRIBUTING.md): local setup, validation, and pull-request
  expectations.
- [Maintainer guide](maintainers.md): updating the fork, validating platforms,
  capturing examples, and preparing releases.
- [Changelog](../CHANGELOG.md)

## Runnable examples

| Example | Demonstrates |
|---|---|
| `getting_started` | Minimal camera-bound retained overlay |
| `overlay_3d` | Live 3D overlay, deferred ECS writes, raw input filtering, and Bevy picking |
| `multi_window` | One retained root per Bevy window and camera |
| `render_to_texture` | GPUI rendered to a Bevy `Image` and Bevy images painted inside GPUI |
| `text_input` | Keyboard focus, key bindings, text entry, and IME composition |
| `lifecycle` | Context removal and secondary-window teardown |
| `hdr_overlay` | GPUI composited into an `Rgba16Float` HDR camera target |

Run an example with:

```bash
cargo run --example getting_started
```
