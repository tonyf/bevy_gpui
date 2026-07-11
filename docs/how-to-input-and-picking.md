# How to route input and Bevy picking

Use GPUI input claims to prevent a pointer or keyboard action intended for UI
from also driving gameplay.

## Prerequisites

- A `GpuiPlugin` installed after `DefaultPlugins`.
- At least one retained root attached to a camera.
- A gameplay system that reads Bevy input messages, resources, or picking
  events.

## Filter raw pointer messages

Bevy messages are broadcast. `bevy_gpui` can read a `MouseButtonInput`, but it
cannot remove that message before another reader sees it. Order your gameplay
reader after GPUI dispatch, always drain it, and ignore messages while GPUI owns
the pointer:

```rust
use bevy::{input::{ButtonState, mouse::MouseButtonInput}, prelude::*};
use bevy_gpui::{GpuiInputState, GpuiSystems};

app.add_systems(
    PreUpdate,
    fire_weapon.after(GpuiSystems::Input),
);

fn fire_weapon(
    input: Res<GpuiInputState>,
    mut buttons: MessageReader<MouseButtonInput>,
) {
    for event in buttons.read() {
        if !input.wants_pointer_input
            && event.button == MouseButton::Left
            && event.state == ButtonState::Pressed
        {
            // Fire only when GPUI did not claim this completed input pass.
        }
    }
}
```

All three pieces matter:

1. `.after(GpuiSystems::Input)` ensures the claim reflects the completed GPUI
   input pass.
2. The loop always calls `buttons.read()`, advancing the reader even while UI
   owns the input.
3. The condition suppresses gameplay behavior without leaving old UI messages
   queued for a later frame.

Apply the same pattern to mouse-look, drag selection, wheel zoom, and other raw
pointer systems.

Do not put a message-reading system behind a `run_if` input gate. When a run
condition skips the system, its `MessageReader` cursor does not advance. A UI
message can remain unread and appear later after the claim clears.

The public `gpui_wants_pointer_input` run condition remains useful for systems
that poll input resources and do not own message cursors.

GPUI click counts are synthesized per Bevy window and mouse button. Consecutive
presses count as one sequence when they occur within 500 milliseconds and four
logical pixels; a later or more distant press starts again at one.

## Filter raw keyboard messages

```rust
use bevy_gpui::{GpuiInputState, GpuiSystems};

app.add_systems(PreUpdate, keyboard_shortcuts.after(GpuiSystems::Input));
```

Always drain the keyboard reader and skip shortcut behavior when
`input.wants_keyboard_input` is true. The keyboard claim is set when GPUI
dispatch handles or prevents a key event, or while a focused GPUI input handler
accepts text. The `gpui_wants_keyboard_input` run condition is suitable for
polling systems without message cursors.

## Use Bevy picking

With the default `picking` feature, the plugin integrates automatically if
Bevy's `PickingPlugin` has already been installed when `GpuiPlugin` is built.
`DefaultPlugins` includes Bevy's picking infrastructure; add any backend your
scene needs, such as `MeshPickingPlugin`:

```rust
App::new().add_plugins((
    DefaultPlugins,
    MeshPickingPlugin,
    GpuiPlugin::default(),
));
```

For a claimed window, `bevy_gpui` emits a hit at `f32::MAX` backend order for
every Bevy pointer located in that window. The hit belongs to an internal
non-hoverable entity configured to block lower hits. A GPUI button therefore
does not also trigger `On<Pointer<Click>>` on the mesh behind it in the normal
Bevy picking pipeline.

Plugin order matters. If `GpuiPlugin` is added before `PickingPlugin`, the
automatic backend is not installed because the plugin is not present during
`GpuiPlugin::build`.

## Scope UI hit regions deliberately

GPUI claims a pointer when its hit test finds an interactive or otherwise
hittable retained element. Avoid attaching input handlers to a full-window root
unless the UI is intentionally modal. Put click, mouse-down, and scroll handlers
on the panel or control that should own those events.

Pointer capture remains claimed while events continue inside the mapped camera
viewport. Leaving that viewport, or sending `CursorLeft` for the native window,
clears the public claim because the adapter no longer dispatches the pointer
event to that context. Do not rely on cross-window or outside-viewport capture
for a drag.

## Handle multiple windows

Pointer claims are tracked internally per Bevy window. A retained hit in one
native window produces picking blockers only for pointers in that window. The
public `GpuiInputState` exposes only an aggregate boolean, so application code
cannot query which window claimed it. A global raw-input system therefore pauses
if any GPUI window owns the pointer. True per-window raw gameplay gating needs
an application-level active-window policy until the crate exposes
window-specific claims.

The claim is pass-level rather than a receipt for each Bevy message. Bevy stores
cursor, button, wheel, and gesture messages in separate streams, so the adapter
cannot reconstruct a total cross-type OS event order. The documented
drain-and-filter pattern prevents buffered UI messages from reappearing later,
but dense mixed pointer activity in one Bevy update is interpreted as one
aggregate input pass.

## Verification

Run the interaction example:

```bash
cargo run --example overlay_3d
```

Expected behavior:

- Clicking **Increment** increases the GPUI callback and Bevy resource counts.
- The same UI click does not increase the scene-input or picking counts.
- Clicking the 3D scene increases the scene-input and picking counts without
  increasing the GPUI counter.

## Troubleshooting

### UI clicks still reach a raw gameplay system

Confirm the gameplay system is in `PreUpdate`, ordered after
`GpuiSystems::Input`, drains every message, and ignores behavior while the
matching claim is true. Merely reading `GpuiInputState` in an unordered system
can observe the previous input pass.

### UI clicks still reach a picked mesh

Confirm the `picking` feature is enabled and `PickingPlugin` is installed before
`GpuiPlugin`. Check for a custom backend using a competing order such as
`f32::MAX`, positive infinity, or a path that bypasses Bevy's normal
hit-blocking pipeline.

### The whole scene is blocked

Look for event handlers or hit-test behavior on a full-window GPUI element.
Reduce the retained hit region to the visible controls that should own input.

## Related

- [Public API reference](reference.md#input-state-and-run-conditions)
- [Architecture](architecture.md#input-ownership)
- [`overlay_3d` source](../examples/overlay_3d.rs)
