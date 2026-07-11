//! Minimal camera-bound retained GPUI overlay.

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
