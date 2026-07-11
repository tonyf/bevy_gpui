# Architecture

`bevy_gpui` lets an existing Bevy application render and interact with retained
GPUI views without transferring ownership of the process to GPUI.

## The ownership problem

Bevy and GPUI are both capable of acting as a desktop application host. Each
normally expects to control some combination of the event loop, native windows,
GPU surfaces, and presentation. Letting both frameworks start those resources
would produce two application lifecycles and incompatible renderer ownership.

`bevy_gpui` chooses one host: Bevy.

- Bevy owns the process event loop and application runner.
- Bevy owns native windows and their Winit integration.
- Bevy owns the WGPU adapter, device, queue, command submission, and presentation.
- Bevy owns ECS schedules, camera targets, and AccessKit adapters.
- GPUI owns retained views, layout, focus, event dispatch, painting, and scene
  generation.

## The embedded approach

The vendored GPUI revision adds a host-neutral `EmbeddedApplication`. It starts
GPUI without entering `Platform::run`. `BevyPlatform` and
`BevyPlatformWindow` implement the services GPUI needs using synchronized Bevy
state, but the GPUI window is virtual: it never creates a second native window
or WGPU surface.

```text
OS / Winit
    |
    v
Bevy Window + input messages
    |
    +----> BevyPlatformWindow ----> GPUI retained view
    |                                  |
    |                                  v
    |                            SceneSnapshot
    |                                  |
    v                                  v
Bevy main world ---- extraction ----> Bevy RenderApp
                                          |
                                          v
                                existing camera ViewTarget
                                          |
                                          v
                                   Bevy presentation
```

This keeps existing Bevy applications structurally unchanged: they still use
`DefaultPlugins` and `.run()`.

## Camera-bound roots

Every retained root is associated with one Bevy camera entity through a
`GpuiContext` component. The camera determines:

- the native window or `Image` render target;
- logical viewport bounds used for GPUI layout;
- viewport offsets used for input coordinates on native-window targets;
- physical viewport and scissor used during rendering;
- camera order when multiple roots overlap in one window.

The highest-order context under the cursor receives pointer events last and is
the focused keyboard context after a press. An explicit root always takes
precedence over automatic primary-camera attachment.

## Main-world flow

The integration installs these public boundaries:

```text
PreUpdate:
  DriveExecutor -> Input -> ApplyDeferredBridge

Update:
  application gameplay and GPUI synchronization systems

PostUpdate:
  WindowSync -> BuildScene
```

1. The dispatcher runs queued main-thread GPUI tasks and expired timers.
2. Bevy window and input messages become GPUI `PlatformInput` events.
3. Commands and messages queued by GPUI callbacks are applied to Bevy.
4. Application systems read Bevy state and update retained roots.
5. Camera, window, monitor, cursor, focus, theme, and IME state are synchronized.
6. Dirty GPUI windows lay out and paint a new immutable `SceneSnapshot`.

The last complete snapshot remains attached to the camera between updates, so
reactive applications do not need to rebuild retained scenes every frame.

## Render-world flow

`GpuiScene` derives Bevy's extraction support when `render` is enabled. Its
`Arc<SceneSnapshot>` crosses the pipelined render boundary without carrying a
native window, surface, or main-thread GPUI entity.

The render pass:

1. Selects the camera's `ViewTarget` and physical viewport.
2. Reuses a GPUI renderer keyed by extracted view entity and target format.
3. Resolves external Bevy images to prepared `GpuImage` texture views.
4. Records GPUI commands into Bevy's existing command encoder with
   `LoadOp::Load`.
5. Leaves command submission and presentation to Bevy.

One `WgpuAtlas` is shared per Bevy render-device generation. This is a
zero-copy integration in the practical sense: prepared Bevy image textures and
the camera target remain on the GPU and are referenced directly rather than
read back to CPU memory.

### Render ordering

