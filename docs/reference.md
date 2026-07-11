# Public API reference

This page describes the public API exported by `bevy_gpui` 0.1.0. The crate
targets Bevy 0.19 and re-exports its pinned GPUI revision as `bevy_gpui::gpui`.

## Installation

The crate is not published to crates.io yet (`publish = false`). Depend on the
repository while evaluating it, then pin a tested commit for reproducible
application builds:

```toml
[dependencies]
bevy = "0.19"
bevy_gpui = { path = "../bevy_gpui" }
```

No committed remote revision contains the current in-progress integration yet.
Use this checkout by path, then switch to an exact Git `rev` after the
integration is committed and pushed.

Add the plugin after Bevy's rendering, window, and Winit plugins. Normal
applications should add it after `DefaultPlugins`:

```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(GpuiPlugin::default())
    .run();
```

With the `render` feature enabled, adding `GpuiPlugin` before the corresponding
Bevy plugins panics during plugin construction with an ordering error.

## Cargo features

| Feature | Default | Effect |
|---|---:|---|
| `render` | Yes | Enables camera overlays, Bevy render-world extraction, `GpuiBevyImage`, and `bevy_image` |
| `picking` | Yes | Adds a blocker at `f32::MAX` for every picking pointer in a GPUI-claimed Bevy window |
| `font-kit` | Yes | Enables GPUI's platform font-kit integration |
| `wayland` | Yes | Enables GPUI Wayland support |
| `x11` | Yes | Enables GPUI X11 support |
| `windows-manifest` | Yes | Enables the GPUI Windows manifest support needed by its platform crate |

`cargo check --lib --no-default-features` supports compile-time coverage of the
plugin's state resources and run conditions. It does not initialize an embedded
runtime or retained-root lifecycle. Creating a retained root without `render`
returns an error.

## `GpuiPlugin`

```rust
pub struct GpuiPlugin {
    pub render_order: GpuiRenderOrder,
    pub auto_attach_primary: bool,
    pub accessibility: GpuiAccessibility,
}
```

Defaults:

```rust
GpuiPlugin {
    render_order: GpuiRenderOrder::AboveBevyUi,
    auto_attach_primary: true,
    accessibility: GpuiAccessibility::Disabled,
}
```

### `render_order`

- `GpuiRenderOrder::BelowBevyUi` records GPUI after the main 2D/3D camera pass
  and before Bevy's UI pass.
- `GpuiRenderOrder::AboveBevyUi` records GPUI after Bevy's UI pass. This is the
  default.

Both variants run before Bevy's upscaling/presentation stage.

### `auto_attach_primary`

When `true`, the plugin creates an empty retained root on the highest-order
active camera targeting the primary window if no such camera already has a
`GpuiContext`. It marks that camera with `PrimaryGpuiContext`.

Set this to `false` when every root is created explicitly, or when the app does
not want an empty root on its primary camera.

### `accessibility`

`GpuiAccessibility` currently has one variant: `Disabled`. Bevy already owns
the native AccessKit adapter, and neither framework exposes a supported subtree
merge API. The plugin does not install a conflicting second adapter.

## `GpuiContexts`

`GpuiContexts<'w, 's>` is a main-thread Bevy `SystemParam`. It owns access to
the embedded GPUI application while a Bevy system runs.

### `set_root`

```rust
pub fn set_root<V, F>(
    &mut self,
    camera: Entity,
    build: F,
) -> anyhow::Result<GpuiViewHandle<V>>
where
    V: gpui::Render + 'static,
    F: Fn(&mut gpui::Window, &mut gpui::App) -> gpui::Entity<V> + 'static;
```

Creates a retained GPUI root and associates it with a Bevy camera. Calls made
before the render device is ready are queued. The returned typed handle keeps
the same internal ID if Bevy replaces its render device.

The builder can run more than once during render-device recovery. It must be
able to reconstruct the view from durable Bevy or shared application state.
The supplied entity must remain live and carry the Bevy camera/render-target
components used by camera queries. `set_root` does not validate that precondition
synchronously; an invalid entity can produce a root that is later orphaned and
a stale handle.

When the runtime is already ready, the method can return a virtual-window/open
error synchronously. Before runtime readiness, it stores the reusable factory
and returns a handle; a later materialization failure is logged and leaves the
handle pending.

Errors returned directly:

- the `render` feature is disabled;
- GPUI cannot open the virtual adapter window while materializing synchronously;
- another virtual GPUI root is already in the middle of opening synchronously.

### `set_root_with_options`

```rust
pub fn set_root_with_options<V, F>(
    &mut self,
    camera: Entity,
    options: gpui::WindowOptions,
    build: F,
) -> anyhow::Result<GpuiViewHandle<V>>;
```

Behaves like `set_root`, but accepts explicit GPUI virtual-window options. The
options configure the retained GPUI window abstraction; they do not create a
second native window.

### `update`

```rust
pub fn update<V, R>(
    &mut self,
    handle: &GpuiViewHandle<V>,
    update: impl FnOnce(
        &mut V,
        &mut gpui::Window,
        &mut gpui::Context<V>,
    ) -> R,
) -> anyhow::Result<R>
where
    V: gpui::Render + 'static;
```

Updates the root referenced by a typed handle. Call `cx.notify()` inside the
closure when a changed view must be repainted.

Errors:

- the embedded runtime is not ready;
- the handle is pending or stale;
- the handle's view type does not match the stored GPUI entity;
- GPUI rejects a reentrant or otherwise invalid application update.

### `primary`

```rust
pub fn primary<V>(&self) -> anyhow::Result<GpuiViewHandle<V>>
where
    V: gpui::Render + 'static;
```

Returns a typed handle for the single camera marked `PrimaryGpuiContext`.
It errors if zero or multiple matching cameras exist. The requested type is
validated later when the handle is passed to `update`.

