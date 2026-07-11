//! Phase-zero acceptance application: an ordinary Bevy 3D app with
//! `GpuiPlugin` added and no custom runner.

use bevy::{
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
    window::{CursorMoved, PrimaryWindow},
};
use bevy_gpui::{
    BevyAppContextExt, GpuiContexts, GpuiInputState, GpuiPlugin, GpuiRuntimeStatus, GpuiSystems,
    GpuiViewHandle,
    gpui::{
        Context, IntoElement, PathBuilder, Render, Window, canvas, div, point, prelude::*, px, rgb,
        rgba,
    },
};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, MeshPickingPlugin, GpuiPlugin::default()))
        .add_systems(Startup, setup)
        .init_resource::<BevyClickCount>()
        .init_resource::<SceneClickCount>()
        .init_resource::<PickingClickCount>()
        .init_resource::<InteractionProbe>()
        .add_systems(
            Update,
            (rotate_cube, sync_clicks, verify_interaction_bridge),
        )
        .add_systems(PreUpdate, count_scene_clicks.after(GpuiSystems::Input))
        .run();
}

#[derive(Component)]
struct RotatingCube;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gpui: GpuiContexts,
) {
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(1.5, 1.5, 1.5))),
            MeshMaterial3d(materials.add(Color::srgb(0.16, 0.38, 0.92))),
            Transform::from_xyz(0.0, 0.75, 0.0),
            RotatingCube,
        ))
        .observe(record_picking_click);
    commands
        .spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(100.0, 100.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.08, 0.09, 0.12))),
        ))
        .observe(record_picking_click);
    commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));
    let camera = commands
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(-3.5, 3.5, 7.0).looking_at(Vec3::Y * 0.6, Vec3::Y),
        ))
        .id();
    let root = gpui
        .set_root(camera, |_window, cx| {
            cx.new(|_| OverlayView {
                gpui_clicks: 0,
                bevy_clicks: 0,
                scene_clicks: 0,
                picking_clicks: 0,
            })
        })
        .expect("failed to queue GPUI overlay root");
    commands.insert_resource(OverlayRoot(root));
}

fn rotate_cube(time: Res<Time>, mut cubes: Query<&mut Transform, With<RotatingCube>>) {
    for mut transform in &mut cubes {
        transform.rotate_y(time.delta_secs() * 0.7);
    }
}

struct OverlayView {
    gpui_clicks: u32,
    bevy_clicks: u32,
    scene_clicks: u32,
    picking_clicks: u32,
}

impl Render for OverlayView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().id("overlay-root").size_full().p_6().child(
            div()
                .w_96()
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .rounded_lg()
                .overflow_hidden()
                .shadow_lg()
                .backdrop_blur(bevy_gpui::gpui::px(12.0))
                .bg(rgba(0x18_18_1b_e8))
                .text_color(rgb(0xf4_f4_f5))
                .child(div().text_xl().child("Bevy + retained GPUI"))
                .child(
                    canvas(
                        |_, _, _| {},
                        |bounds, _, window, _| {
                            let mut path = PathBuilder::fill();
                            path.move_to(point(
                                bounds.origin.x + px(4.0),
                                bounds.origin.y + px(28.0),
                            ));
                            path.line_to(point(
                                bounds.origin.x + px(20.0),
                                bounds.origin.y + px(4.0),
                            ));
                            path.line_to(point(
                                bounds.origin.x + px(36.0),
                                bounds.origin.y + px(28.0),
                            ));
                            path.close();
                            window.paint_path(
                                path.build().expect("triangle path is valid"),
                                rgb(0x60_a5_fa),
                            );
                        },
                    )
                    .w_full()
                    .h_8(),
                )
                .child("Bevy owns this window, camera, event loop, and GPU.")
                .child(format!("GPUI callback count: {}", self.gpui_clicks))
                .child(format!("Bevy resource count: {}", self.bevy_clicks))
                .child(format!("Scene input count: {}", self.scene_clicks))
                .child(format!("Picking click count: {}", self.picking_clicks))
                .child(
                    div()
                        .id("increment")
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0x25_63_eb))
                        .hover(|style| style.bg(rgb(0x3b_82_f6)))
                        .cursor_pointer()
                        .child("Increment")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.gpui_clicks += 1;
                            cx.queue_bevy_command(|world: &mut World| {
                                world.resource_mut::<BevyClickCount>().0 += 1;
                            });
                            cx.notify();
                        })),
                ),
        )
    }
}

