# How to manage multiple windows and root lifecycles

Attach one retained root to each Bevy-owned window and tear roots down without
leaving cameras pointed at removed render targets.

## Prerequisites

- `DefaultPlugins` followed by `GpuiPlugin`.
- One Bevy camera entity for each retained root.
- Explicit window targets for cameras outside the primary window.

## Create a second Bevy window

```rust
use bevy::{camera::RenderTarget, prelude::*, window::WindowRef};

fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let primary_camera = commands.spawn(Camera2d).id();

    let second_window = commands
        .spawn(Window {
            title: "Second window".into(),
            ..default()
        })
        .id();
    let second_camera = commands
        .spawn((
            Camera2d,
            RenderTarget::Window(WindowRef::Entity(second_window)),
        ))
        .id();

    gpui.set_root(primary_camera, |_, cx| cx.new(|_| PrimaryPanel))
        .expect("primary root should be queued");
    gpui.set_root(second_camera, |_, cx| cx.new(|_| SecondaryPanel))
        .expect("secondary root should be queued");
}
```

GPUI does not create either native window. The second `Window` entity enters
Bevy's ordinary Winit lifecycle. Its camera determines which window receives
the retained root's output and input.

## Route overlapping camera contexts

When several GPUI cameras target one window, the plugin sorts their contexts by
Bevy camera order. Pointer events route through contexts whose logical viewport
contains the cursor. Keyboard input goes to the context focused by the last
pointer press, or to the highest-order context when none is focused.

Attach each overlay to exactly one camera. Do not clone the same `GpuiContext`
across multiple camera entities.

## Use explicit roots in complex camera stacks

Disable automatic attachment when the application owns every overlay:

```rust
GpuiPlugin {
    auto_attach_primary: false,
    ..default()
}
```

Automatic attachment selects the highest-order active camera targeting the
primary window only when no explicit `GpuiContext` is present there.

## Remove one retained root

```rust
commands.entity(camera).remove::<GpuiContext>();
```

During the following synchronization pass, the plugin closes the virtual GPUI
window, drops the root factory, removes the extracted scene, and makes its typed
handle stale.

## Close a native Bevy window

Use Bevy's normal close message:

```rust
close_requests.write(WindowCloseRequested { window });
```

The plugin sees the request in `PreUpdate` and deactivates every GPUI camera
targeting that window. This protects Bevy's pipelined renderer while
`WindowPlugin` moves the target through its close state. Orphan cleanup then
closes the retained roots and removes their context components.

GPUI close-veto callbacks cannot override Bevy's default window close policy.
The integration logs a warning if a retained root requests a veto.

## Manually despawn a window safely

If application code bypasses `WindowCloseRequested`, first deactivate or
retarget every camera targeting that window. Despawning a live target and its
camera relationship in one step can leave Bevy's preceding extracted frame
referencing a target that no longer exists.

## Recover after render-device replacement

The plugin rebuilds all live retained roots when Bevy publishes a new render
device. Root builders passed to `set_root` must be reusable. Store durable
application state in Bevy resources or cloneable shared state rather than
consuming it permanently inside the builder.

Use `GpuiRuntimeStatus::gpu_generation` and `recoveries` for diagnostics.

## Verification

```bash
cargo run --example multi_window
cargo run --example lifecycle
```

The first command opens two Bevy windows with independent clickable roots. The
second removes one context, closes the other window, verifies both roots are
gone, and exits successfully.

## Troubleshooting

### Input appears in the wrong window

Confirm each camera uses `RenderTarget::Window(WindowRef::Entity(...))` for the
intended `Window` entity. Input routing follows the normalized camera target.

### `GpuiContexts::primary` returns an error

Exactly one camera must carry `PrimaryGpuiContext`. Automatic attachment adds
the marker to its chosen camera; explicit configurations must add it themselves
if they use the helper.

### A camera reports a missing window target

Deactivate or retarget the camera before manually despawning its window. Prefer
the normal `WindowCloseRequested` path.

## Related

- [Public API reference](reference.md#context-and-handle-types)
- [Architecture](architecture.md#lifecycle-and-recovery)
- [`multi_window` source](../examples/multi_window.rs)
- [`lifecycle` source](../examples/lifecycle.rs)
