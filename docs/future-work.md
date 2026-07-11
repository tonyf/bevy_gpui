# Future work

This page turns the open limitations exposed by the adversarial API,
newcomer, and maintainer reviews into an engineering backlog. It describes
work that is not complete in the current checkout. For the support contract of
the code that exists today, read [Compatibility and limitations](compatibility.md).

The review also found documentation and implementation defects that were fixed
immediately. Those closed findings are not repeated here. This page keeps only
the underlying product, correctness, validation, and maintenance gaps that
still require engineering work.

## Priority definitions

| Priority | Meaning |
|---|---|
| P0 | Required before calling the integration release-ready across its claimed desktop platforms |
| P1 | Correctness or adoption gap that should be addressed before production use in the affected scenario |
| P2 | Capability, ergonomics, or operational work that broadens the set of applications the crate can serve |
| P3 | Optional host-service parity that should remain outside the core bridge unless demand justifies it |

The priorities express dependency and risk, not calendar estimates.

## Roadmap summary

| Priority | Workstream | Current limitation | Completion signal |
|---|---|---|---|
| P0 | Reproducible distribution | Consumers can only use a path dependency from this uncommitted checkout | A clean external project builds from a published immutable revision or package |
| P0 | Desktop platform validation | Only macOS has native runtime and interaction evidence | Linux X11, Linux Wayland, and Windows pass CI plus native smoke tests |
| P0 | Automated interaction regression | The strongest click, focus, and lifecycle checks still depend partly on native manual runs | CI or a repeatable native harness proves the critical interaction invariants |
| P1 | Scoped input ownership | Public claims are aggregate across windows and an entire input pass | Gameplay can query ownership for the relevant window, pointer, and event class |
| P1 | Precise picking and capture | Picking blocks a claimed window and capture ends outside the mapped viewport | Blocking and capture follow the actual pointer and retained hit region |
| P1 | Root state and teardown | Root failures are logged late and manual window/camera teardown has an unsafe ordering | Every root has queryable state/errors and teardown is safe in either order |
| P1 | Viewport filter support | Filters skip GPUI rendering in cropped or non-zero-origin viewports | Filtered retained content renders correctly inside arbitrary camera viewports |
| P1 | Accessibility | GPUI content is absent from Bevy's AccessKit tree | One native accessibility tree exposes GPUI nodes and routes actions correctly |
| P2 | Offscreen and gesture input | Image targets have no input adapter and pinch has no reliable multi-window route | Applications can inject offscreen input and route gestures to an explicit context |
| P2 | Visual parity and performance | Cross-platform goldens, idle-power data, and allocation baselines do not exist | Repeatable baselines detect output and performance regressions |
| P2 | Host-window correctness | Several GPUI window queries return fixed or empty values | Supported setters and queries round-trip through Bevy state |
| P3 | Optional native services | Dialogs, menus, credentials, reopen, and similar services are unsupported | Opt-in host adapters implement only the services applications request |

## P0: make releases reproducible

The current installation uses a path dependency because the integration is not
present on an immutable remote revision and the crate has `publish = false`.
That prevents a newcomer from validating the documented setup outside this
working tree.

Required work:

1. Commit the root integration and the complete, pruned vendored GPUI source.
2. Decide whether releases will ship the vendored crates, use published GPUI
   crates, or pin a Git revision that contains the required embedding seams.
3. Add the repository, readme, license, and package metadata required by that
   distribution model.
4. Verify `cargo package --list` does not include build artifacts or omit
   required GPUI assets.
5. Build the packaged artifact from a clean directory with no path back to the
   maintainer checkout.
6. Replace the temporary path installation snippet with an immutable version or
   exact revision.

Done when a new Bevy project can copy the documented dependency declaration,
build from a clean machine, and run `getting_started` without access to this
checkout.

## P0: close the desktop validation matrix

Linux and Windows jobs are configured, but the project does not yet have the
first recorded green integration runs or native visual and interaction parity
on those platforms. A compile-only pass is not enough evidence for window,
input, text, DPI, or GPU-presentation behavior.

Required work:

- Get the full root and focused vendored test matrix green on macOS, Linux, and
  Windows from a clean checkout.
