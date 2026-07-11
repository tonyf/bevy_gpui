//! Self-checking teardown for removed GPUI contexts and Bevy windows.

use bevy::{
    camera::RenderTarget,
    prelude::*,
    window::{WindowRef, WindowResolution},
};
use bevy_gpui::{
    GpuiContext, GpuiContexts, GpuiPlugin, GpuiRuntimeStatus,
    gpui::{Context, IntoElement, Render, Window as GpuiWindow, div, prelude::*, rgb},
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            GpuiPlugin {
                auto_attach_primary: false,
                ..default()
            },
        ))
        .add_systems(Startup, setup)
        .init_resource::<LifecycleProbe>()
        .add_systems(Update, verify_lifecycle)
        .run();
}

#[derive(Resource)]
struct LifecycleEntities {
    explicit_camera: Entity,
    window_camera: Entity,
    secondary_window: Entity,
}

#[derive(Resource)]
struct LifecycleProbe {
    phase: u8,
    presentation: Timer,
}

impl Default for LifecycleProbe {
    fn default() -> Self {
        Self {
            phase: 0,
            presentation: Timer::from_seconds(10.0, TimerMode::Once),
        }
    }
}

fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let explicit_camera = commands.spawn(Camera2d).id();
    gpui.set_root(explicit_camera, |_, cx| cx.new(|_| LifecycleView))
        .expect("lifecycle GPUI root should be queued");
    let secondary_window = commands
        .spawn(bevy::window::Window {
            title: "bevy_gpui lifecycle secondary".into(),
            resolution: WindowResolution::new(320, 240),
            ..default()
        })
        .id();
    let window_camera = commands
        .spawn((
            Camera2d,
            RenderTarget::Window(WindowRef::Entity(secondary_window)),
        ))
        .id();
    gpui.set_root(window_camera, |_, cx| cx.new(|_| LifecycleView))
        .expect("secondary-window GPUI root should be queued");
    commands.insert_resource(LifecycleEntities {
        explicit_camera,
        window_camera,
        secondary_window,
    });
}

fn verify_lifecycle(
    mut commands: Commands,
    time: Res<Time>,
    entities: Res<LifecycleEntities>,
    status: Res<GpuiRuntimeStatus>,
    contexts: Query<(), With<GpuiContext>>,
    mut probe: ResMut<LifecycleProbe>,
    mut messages: ParamSet<(
        MessageWriter<bevy::window::WindowCloseRequested>,
        MessageWriter<AppExit>,
    )>,
) {
    if probe.phase == 0
        && status.roots == 2
        && status.scenes_built >= 2
        && probe.presentation.tick(time.delta()).just_finished()
    {
        commands
            .entity(entities.explicit_camera)
            .remove::<GpuiContext>();
        messages.p0().write(bevy::window::WindowCloseRequested {
            window: entities.secondary_window,
        });
        probe.phase = 1;
    } else if probe.phase == 1
        && status.roots == 0
        && contexts.get(entities.explicit_camera).is_err()
        && contexts.get(entities.window_camera).is_err()
    {
        info!("bevy_gpui context and host-window teardown verified");
        messages.p1().write(AppExit::Success);
    }
}

struct LifecycleView;

impl Render for LifecycleView {
    fn render(&mut self, _: &mut GpuiWindow, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_6()
            .bg(rgb(0x0f_17_2a))
            .text_color(rgb(0xf8_fa_fc))
            .child("This GPUI root will be removed by Bevy ECS.")
    }
}
