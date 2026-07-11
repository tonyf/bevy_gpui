# How to use render targets and Bevy images

Render a retained GPUI root into a Bevy camera target, and paint prepared Bevy
image assets inside GPUI without CPU readback.

## Prerequisites

- The default `render` feature.
- A normal Bevy `RenderPlugin`, `WindowPlugin`, and `WinitPlugin`, usually
  provided by `DefaultPlugins`.
- An explicit camera entity for each retained root.

## Attach an overlay to a window camera

Pass the camera entity to `GpuiContexts::set_root`:

```rust
fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let camera = commands.spawn(Camera3d::default()).id();
    gpui.set_root(camera, |_, cx| cx.new(|_| OverlayView))
        .expect("overlay root should be queued");
}
```

For a native-window target, the camera's logical viewport drives GPUI layout
and input coordinates. Its physical viewport drives the render origin, size,
and scissor. An offscreen `RenderTarget::Image` root renders normally but
receives no automatic native pointer or keyboard input because it is not mapped
to a Bevy `Window` entity.

## Render GPUI into a Bevy `Image`

Create a render-target image and point a camera at it:

```rust
use bevy::{
    camera::RenderTarget,
    prelude::*,
    render::render_resource::TextureFormat,
};

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut gpui: GpuiContexts,
) {
    let target = images.add(Image::new_target_texture(
        512,
        512,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));

    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
        ))
        .id();

    gpui.set_root(camera, |_, cx| cx.new(|_| RenderTargetPanel))
        .expect("texture root should be queued");

    // Use `target` in a Bevy material, sprite, or another camera pipeline.
}
```

The GPUI pass records into the same texture view that Bevy prepared for the
camera. It does not acquire or present a native surface.

Bevy images use a top-left UV origin. Check the mesh or material consuming a
render target if the retained UI appears vertically inverted. The
[`render_to_texture`](../examples/render_to_texture.rs) example flips the front
face UVs of its demonstration mesh.

`RenderTargetPanel` can be any type that implements GPUI's `Render` trait, such
as the panel from the getting-started tutorial.

## Paint a Bevy image inside GPUI

Load an image, wrap its strong handle, and capture a clone in a root builder:

```rust
fn attach_image_panel(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut gpui: GpuiContexts,
) {
    let camera = commands.spawn(Camera2d).id();
    let logo = GpuiBevyImage::new(asset_server.load("logo.png"));

    gpui.set_root(camera, move |_, cx| {
        let logo = logo.clone();
        cx.new(move |_| ImagePanel { logo })
    })
    .expect("image panel should be queued");
}
```

Store the wrapper in the retained view and render its styleable canvas:

```rust
struct ImagePanel {
    logo: GpuiBevyImage,
}

impl Render for ImagePanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .child("Bevy image inside GPUI")
            .child(bevy_image(self.logo.clone()).w_32().h_32().rounded_lg())
    }
}
```

The render bridge maps the stable external-surface ID to Bevy's prepared
`GpuImage`. If the image is not prepared yet, the pass uses Bevy's fallback
texture, marks the lookup missing, and requests another retained frame.

The registry releases its mapping after the final `GpuiBevyImage` clone is
dropped. Keep a clone in retained state for as long as the image should render.

## Render into HDR

Add Bevy's `Hdr` component to the camera:

```rust
commands.spawn((Camera3d::default(), Hdr));
```

The GPUI renderer specializes for the camera target format, including
`Rgba16Float`. Run `cargo run --example hdr_overlay` for the complete setup.
The renderer path is tested, but cross-platform golden color validation is
still pending.

## Use backdrop and content filters

Filters work when the camera covers the full render target. They use Bevy's
post-process source as the already-painted background:

```rust
div()
    .bg(rgba(0x18_18_1b_c0))
    .backdrop_blur(px(12.0)) // Blur the camera content behind the panel.
    .child(
        div()
            .blur(px(4.0)) // Blur this element and its retained children.
            .child("Filtered content"),
    )
```

The vendored GPUI example
[`examples/learn/blur.rs`](../vendor/gpui-ce/crates/gpui/examples/learn/blur.rs)
shows the complete retained styling API.

Do not combine GPUI filters with a cropped or non-zero-origin camera viewport.
That path logs an explicit renderer error and skips recording that GPUI scene
because the current filter intermediates assume a full, origin-anchored target.
Unfiltered GPUI remains supported in cropped viewports.

## Select ordering relative to Bevy UI

```rust
GpuiPlugin {
    render_order: GpuiRenderOrder::BelowBevyUi,
    ..default()
}
```

Use `BelowBevyUi` when Bevy UI should cover GPUI. The default
`AboveBevyUi` places GPUI over Bevy UI.

## Verification

```bash
cargo run --example render_to_texture
cargo run --example hdr_overlay
```

The first command shows a GPUI-produced Bevy texture on a rotating 3D object,
including a Bevy image painted inside GPUI. The second shows GPUI over a live
HDR camera.

## Troubleshooting

### A target stays blank

Confirm the target camera is active, has non-zero physical dimensions, and
still owns a `GpuiContext`. Check `GpuiRuntimeStatus::roots` and `scenes_built`.

### A Bevy image shows the fallback texture

Keep a strong `Handle<Image>` through `GpuiBevyImage`, confirm the asset reached
Bevy's render world, and allow another frame after preparation.

### GPUI appears behind another interface

Set `GpuiRenderOrder::AboveBevyUi`, or inspect camera ordering if another camera
renders over the target later.

## Related

- [Public API reference](reference.md#bevy-image-interop)
- [Architecture](architecture.md#render-world-flow)
- [Compatibility and limitations](compatibility.md#render-targets)