- Run separate native X11 and Wayland sessions. The default Linux feature set
  compiling both backends does not prove either session works.
- Launch every example on Windows, X11, and Wayland and record the GPU/backend,
  scale factor, and result.
- Exercise at least 100%, 150%, and 200% scale factors where the platform
  supports them.
- Capture platform-labelled SDR, HDR, alpha, viewport, multi-window, text/IME,
  and filter artifacts.
- Keep compile/test validation and native runtime validation as separate status
  labels in [Compatibility and limitations](compatibility.md).

Done when every claimed desktop backend has a green CI result and a dated
native smoke record covering rendering and input.

## P0: automate the critical native invariants

The examples contain counters and synthetic probes, but `cargo test --examples`
mostly proves that examples compile. Manual screenshots also cannot prove the
absence of a transient input or lifecycle failure.

Build a native test harness that can launch examples, wait for an explicit
ready signal, inject input, read deterministic counters, and retain screenshots
and logs on failure. It must cover these invariants:

- A click on GPUI increments the GPUI callback count and does not increment raw
  Bevy scene input or Bevy picking counts.
- A click outside the GPUI hit region reaches the Bevy scene.
- A message-reading gameplay system drains UI-directed messages instead of
  replaying them after the claim clears.
- Text, physical keys, modifiers, and IME preedit/commit reach the focused GPUI
  handler without firing gameplay shortcuts.
- Two windows route pointer and keyboard input only to their own roots.
- Removing a context and closing a window both release their roots.
- Render-to-texture output has the expected orientation, and device recovery
  reconstructs roots without stale GPU resources.

The harness should preserve the deterministic counters as the source of truth;
screenshots are supporting visual evidence.

Done when these cases run repeatedly without manual interpretation and produce
enough artifacts to diagnose a failure.

## P1: expose scoped input ownership

`GpuiInputState` currently exposes aggregate booleans. Pointer claims are keyed
by window internally, but application code cannot ask which window claimed the
pointer. Keyboard ownership is also aggregate. One interactive GPUI window can
therefore pause a global gameplay system that is operating on another window.

Design a public query surface that can answer at least:

- whether a specific Bevy window claims pointer or keyboard input;
- whether a specific pointer is blocked from Bevy picking;
- which input class caused the current claim;
- whether a claim is a retained capture or a hit from the current dispatch;
- when the claim was last updated or cleared.

Do not expose the private runtime maps directly. A stable resource or
`SystemParam` should preserve the option to change internal context IDs.

Done when two-window gameplay can gate only the affected window and a
multi-pointer picking test blocks only the pointer that hit GPUI.

## P1: provide an event-correlated gameplay contract

Bevy broadcasts input messages to independent readers, and its cursor, button,
wheel, keyboard, and gesture streams do not provide one total OS-event order.
The current drain-and-filter pattern is correct for the available aggregate
state, but it is easy for application authors to put a `MessageReader` behind a
run condition and replay stale UI input later.

Explore a first-class helper such as a filtered reader, an input-routing
receipt, or GPUI-aware gameplay messages. The design must:

- always advance the underlying Bevy reader;
- associate a decision with a window and input class;
- avoid claiming a total cross-stream order that Bevy does not provide;
- preserve polling run conditions for systems without message cursors;
- define dense mixed input behavior within one Bevy update;
- remain usable without Bevy picking.

Done when application code cannot accidentally retain a UI-directed message
merely by using the supported helper, and regression tests cover interleaved
move, button, wheel, and keyboard messages.

## P1: make picking and pointer capture precise

The automatic picking backend currently emits a maximum-order blocker for
every Bevy pointer located in a claimed window. This protects the common
single-pointer case, but the claim is wider than the retained hit region and
wider than one pointer. Pointer capture also clears when the pointer leaves the
camera viewport or native window because the adapter stops dispatching to that
context.

Required work:

- Track claims by pointer and context, not only by native window.
- Re-run or preserve the GPUI hit result for the pointer position used by the
  picking backend.
- Block only the corresponding pointer while preserving independent pointers
  and non-overlapping camera viewports.
- Preserve a captured context for move and release events outside its viewport,
  translating out-of-bounds coordinates consistently.
- Clear capture on release, focus loss, context removal, window teardown, and
  cancellation.
