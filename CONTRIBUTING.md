# Contributing to `bevy_gpui`

`bevy_gpui` joins two renderer and application frameworks. Changes must preserve
Bevy's ownership of the event loop, native windows, schedules, GPU resources,
command submission, and presentation.

## Development setup

Requirements:

- Rust 1.95.0 or newer with `rustfmt` and Clippy.
- A desktop environment supported by Bevy 0.19.
- On Linux, the X11, Wayland, XKB, XCB, and Fontconfig development packages
  installed by [CI](.github/workflows/check.yml).

Clone the repository with its vendored GPUI source and build all targets:

```bash
git clone https://github.com/tonyf/bevy_gpui.git
cd bevy_gpui
cargo check --all-targets
```

## Ownership invariants

Every change must keep these rules true:

1. Bevy's existing runner and Winit event loop remain installed.
2. GPUI does not create or present a native WGPU surface for a Bevy window.
3. GPUI records into Bevy's command encoder and never submits it.
4. Retained GPUI callbacks do not hold a live Bevy `World` reference.
5. Scene data crossing into `RenderApp` remains `Send + Sync` and free of
   main-thread platform objects.
6. Unsupported platform services are listed in the compatibility contract.
   Adapter-specific overrides should return errors or warnings; inherited GPUI
   default no-ops must remain documented explicitly.

Read [Architecture](docs/architecture.md) before changing lifecycle, input, or
render ownership.

## Make a focused change

- Preserve unrelated changes in a dirty worktree.
- Add or update a runnable example when user-visible behavior changes.
- Add a regression test for input routing, lifecycle, target formats, or the
  vendored embedding seam affected by the change.
- Update the [API reference](docs/reference.md), [compatibility contract](docs/compatibility.md),
  and [changelog](CHANGELOG.md) when behavior or support changes.
- Keep all GPUI imports routed through the vendored workspace or
  `bevy_gpui::gpui`; do not introduce a second GPUI package identity.

## Validation

Run the root crate gates:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --lib --no-default-features
cargo test --lib
cargo test --examples
cargo test --doc
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps
ruby scripts/check-docs.rb
ruby scripts/check-wgpu-identity.rb
```

Run the vendored GPUI regression suites when its code or a renderer boundary
changes:

```bash
cargo test --manifest-path vendor/gpui-ce/Cargo.toml -p gpui --lib
cargo test --manifest-path vendor/gpui-ce/Cargo.toml -p gpui_wgpu --lib
```

GPU-backed tests require access to a native adapter. A sandbox or headless host
may report that no Metal, Vulkan, or DX12 adapter is available even when the
code is correct. Rerun on a host with GPU access before treating those tests as
passed.

## Native smoke tests

At minimum, launch:

```bash
cargo run --example getting_started
cargo run --example overlay_3d
cargo run --example text_input
cargo run --example multi_window
cargo run --example render_to_texture
cargo run --example lifecycle
cargo run --example hdr_overlay
```

For input changes, verify both sides of the boundary:

- a GPUI control click changes only UI/bridged state;
- the same click does not reach raw scene input or Bevy picking;
- a click outside retained controls still reaches the scene;
- raw message readers keep running and draining while GPUI owns input, so old UI
  messages cannot surface after a run condition starts passing again.

For render changes, inspect camera content beneath GPUI, alpha edges, text,
paths, images, viewport offsets, and any filter output.

## Documentation style

- Keep tutorials focused on learning, how-to pages focused on tasks, reference
  pages factual, and architecture pages focused on design reasoning.
- Use complete commands and code that matches the current API.
- Link every new page from [the documentation index](docs/README.md).
- Distinguish runtime-tested, compile-tested, pending, and unsupported behavior.
- Do not describe planned behavior as shipped.

## Pull requests

A pull request should include:

- the user-visible outcome;
- which ownership or lifecycle boundary changed;
- tests and native examples run;
- platform and GPU backend used for runtime validation;
- screenshots when rendering changes;
- documentation and compatibility updates.

Do not commit generated `target/` directories from either the root crate or the
vendored GPUI workspace.

## Vendored GPUI changes

Keep host-neutral embedding modifications isolated and documented in
[`vendor/gpui-ce/BEVY_GPUI_PATCH.md`](vendor/gpui-ce/BEVY_GPUI_PATCH.md). Follow
the [maintainer guide](docs/maintainers.md#update-the-vendored-gpui-revision)
for revision updates.