#[derive(Resource, Default)]
struct BevyClickCount(u32);

#[derive(Resource, Default)]
struct SceneClickCount(u32);

#[derive(Resource, Default)]
struct PickingClickCount(u32);

fn record_picking_click(_: On<Pointer<Click>>, mut clicks: ResMut<PickingClickCount>) {
    clicks.0 += 1;
}

fn count_scene_clicks(
    mut buttons: MessageReader<MouseButtonInput>,
    input: Res<GpuiInputState>,
    mut clicks: ResMut<SceneClickCount>,
) {
    for event in buttons.read() {
        if !input.wants_pointer_input
            && event.button == MouseButton::Left
            && event.state == ButtonState::Pressed
        {
            clicks.0 += 1;
        }
    }
}

#[derive(Resource)]
struct OverlayRoot(GpuiViewHandle<OverlayView>);

#[derive(Default, Resource)]
struct InteractionProbe(u8);

fn sync_clicks(
    clicks: Res<BevyClickCount>,
    scene_clicks: Res<SceneClickCount>,
    picking_clicks: Res<PickingClickCount>,
    root: Option<Res<OverlayRoot>>,
    mut gpui: GpuiContexts,
) {
    let Some(root) = root else {
        return;
    };
    let _ = gpui.update(&root.0, |view, _, cx| {
        if view.bevy_clicks != clicks.0
            || view.scene_clicks != scene_clicks.0
            || view.picking_clicks != picking_clicks.0
        {
            view.bevy_clicks = clicks.0;
            view.scene_clicks = scene_clicks.0;
            view.picking_clicks = picking_clicks.0;
            cx.notify();
        }
    });
}

fn verify_interaction_bridge(
    status: Res<GpuiRuntimeStatus>,
    mut counts: ParamSet<(
        Res<BevyClickCount>,
        Res<SceneClickCount>,
        Res<PickingClickCount>,
    )>,
    input: Res<GpuiInputState>,
    window: Single<Entity, With<PrimaryWindow>>,
    mut input_messages: ParamSet<(MessageWriter<CursorMoved>, MessageWriter<MouseButtonInput>)>,
    mut probe: ResMut<InteractionProbe>,
) {
    let clicks = counts.p0().0;
    let scene_clicks = counts.p1().0;
    let picking_clicks = counts.p2().0;
    if probe.0 == 0 && status.roots == 1 && status.scenes_built > 0 {
        let cursor = CursorMoved {
            window: *window,
            position: Vec2::new(200.0, 370.0),
            delta: None,
        };
        input_messages.p0().write(cursor);
        let pressed = MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window: *window,
        };
        input_messages.p1().write(pressed);
        let released = MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Released,
            window: *window,
        };
        input_messages.p1().write(released);
        probe.0 = 1;
    } else if probe.0 == 1 && clicks > 0 {
        assert_eq!(
            scene_clicks, 0,
            "the synthetic GPUI click reached raw Bevy scene input"
        );
        assert_eq!(
            picking_clicks, 0,
            "the synthetic GPUI click reached a Bevy picking target"
        );
        info!(
            clicks,
            scene_clicks,
            wants_pointer_input = input.wants_pointer_input,
            "bevy_gpui pointer dispatch and deferred ECS command bridge verified"
        );
        probe.0 = 2;
    }
}
