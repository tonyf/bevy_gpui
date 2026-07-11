//! GPUI composited into Bevy's floating-point HDR camera target.

use bevy::{camera::Hdr, prelude::*};
use bevy_gpui::{
    GpuiContexts, GpuiPlugin,
    gpui::{Context, IntoElement, Render, Window, div, prelude::*, rgb, rgba},
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
        .add_systems(Update, rotate)
        .run();
}

#[derive(Component)]
struct HdrCube;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gpui: GpuiContexts,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.3, 1.0),
            emissive: LinearRgba::rgb(0.3, 0.8, 4.0),
            ..default()
        })),
        HdrCube,
    ));
    commands.spawn((
        PointLight {
            intensity: 3_000_000.0,
            ..default()
        },
        Transform::from_xyz(3.0, 4.0, 4.0),
    ));
    let camera = commands
        .spawn((
            Camera3d::default(),
            Hdr,
            Transform::from_xyz(0.0, 1.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    gpui.set_root(camera, |_, cx| cx.new(|_| HdrPanel))
        .expect("HDR GPUI root should be queued");
}

fn rotate(time: Res<Time>, mut cube: Single<&mut Transform, With<HdrCube>>) {
    cube.rotate_y(time.delta_secs() * 0.6);
    cube.rotate_x(time.delta_secs() * 0.2);
}

struct HdrPanel;

impl Render for HdrPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_6().child(
            div()
                .p_5()
                .rounded_lg()
                .bg(rgba(0x0f_17_2a_e8))
                .text_color(rgb(0xf8_fa_fc))
                .child(div().text_xl().child("GPUI over Bevy HDR"))
                .child("This overlay records directly into an Rgba16Float camera target."),
        )
    }
}
