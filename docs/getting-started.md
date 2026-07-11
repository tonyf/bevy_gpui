# Build your first retained GPUI overlay

You will add a clickable GPUI panel to a normal Bevy camera. By the end, Bevy
still owns the application runner and native window while GPUI owns the retained
panel and its interaction state.

## What you need

- Rust 1.95.0 or newer.
- A desktop target supported by Bevy 0.19.
- A local checkout of this repository while `bevy_gpui` remains unpublished.
- On Debian or Ubuntu, the native packages used by CI:

  ```bash
  sudo apt-get install --no-install-recommends \
    g++ pkg-config libx11-dev libasound2-dev libudev-dev \
    libxkbcommon-x11-0 libfontconfig-dev libwayland-dev \
    libx11-xcb-dev libxcb1-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libxkbcommon-x11-dev
  ```

  This combines Bevy 0.19's
  [Linux dependencies](https://github.com/bevyengine/bevy/blob/v0.19.0/docs/linux_dependencies.md)
  with the additional packages used by this repository's CI.

## Step 1: Create a Bevy project

```bash
cargo new gpui-overlay-demo
cd gpui-overlay-demo
```

Add Bevy and the integration to `Cargo.toml`:

```toml
[dependencies]
bevy = "0.19"
bevy_gpui = { path = "../bevy_gpui" }
```

Place the `bevy_gpui` checkout beside `gpui-overlay-demo`, or adjust the relative
path. No committed remote revision contains this in-progress 0.1.0 integration
yet. Once one is published, pin that exact Git revision for reproducible builds.

## Step 2: Add the retained view

Replace `src/main.rs` with the source from
[`examples/getting_started.rs`](../examples/getting_started.rs):

```rust
use bevy::prelude::*;
use bevy_gpui::{
    GpuiContexts, GpuiPlugin,
    gpui::{Context, IntoElement, Render, Window, div, prelude::*, rgb, rgba},
};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GpuiPlugin::default()))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let camera = commands.spawn(Camera2d).id();
    gpui.set_root(camera, |_, cx| cx.new(|_| WelcomePanel { clicks: 0 }))
        .expect("GPUI root should be queued");
}

struct WelcomePanel {
    clicks: u32,
}

impl Render for WelcomePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_8().child(
            div()
                .w_96()
                .p_5()
                .rounded_lg()
                .bg(rgba(0x18_18_1b_e8))
                .text_color(rgb(0xf4_f4_f5))
                .child(div().text_xl().child("Hello from GPUI"))
                .child("Bevy owns the window and camera.")
                .child(format!("Button clicks: {}", self.clicks))
                .child(
                    div()
                        .id("increment")
                        .mt_3()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0x25_63_eb))
                        .hover(|style| style.bg(rgb(0x3b_82_f6)))
                        .cursor_pointer()
                        .child("Increment")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.clicks += 1;
                            cx.notify();
                        })),
                ),
        )
    }
}
```

`GpuiContexts::set_root` associates the retained root with the camera. The
closure builds one GPUI entity. `cx.notify()` marks the retained view dirty
after the button changes its state.

## Step 3: Run it

```bash
cargo run
```

A Bevy window opens with a dark GPUI panel. Clicking **Increment** changes the
counter. There is no custom runner or second native window.

Inside this repository, the same source is checked as a Cargo example:

```bash
cargo check --example getting_started
cargo run --example getting_started
```

## Step 4: Keep the typed handle

Local GPUI state works for a self-contained widget. To synchronize Bevy state
into the view later, store the returned `GpuiViewHandle<V>` in a resource:

```rust
use bevy_gpui::GpuiViewHandle;

#[derive(Resource)]
struct WelcomeRoot(GpuiViewHandle<WelcomePanel>);

fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let camera = commands.spawn(Camera2d).id();
    let root = gpui
        .set_root(camera, |_, cx| cx.new(|_| WelcomePanel { clicks: 0 }))
        .expect("GPUI root should be queued");
    commands.insert_resource(WelcomeRoot(root));
}
```

An ordinary Bevy system can then call `GpuiContexts::update`. The
[ECS synchronization guide](how-to-ecs-bridge.md) covers both directions.

## What you built

You built a retained GPUI view attached to a Bevy camera. Bevy still owns the
application lifecycle, and GPUI handles layout, painting, hover state, and the
button callback.

Next:

- [Route input and Bevy picking correctly](how-to-input-and-picking.md).
- [Synchronize GPUI and Bevy ECS](how-to-ecs-bridge.md).
- [Read the public API reference](reference.md).
