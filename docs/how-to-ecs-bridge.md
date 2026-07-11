# How to synchronize GPUI and Bevy ECS

Move state in both directions without retaining a Bevy `World` reference inside
a GPUI callback.

## Prerequisites

- A retained root created with `GpuiContexts::set_root`.
- A Bevy resource, component, command, or message that should interact with UI.

## Send a GPUI callback into Bevy

Import `BevyAppContextExt` and enqueue a Bevy command from the retained callback:

```rust
use bevy::prelude::*;
use bevy_gpui::BevyAppContextExt;

#[derive(Resource, Default)]
struct ClickCount(u32);

// Register the resource while building the app:
app.init_resource::<ClickCount>();

// Inside Render::render:
div()
    .id("increment")
    .child("Increment")
    .on_click(cx.listener(|view, _, _, cx| {
        view.local_clicks += 1;
        cx.queue_bevy_command(|world: &mut World| {
            world.resource_mut::<ClickCount>().0 += 1;
        });
        cx.notify();
    }))
```

The closure passed to `queue_bevy_command` must implement Bevy's `Command` and
return `()`. It is stored until the bridge reaches a safe main-world boundary.

To publish a typed Bevy message instead:

```rust
#[derive(Message)]
struct OpenInventory;

// Register once while building the Bevy app:
app.add_message::<OpenInventory>();

// Inside a GPUI callback:
cx.send_bevy_message(OpenInventory);
```

Input callbacks run during `GpuiSystems::Input`, and their queued writes are
applied in the following `GpuiSystems::ApplyDeferredBridge` set in the same
`PreUpdate`. Writes queued while a retained scene is built apply during the
next frame's bridge pass.

## Synchronize Bevy state into GPUI

Store the typed root handle in a Bevy resource:

```rust
#[derive(Resource)]
struct HudRoot(GpuiViewHandle<HudView>);

fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let camera = commands.spawn(Camera2d).id();
    let root = gpui
        .set_root(camera, |_, cx| cx.new(|_| HudView { score: 0 }))
        .expect("HUD root should be queued");
    commands.insert_resource(HudRoot(root));
}
```

Update it from an ordinary system:

```rust
fn sync_score(
    score: Res<Score>,
    root: Res<HudRoot>,
    mut gpui: GpuiContexts,
) {
    if let Err(error) = gpui.update(&root.0, |view, _, cx| {
        if view.score != score.0 {
            view.score = score.0;
            cx.notify();
        }
    }) {
        warn!(?error, "HUD root is not ready");
    }
}
```

This system retries until a pending or reconstructed root is ready, but only
notifies GPUI when the rendered value changes. A `Res::is_changed()` early
return alone is unsafe here: the Bevy resource may stop reporting a change
before initial root materialization or render-device recovery completes.

`cx.notify()` is necessary when the mutation changes rendered output. Without
it, the retained view may remain clean and no new scene is built.

## Choose durable root state

The builder passed to `set_root` can run again after Bevy replaces its render
device. Do not make the builder consume one-shot state that cannot be recreated.
Prefer one of these patterns:

- Rebuild the view from Bevy resources in subsequent synchronization systems.
- Capture cloneable shared state.
- Construct a default view and immediately synchronize durable state into it.

The `GpuiViewHandle` keeps its ID across recovery, but the underlying GPUI
entity is reconstructed.

## Remove a root

Remove `GpuiContext` from the camera:

```rust
commands.entity(camera).remove::<GpuiContext>();
```

The next synchronization pass closes the GPUI adapter window, drops the root
factory, removes any `GpuiScene`, and decrements `GpuiRuntimeStatus::roots`.
Handles for that root become stale and `GpuiContexts::update` returns an error.

## Verification

Run:

```bash
cargo run --example overlay_3d
```

Clicking the GPUI button must increase both the local GPUI count and the Bevy
resource count. The example continuously synchronizes the Bevy count back into
the retained view.

For teardown behavior:

```bash
cargo run --example lifecycle
```

The example removes one context, closes another window, verifies the root count
reaches zero, and exits.

## Troubleshooting

### `the embedded GPUI runtime is not ready yet`

Root creation and GPU initialization can be asynchronous relative to early
startup systems. Treat this error as a pending state and retry from a later
system. `GpuiRuntimeStatus::ready` and `roots` provide diagnostics.

### `GPUI view handle is stale or still pending`

The root is not materialized yet or its `GpuiContext` was removed. Retry pending
roots; stop updating removed roots.

### `GPUI view handle has the wrong view type`

The typed handle was reconstructed or requested with the wrong `V`. Keep the
handle in a resource whose field type names the actual retained root.

## Related

- [Public API reference](reference.md#deferred-writes-into-bevy)
- [Architecture](architecture.md#safe-ecs-interoperability)
- [`overlay_3d` source](../examples/overlay_3d.rs)
