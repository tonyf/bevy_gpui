# Troubleshooting

Start with the exact symptom below. Commands assume the repository checkout;
application users can substitute their own package and example names.

## The plugin panics during startup

### `GpuiPlugin must be added after Bevy's RenderPlugin`

Add `GpuiPlugin` after `DefaultPlugins` or after the custom plugin group that
installs Bevy rendering:

```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(GpuiPlugin::default());
```

The render build also requires Bevy's `WindowPlugin` and `WinitPlugin`.

## The window opens but no GPUI is visible

Check, in order:

1. The root is attached to the intended active camera.
2. The camera has a non-zero physical viewport.
3. The camera still has `GpuiContext` and later receives `GpuiScene`.
4. `GpuiRuntimeStatus::ready` is `true`, `roots` is non-zero, and
   `scenes_built` advances after the view becomes dirty.
5. Another camera or Bevy UI pass is not drawing over GPUI. Try
   `GpuiRenderOrder::AboveBevyUi`.
6. The retained mutation calls `cx.notify()`.

Run the minimal known-good example:

```bash
cargo run --example getting_started
```

## A handle update fails

### `the embedded GPUI runtime is not ready yet`

The render device and atlas have not been published. Retry in a later system and
inspect `GpuiRuntimeStatus::ready`.

### `GPUI view handle is stale or still pending`

The root is waiting for GPU initialization, deferred virtual-window
materialization failed and logged an error, or its `GpuiContext` was removed.
Retry genuinely pending roots, inspect materialization errors, and stop using
handles after teardown.

### `GPUI view handle has the wrong view type`

The `GpuiViewHandle<V>` type does not match the entity created by that root.
Store the handle in a typed resource and do not reconstruct it through an
unrelated `primary::<V>()` call.

## UI clicks affect the scene

Raw Bevy message systems must run after `GpuiSystems::Input`, always drain their
readers, and skip behavior while the matching `GpuiInputState` claim is true.
Polling systems without message cursors may use the negated pointer or keyboard
run condition. Bevy messages are broadcast and cannot be deleted by the
integration.

For Bevy picking, enable the default `picking` feature and install
`PickingPlugin` before `GpuiPlugin`. See [How to route input and Bevy
picking](how-to-input-and-picking.md).

## The whole scene stops receiving pointer input

Inspect full-window retained elements for pointer handlers or modal hit regions.
Attach handlers to the visible panel or control rather than the root when the
rest of the camera should remain interactive.

## Text input does not work

GPUI needs a focused entity that installs an `ElementInputHandler`. Confirm:

- the element implements `Focusable` and is focused through the GPUI window;
- the element calls `window.handle_input` while painting;
- key bindings use names produced by GPUI, such as `backspace`, `left`, and
  `right`;
- the Bevy window's `ime_enabled` becomes `true` while the handler accepts text.

Run:

```bash
cargo run --example text_input
```

The example injects keyboard text, IME preedit, and IME commit and logs success.

## A Bevy image inside GPUI never appears

- Keep a `GpuiBevyImage` clone alive in retained state.
- Confirm the underlying `Handle<Image>` points to a loaded asset.
- Allow a later frame after Bevy prepares the GPU image.
- Check render logs for a missing asset or external-surface renderer error.

Until preparation completes, the plugin intentionally uses Bevy's fallback
texture and requests another frame.

## A render-to-texture result is upside down

Bevy images use a top-left UV origin. The consuming mesh may map V=0 to its
bottom edge. Flip the appropriate mesh UVs or adjust the sampling material. See
`display_mesh` in [`render_to_texture.rs`](../examples/render_to_texture.rs).

## Filters log an external-renderer error

Backdrop and content filters require a full-target, zero-origin camera viewport.
Remove the filter, use a full-target camera, or render the filtered UI into a
separate full-size target. Unfiltered cropped viewports are supported.

## A secondary window closes badly

Prefer `WindowCloseRequested`. If manually despawning a `Window`, deactivate or
retarget its cameras first. The renderer may still be consuming the preceding
extracted frame.

## Linux fails to build native dependencies

Install Bevy 0.19's native requirements plus the additional windowing and font
packages used by this repository's CI:

```bash
sudo apt-get update
sudo apt-get install --no-install-recommends \
  g++ pkg-config libx11-dev libasound2-dev libudev-dev \
  libxkbcommon-x11-0 libfontconfig-dev libwayland-dev \
  libx11-xcb-dev libxcb1-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libxkbcommon-x11-dev
```

See Bevy 0.19's pinned
[Linux dependency guide](https://github.com/bevyengine/bevy/blob/v0.19.0/docs/linux_dependencies.md)
for other distributions and Vulkan driver packages.

To select only X11 on the GPUI side of this path dependency:

```toml
bevy_gpui = { path = "../bevy_gpui", default-features = false, features = ["render", "picking", "font-kit", "x11"] }
```

To select only Wayland on the GPUI side:

```toml
bevy_gpui = { path = "../bevy_gpui", default-features = false, features = ["render", "picking", "font-kit", "wayland"] }
```

These snippets configure only `bevy_gpui`. Bevy's default `default_platform`
feature still enables both X11 and Wayland. A truly single-backend application
must also disable Bevy defaults and select its application-specific Bevy
features plus `x11` or `wayland`; use Bevy's
[0.19 feature list](https://docs.rs/crate/bevy/0.19.0/features) to construct that
larger application configuration.

## Clipboard, prompts, menus, or credentials do not work

Text clipboard and URL/path opening are implemented. Native file/save prompts,
message prompts, application menus, credential storage, URL-scheme callbacks,
and restart/hide operations are unsupported. See the [compatibility
page](compatibility.md#window-and-host-services) for exact behavior.

## Device recovery resets retained local state

Root builders run again when Bevy replaces its render device. Reconstruct views
from durable Bevy or shared state and synchronize that state after recovery.
Watch `GpuiRuntimeStatus::recoveries` to confirm this path occurred.

## Collect diagnostics

Run the validation gates:

```bash
cargo check --all-targets
cargo test --lib
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps
```

When reporting a problem, include:

- operating system and window backend;
- GPU and renderer backend;
- Rust, Bevy, and `bevy_gpui` revisions;
- enabled Cargo features;
- target type, camera viewport, and render order;
- relevant logs and the smallest example that reproduces the issue.