## Context and handle types

### `GpuiViewHandle<V>`

A copyable typed token for one retained root. It does not expose or retain a
direct GPUI entity reference outside the embedded main-thread runtime.

### `GpuiContext`

A camera component inserted by `set_root` and `set_root_with_options`. Removing
it tears down the retained root and removes its `GpuiScene` after synchronization.
Its fields are private; applications should treat it as an ownership marker.

### `PrimaryGpuiContext`

A marker component used by `GpuiContexts::primary`. Automatic attachment adds
it to the selected primary-window camera. Applications using explicit roots may
add it to exactly one camera when they want the primary helper.

### `GpuiScene`

```rust
pub struct GpuiScene {
    pub snapshot: Arc<gpui::SceneSnapshot>,
    pub generation: u64,
}
```

An immutable renderer-ready retained scene attached to the same camera as its
`GpuiContext`. With `render`, Bevy extracts a cheap clone into the pipelined
render world. Applications normally inspect this component only for diagnostics.

## Runtime status

`GpuiRuntimeStatus` is a Bevy resource with these public fields:

| Field | Type | Meaning |
|---|---|---|
| `ready` | `bool` | The embedded application and shared GPU atlas are initialized |
| `roots` | `usize` | Number of materialized retained roots after the latest window synchronization |
| `scenes_built` | `u64` | Number of scene snapshots delivered to cameras |
| `gpu_generation` | `u64` | Current Bevy render-device generation used by GPUI |
| `recoveries` | `u64` | Number of embedded-runtime rebuilds after render-device replacement |

The counters are diagnostic state, not frame synchronization primitives.
`GpuiRuntimeStatus::default()` sets `ready` to `false` and every counter to zero.

## Deferred writes into Bevy

`BevyAppContextExt` extends `gpui::App` with:

```rust
pub trait BevyAppContextExt {
    fn send_bevy_message<M: bevy_ecs::message::Message>(&mut self, message: M);
    fn queue_bevy_command<C: bevy_ecs::system::Command<Out = ()>>(
        &mut self,
        command: C,
    );
}
```

Both methods enqueue work. `GpuiSystems::ApplyDeferredBridge` applies it at the
next safe main-world boundary after input dispatch. A retained callback never
receives a live Bevy `World` reference.

## Input state and run conditions

`GpuiInputState` is a Bevy resource:

| Field | Type | Meaning |
|---|---|---|
| `wants_pointer_input` | `bool` | The latest routed pointer state contains a GPUI hit or capture claim in at least one window; it may persist until another event clears it |
| `wants_keyboard_input` | `bool` | Focused GPUI text input or a keyboard dispatch in the latest pass claimed input |
| `default_prevented` | `bool` | At least one GPUI dispatch called `prevent_default` in the latest input pass |

Two public run conditions expose the claims:

```rust
pub fn gpui_wants_pointer_input(state: Option<Res<GpuiInputState>>) -> bool;
pub fn gpui_wants_keyboard_input(state: Option<Res<GpuiInputState>>) -> bool;
```

They return `false` if the resource does not exist. Use them for polling systems
that do not own message cursors. A system with `MessageReader` must always run
and drain its messages, then inspect `GpuiInputState` before acting; otherwise a
skipped reader can observe an old UI event after the claim clears. See [How to
route input and Bevy picking](how-to-input-and-picking.md) for the complete
pattern.

The booleans are aggregate pass-level state, not per-message receipts. Pointer
claims are private and window keyed, while the public resource does not expose
which window claimed input.
`GpuiInputState::default()` sets all three public booleans to `false` and starts
with no pointer claims.

## Bevy image interop

These APIs require the `render` feature.

### `GpuiBevyImage`

`GpuiBevyImage::new(handle: Handle<Image>)` creates a cloneable, stable GPUI
reference to a Bevy image asset. `handle()` returns the supplied Bevy handle.
Pass a strong asset handle when this wrapper must retain the image lifetime;
the constructor also accepts other `Handle<Image>` variants and does not
upgrade them to strong handles.

The registration does not keep the image alive after the final
`GpuiBevyImage` clone is dropped. A not-yet-prepared image uses Bevy's fallback
texture and requests another retained frame.

### `bevy_image`

```rust
pub fn bevy_image(image: GpuiBevyImage) -> gpui::Canvas<()>;
```

Returns a styleable GPUI canvas that paints the Bevy-owned GPU image directly.
The steady-state path performs no CPU framebuffer readback and does not copy
the image into GPUI's sprite atlas.

## Schedule sets

`GpuiSystems` exposes stable ordering points:

| Set | Schedule | Work |
|---|---|---|
| `DriveExecutor` | `PreUpdate` | Poll GPUI foreground tasks and expired timers |
| `Input` | `PreUpdate` | Translate Bevy messages and dispatch GPUI input |
| `ApplyDeferredBridge` | `PreUpdate` | Apply queued GPUI-to-Bevy commands and messages |
| `WindowSync` | `PostUpdate` | Synchronize cameras, windows, monitors, cursor, theme, and IME state |
| `BuildScene` | `PostUpdate` | Layout and paint dirty GPUI roots into scene snapshots |

The three `PreUpdate` sets are chained in the order shown. `WindowSync` runs
after Bevy camera updates and before `BuildScene`.

## GPUI re-export

`bevy_gpui::gpui` re-exports the exact GPUI API compiled by the integration.
Import GPUI traits and elements from this re-export to avoid accidentally
depending on a second, incompatible GPUI revision.

## Related

- [Getting started](getting-started.md)
- [Architecture](architecture.md)
- [Compatibility and limitations](compatibility.md)
- [How to synchronize GPUI and Bevy ECS](how-to-ecs-bridge.md)
