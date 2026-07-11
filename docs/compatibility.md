# Compatibility and limitations

This page separates implemented behavior, compile-time validation, native
runtime evidence, and unsupported capabilities. It is the authoritative support
statement for the current checkout.

## Version compatibility

| Component | Version |
|---|---|
| `bevy_gpui` | 0.1.0, unpublished |
| Bevy | 0.19 |
| Rust | 1.95.0 or newer |
| GPUI | Vendored `gpui-ce` revision `20340e14874a3b55122e5cb2aa0d023874e08b2d` plus documented embedding patches |
| WGPU | One crates.io package identity shared by Bevy and the vendored GPUI renderer |

Use `bevy_gpui::gpui` for GPUI imports. Adding a separate GPUI dependency can
introduce incompatible duplicate types even when the version labels look alike.

## Platform validation

| Platform | Current evidence | Status |
|---|---|---|
| macOS Apple Silicon | Native example launches, interaction testing, screenshots, and Metal-backed renderer tests | Runtime tested |
| Linux X11 | CI workflow configured for compile, Clippy, library tests, and focused GPUI renderer tests | First green integration run and native visual smoke pending |
| Linux Wayland | CI workflow configured with Wayland dependencies and the default feature set | First green integration run and native visual smoke pending |
| Windows | CI workflow configured for compile, Clippy, library tests, and focused GPUI renderer tests | First green integration run and native visual smoke pending |
| Web/Wasm | No supported host adapter or CI target | Unsupported |
| Android/iOS | GPUI lacks the required public touch input model and no mobile host is provided | Unsupported |

Do not interpret a green cross-platform CI build as proof of native visual or
input parity. Cross-platform golden screenshots remain future work.

## Render targets

| Target or behavior | Status |
|---|---|
| 2D camera window target | Supported |
| 3D camera window target | Supported |
| Camera viewport offset and scissor | Supported |
| Multiple cameras | Supported when each root is attached to one explicit camera |
| Multiple native Bevy windows | Supported |
| `RenderTarget::Image` | Rendering supported; no automatic native input routing |
| Prepared Bevy `Image` painted inside GPUI | Supported through `GpuiBevyImage` and `bevy_image` |
| `Rgba8Unorm`, sRGB BGRA, and `Rgba16Float` renderer paths | Covered by GPUI renderer tests |
| HDR visual color correctness | Renderer path works; cross-platform golden color validation pending |
| Transparent background | Premultiplied-alpha path implemented; platform screenshot matrix pending |
| Backdrop/content filters on a full camera target | Supported |
| Filters on a cropped or non-zero-origin viewport | Logs an error and skips that GPUI scene; unsupported |
| GPUI platform-native paint surfaces | Unsupported; use Bevy images or host-neutral external surfaces |

## Input

| Input | Status |
|---|---|
| Mouse move and buttons | Supported; click counts use 500 ms/four-pixel thresholds, and capture claims end outside the mapped viewport/window |
| Wheel scrolling | Supported with line and pixel units |
| Keyboard press/release | Supported with logical and physical key identity |
| Modifiers and Caps Lock | Supported |
| Text insertion | Supported |
| IME preedit and commit | Supported through `PlatformInputHandler` |
| Cursor enter/leave and focus | Supported |
| Pinch gestures | Single-window/active-context use only; Bevy's message has no window ID, so multi-window routing is ambiguous |
| File drag and drop | Supported |
| Bevy picking | Window-wide blocker for every pointer located in a claimed window when Bevy's `PickingPlugin` is already installed and `picking` is enabled |
| Raw gameplay input suppression | Aggregate pass-level claim; message readers must always drain and filter, while polling systems may use run conditions |
| Native touch | Unsupported |

## Window and host services

| Service | Status |
|---|---|
| GPUI requests for title, logical resize, minimize, maximize, fullscreen, and activation | Applied to Bevy window components |
| GPUI queries for maximized/windowed bounds | Partial: `is_maximized` reports false and `window_bounds` reports windowed |
| GPUI title query | Returns the inherited empty string even after a title write; unsupported |
| Window stacking and native button-layout queries | Return no platform information; unsupported |
| Cursor shape and visibility | Delegated to Bevy cursor components |
| Theme and active state | Synchronized from Bevy |
| IME candidate position | Delegated to Bevy's window IME position |
| Transparency request | Delegated to Bevy; blurred/mica native materials are not implemented |
| Text clipboard | Implemented with the host clipboard and an in-memory fallback |
| Open URL/path and reveal path | Implemented through the host OS |
| GPUI quit request | Converted to Bevy `AppExit::Success` |
| GPUI close veto | Observed and warned, but Bevy's `WindowPlugin` owns close policy |
| Reopen callbacks | Stored but never invoked; unsupported |
| Native file and save prompts | Explicit error; unsupported |
| Native message prompts | Warning and no prompt receiver; unsupported |
| Credential storage | Explicit error; unsupported |
| Application, dock, and context menus | Unsupported |
| URL scheme registration and incoming URL callbacks | Unsupported |
| Application restart/hide/unhide | Unsupported |
| Keyboard-layout identity and change callbacks | Dummy fixed layout; change callback unsupported |
| Thermal and GPU/subpixel capability reporting | Nominal/absent fixed values; change callback unsupported |
| Recent documents, jump lists, tabbing, edited-document state, character palette | Inherited GPUI platform defaults; unsupported no-ops |
| Client decorations, native window move/resize commands, input regions, exclusive zones, system bell | Inherited GPUI platform defaults; unsupported no-ops |
| Platform screen capture and platform-window render-to-image | Unsupported |

## Accessibility

Accessibility is disabled. Bevy already owns one AccessKit adapter per native
window, and neither Bevy nor GPUI exposes a supported way to merge GPUI's tree
as a subtree under Bevy's root. Installing a second adapter would be incorrect.

Do not claim screen-reader support for GPUI content until subtree updates and
actions are routed through Bevy's adapter and tested natively.

## Lifecycle guarantees

- Root creation may wait until Bevy publishes its render device.
- Root builders can run again after render-device replacement.
- Removing `GpuiContext` tears down that retained root.
- A normal `WindowCloseRequested` deactivates targeted cameras before teardown.
- Manually despawning a window and its active camera target in one step is not
  made safe automatically; deactivate or retarget the camera first.
- Foreground GPUI tasks are bounded to 1,024 runnable executions per Bevy frame.
- Offscreen image-target roots receive no automatic native pointer or keyboard
  input.

## Known validation gaps

- First green runs of the configured Linux and Windows integration CI jobs.
- Native Windows, X11, and Wayland screenshots and interaction automation.
- Cross-platform golden output for SDR, HDR, alpha, DPI, and filters.
- Accessibility subtree integration.
- Native touch/mobile support.
- Long-running idle-power and allocation benchmarks.

## Related

- [Future work](future-work.md)
- [Public API reference](reference.md)
- [Architecture](architecture.md)
- [Troubleshooting](troubleshooting.md)
- [Vendored GPUI patch](../vendor/gpui-ce/BEVY_GPUI_PATCH.md)