- Decide whether click interval and distance thresholds should use platform
  settings or public configuration instead of fixed 500 ms and four logical
  pixels.

Done when multi-pointer, overlapping-camera, drag-outside, focus-loss, and
teardown tests show no scene click leak and no unrelated picking blockage.

## P1: make root creation and teardown observable

`GpuiContexts::set_root` can queue a root before its camera exists in the
applied ECS world. Camera/component validation and virtual-window creation
happen later. A deferred failure is logged and can leave the caller with a
pending or stale handle. `GpuiRuntimeStatus` reports aggregate counts but not
which root failed or why.

Add a per-root lifecycle model with states such as queued, waiting for GPU,
materialized, rebuilding, failed, and removed. Expose failures through a
queryable resource, message, or result-bearing asynchronous handle. Each error
should identify the camera entity, attempted target, and recovery action.

Also remove the manual teardown ordering requirement. Despawning a native
window and a camera that still targets it in one application operation should
not leave an extracted view pointing at an invalid target. The bridge should
deactivate, retarget, or safely ignore the camera before render extraction.

Done when:

- an invalid camera entity produces a structured root error rather than only a
  log line;
- a caller can distinguish waiting-for-GPU from permanently failed;
- a rebuilt root transitions through observable recovery state;
- window-first, camera-first, and same-frame teardown all pass native tests.

## P1: support filters in arbitrary camera viewports

Unfiltered GPUI renders into cropped and offset viewports, but backdrop or
content filters currently require a full, zero-origin target. The external
renderer assumes target-sized, origin-anchored intermediate textures and skips
the GPUI scene when that assumption is false.

The renderer needs viewport-aware filter intermediates, background sampling,
coordinate transforms, clipping, and copy regions. Tests must cover offset
viewports, nested filters, HDR targets, multiple cameras sharing a target, and
render-to-image targets.

If full support is not feasible in one change, add an explicit fallback policy
that can render the retained content without filters instead of dropping the
whole overlay after logging an error. The policy and diagnostic must be
observable to the application.

Done when filtered and unfiltered content occupy the same expected viewport
bounds on SDR and HDR targets without affecting neighboring camera regions.

## P1: integrate accessibility through Bevy

Accessibility is currently disabled because both frameworks expect to own an
AccessKit adapter for the native window. Installing a second adapter is not a
valid solution.

Design one composed accessibility tree in which GPUI nodes are mounted below a
stable Bevy-owned subtree. Route focus, value, default-action, scroll, and text
selection actions back to the matching GPUI context. Handle context removal,
multiple cameras in one window, multiple windows, and DPI changes without
replacing Bevy's root tree.

Done when native screen readers can discover, focus, activate, and read GPUI
controls alongside Bevy accessibility nodes on every supported desktop
platform. Until then, the project must continue to state that GPUI content is
not accessible.

## P2: add explicit offscreen and gesture input routing

`RenderTarget::Image` roots receive no automatic native input because an image
has no inherent window, viewport, or pointer coordinate space. Pinch routing is
also limited to one active context because Bevy's `PinchGesture` carries no
window entity.

Provide an explicit input-injection API for offscreen roots. It should accept a
target context, logical coordinates, scale, input event, and focus/capture
policy. Use the same path for applications that display a render target on a
3D surface and need to map a ray hit into texture coordinates.

For gestures, either consume a future Bevy event that includes a window or add
an application-supplied routing hook. Never guess among multiple focused
windows.

Done when two offscreen roots can receive independent pointer and keyboard
input and two native windows can route gestures without selecting an arbitrary
context.

## P2: establish visual and performance baselines

The renderer path has focused tests and macOS screenshots, but no automated
cross-platform golden comparison. The project also lacks long-running idle
power, allocation, frame-time, atlas-growth, and recovery baselines.

Add repeatable measurements for:

- SDR, HDR, premultiplied alpha, filters, external Bevy images, text, DPI, and
  viewport clipping;
- idle applications with clean retained scenes and no expired timers;
- continuously dirty views and large text/image atlases;
- executor saturation near the 1,024 foreground-task-per-frame bound;
- device recovery and repeated root creation/removal;
- one, several, and many camera-bound roots.

