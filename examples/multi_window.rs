//! Two Bevy-owned windows, each with its own camera and retained GPUI root.

use bevy::{camera::RenderTarget, prelude::*, window::WindowRef};
use bevy_gpui::{
    GpuiContexts, GpuiPlugin,
    gpui::{Context, IntoElement, Render, Window as GpuiWindow, div, prelude::*, rgb, rgba},
};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GpuiPlugin::default()))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let primary_camera = commands.spawn(Camera2d).id();

    let second_window = commands
        .spawn(Window {
            title: "bevy_gpui second window".into(),
            ..default()
        })
        .id();
    let second_camera = commands
        .spawn((
            Camera2d,
            RenderTarget::Window(WindowRef::Entity(second_window)),
        ))
        .id();

    gpui.set_root(primary_camera, |_, cx| {
        cx.new(|_| WindowLabel::new("Primary Bevy window", 0x25_63_eb))
    })
    .expect("primary GPUI root should be queued");
    gpui.set_root(second_camera, |_, cx| {
        cx.new(|_| WindowLabel::new("Second Bevy window", 0x7c_3a_ed))
    })
    .expect("second GPUI root should be queued");
}

struct WindowLabel {
    label: &'static str,
    color: u32,
    clicks: u32,
}

impl WindowLabel {
    fn new(label: &'static str, color: u32) -> Self {
        Self {
            label,
            color,
            clicks: 0,
        }
    }
}

impl Render for WindowLabel {
    fn render(&mut self, _: &mut GpuiWindow, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_6().child(
            div()
                .w_96()
                .p_5()
                .rounded_lg()
                .bg(rgba(0x18_18_1b_e8))
                .text_color(rgb(0xf4_f4_f5))
                .child(self.label)
                .child(format!("Clicks routed to this window: {}", self.clicks))
                .child(
                    div()
                        .id("increment")
                        .mt_3()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(self.color))
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