`GpuiRenderOrder` controls whether the pass runs below or above Bevy UI. In both
cases it runs after the main 2D/3D camera pass and before upscaling and
presentation.

### Filters

Backdrop and content filters need the already-rendered camera color. For a
full-target camera, the pass asks `ViewTarget` for a post-process source and
destination, samples the source, and records GPUI into the destination.

Cropped or non-zero-origin viewports are supported for unfiltered GPUI.
Filtered cropped viewports cause the renderer to log an error and skip that
GPUI scene because its filter intermediates currently assume a full,
origin-anchored target.

## Input ownership

Bevy input messages are broadcast: one `MessageReader` cannot delete a message
before another system sees it. The plugin therefore reports ownership instead
of pretending to consume events globally.

For pointer input, GPUI reports whether hit testing found a retained element or
an element held pointer capture. `GpuiInputState` persists that claim per Bevy
window until a later pointer event clears or replaces it. Applications order raw
input systems after `GpuiSystems::Input`. Message-reading systems always drain
and filter against `GpuiInputState`; polling systems can use the public run
conditions.

When Bevy's picking plugin is present and the `picking` feature is enabled,
`bevy_gpui` emits a non-hoverable blocker hit at `f32::MAX` for every Bevy
pointer located in a claimed window. It stops lower scene entities without
becoming the click target in the normal picking pipeline.

Keyboard input routes to the focused GPUI context. The bridge preserves logical
key names, physical key identity, modifier state, text insertion, and IME
composition. Text handlers exchange UTF-16 ranges with platform IME APIs while
Bevy's messages use UTF-8 strings.

Bevy's pinch message carries no window entity. Pinch routing is therefore only
well-defined for a single active window/context; multi-window pinch is an
explicit limitation.

## Safe ECS interoperability

A retained callback can outlive the Bevy system that created it and can execute
while the GPUI application is already borrowed. Giving it a live `World`
reference would violate Bevy aliasing rules and invite reentrant mutation.

`BevyAppContextExt` instead pushes type-erased, `Send` commands or messages into
a thread-safe queue. Bevy drains that queue at
`GpuiSystems::ApplyDeferredBridge`. Reads move the other way through ordinary
Bevy systems calling `GpuiContexts::update`.

The trade-off is one boundary of latency: callback writes are not visible until
the bridge runs. In return, retained callbacks never hold an unsafe world
pointer.

## Lifecycle and recovery

Removing `GpuiContext` closes the associated GPUI window and drops its root
factory. Closing a Bevy window first deactivates cameras targeting it so the
pipelined renderer cannot consume a target that is entering teardown. If an app
manually despawns a target window, it should deactivate or retarget its camera
first.

Bevy can replace its render device. The integration assigns each published
atlas a generation. When the generation changes, it shuts down the old
embedded application, drops renderer caches, starts a new embedded application,
and invokes every live root builder again. This is why root builders must be
reusable and reconstruct state from a durable source.

## Trade-offs

- The vendored GPUI fork is a maintenance burden, but it keeps the embedding
  APIs host-neutral and testable.
- GPUI native application services cannot all be meaningful when Bevy owns the
  host. Unsupported prompts, menus, credential storage, and accessibility are
  reported explicitly.
- Camera-bound roots make target ownership unambiguous, but applications with
  layered cameras must choose which camera owns each overlay.
- Broadcast Bevy input requires explicit drain-and-filter handling for gameplay
  message readers and run conditions for polling systems.

## Rejected alternative

The initial prototype replaced Bevy's runner and let GPUI own native windows.
That architecture breaks existing Bevy applications, creates ambiguous GPU and
presentation ownership, and bypasses Bevy's normal window lifecycle. It is not
a supported integration mode.

## Related

- [Public API reference](reference.md)
- [How to route input and Bevy picking](how-to-input-and-picking.md)
- [Compatibility and limitations](compatibility.md)
- [Original integration specification](integration-spec.md)