Record the environment and variance before choosing regression thresholds.
Goldens need per-backend tolerances where rasterization legitimately differs.

Done when CI or scheduled native jobs can detect a visual regression, idle
wakeup loop, persistent allocation growth, or material frame-time regression.

## P2: make supported host-window state round-trip

Several adapter methods accept a GPUI request but their matching query returns
a fixed or inherited value. Examples include the title query, maximized state,
and window bounds. Close veto is observed but cannot override Bevy's default
close policy.

First make the supported subset internally consistent:

- cache or query the current Bevy title after GPUI changes it;
- report maximized and windowed bounds from synchronized Bevy window state;
- expose a documented Bevy close-policy integration for GPUI veto;
- test minimize, maximize, fullscreen, activation, resize, theme, cursor, and
  IME-position setter/query behavior in multiple windows.

Done when every service labelled supported or partial in the compatibility
table has a round-trip test, and unsupported queries return a deliberate error
or capability result instead of misleading data where GPUI permits it.

## P2: evaluate touch, mobile, and web as separate hosts

Native touch, Android, iOS, and Web/Wasm are unsupported. They should not be
enabled by scattering target conditionals through the desktop adapter. Each
host needs an explicit ownership design for windows or canvases, touch and IME,
clipboard and URLs, accessibility, task execution, and GPU presentation.

Start with a design and a minimal rendering/input probe for one host. Keep the
single-WGPU-identity requirement and Bevy-owned runner rule unless that host's
Bevy architecture proves a different boundary is necessary.

Done for a host only when it has CI, a native or browser interaction smoke test,
and an honest capability table. A compiling target alone is not support.

## P3: keep optional native services modular

Native file/save/message prompts, credentials, menus, reopen callbacks, URL
scheme registration, keyboard-layout changes, screen capture, and several
desktop-shell features remain unsupported. Implementing all of them inside the
core Bevy bridge would add platform policy and dependencies that many games and
tools do not need.

Define capability traits or opt-in plugins before adding these services. An
application should be able to provide its own dialog, credential, menu, or URL
handler while the core adapter remains host-neutral. Unsupported calls must
continue to fail or warn explicitly rather than pretend to succeed.

Prioritize concrete adopter demand. Title/bounds correctness, close policy, and
accessibility belong ahead of convenience services.

## Sequencing

The recommended order is:

1. Publish a reproducible source artifact and get the configured desktop CI
   matrix green.
2. Build the automated native interaction harness before changing more input
   semantics.
3. Add scoped claims, event-aware filtering, precise picking, and full capture.
4. Add root lifecycle diagnostics and teardown safety.
5. Implement cropped-viewport filters and accessibility.
6. Establish visual/performance baselines.
7. Add offscreen input and optional host services in response to real adopter
   needs.
8. Treat touch, mobile, and web as explicit host projects, not feature toggles.

This order creates evidence before expanding the support surface and gives the
input and lifecycle refactors a regression harness to protect them.

## Architectural constraints to preserve

Future work must not regress the decisions that make the integration usable in
existing Bevy applications:

- Bevy owns the runner, event loop, native windows, renderer, command
  submission, and presentation.
- GPUI records retained scenes into Bevy-owned camera targets.
- Bevy and GPUI share one WGPU package identity, adapter, device, and queue.
- Retained callbacks communicate with Bevy through deferred commands and
  messages, not a live `World` reference.
- Roots bind to explicit cameras; code must not select an arbitrary first
  window or camera.
- Unsupported host services remain explicit instead of silently succeeding.
- Bevy-specific types stay out of the vendored GPUI embedding seams.

## Maintaining this page

When a work item lands:

1. Add its regression coverage before removing the limitation.
2. Update [Compatibility and limitations](compatibility.md) with the new support
   evidence.
3. Update the affected [API reference](reference.md), how-to guide, Rustdoc, and
   changelog entry.
4. Remove or narrow the item here. Do not leave completed work described as a
   current gap.

## Related

- [Compatibility and limitations](compatibility.md)
- [Architecture](architecture.md)
- [How to route input and Bevy picking](how-to-input-and-picking.md)
- [How to use render targets and Bevy images](how-to-render-targets.md)
- [Maintainer guide](maintainers.md)
- [Integration specification](integration-spec.md)
