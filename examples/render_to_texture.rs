//! A retained GPUI root rendered into a Bevy `Image` target and sampled by 3D.

use bevy::{
    asset::RenderAssetUsages,
    camera::RenderTarget,
    mesh::VertexAttributeValues,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy_gpui::{
    GpuiBevyImage, GpuiContexts, GpuiPlugin, bevy_image,
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
        .add_systems(Update, rotate_display)
        .run();
}

#[derive(Component)]
struct DisplayCube;

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gpui: GpuiContexts,
) {
    let target = images.add(Image::new_target_texture(
        512,
        512,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    let source = images.add(Image::new_fill(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[
            0xff, 0x40, 0x40, 0xff, 0x40, 0xff, 0x80, 0xff, 0x40, 0x80, 0xff, 0xff, 0xff, 0xd0,
            0x40, 0xff,
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let source = GpuiBevyImage::new(source);

    let texture_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                clear_color: Color::srgb(0.02, 0.025, 0.04).into(),
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
        ))
        .id();

    gpui.set_root(texture_camera, move |_, cx| {
        let source = source.clone();
        cx.new(move |_| TexturePanel { source })
    })
    .expect("texture GPUI root should be queued");

    commands.spawn((
        Mesh3d(meshes.add(display_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(target),
            unlit: true,
            ..default()
        })),
        DisplayCube,
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn display_mesh() -> Mesh {
    let mut mesh = Mesh::from(Cuboid::new(4.0, 4.0, 0.3));
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
    else {
        unreachable!("Bevy's cuboid mesh always has two-dimensional UV coordinates")
    };

    // Bevy images use a top-left UV origin. The stock cuboid maps V=0 to the
    // bottom of its front face, which is useful for render targets produced by
    // a Bevy 3D camera but inverts a top-left-oriented GPUI surface.
    for uv in uvs {
        uv[1] = 1.0 - uv[1];
    }
    mesh
}

fn rotate_display(time: Res<Time>, mut cube: Single<&mut Transform, With<DisplayCube>>) {
    cube.rotation = Quat::from_rotation_y(time.elapsed_secs().sin() * 0.25);
}

struct TexturePanel {
    source: GpuiBevyImage,
}

impl Render for TexturePanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_8()
            .bg(rgb(0x0f_17_2a))
            .text_color(rgb(0xf8_fa_fc))
            .child(
                div()
                    .size_full()
                    .p_6()
                    .rounded_xl()
                    .bg(rgba(0x25_63_eb_e8))
                    .child(div().text_2xl().child("GPUI → Bevy Image"))
                    .child("This retained tree is rendered by a camera targeting a texture.")
                    .child(bevy_image(self.source.clone()).w_32().h_32().rounded_lg()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_mesh_maps_texture_top_to_world_top() {
        let mesh = display_mesh();
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("display mesh should contain two-dimensional UV coordinates")
        };

        // The first four vertices are the front face: bottom-left,
        // bottom-right, top-right, top-left.
        assert_eq!(&uvs[..4], &[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    }
}
