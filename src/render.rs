use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use bevy_ecs::{
    prelude::{Entity, RemovedComponents, Res, ResMut, Resource},
    system::SystemParam,
};
use bevy_render::texture::{FallbackImage, GpuImage};
use bevy_render::{
    camera::ExtractedCamera,
    extract_resource::ExtractResource,
    render_asset::RenderAssets,
    renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue, ViewQuery},
    view::ViewTarget,
};
use gpui::{DevicePixels, point, size};
use gpui_wgpu::{
    ExternalAlphaMode, ExternalRenderTarget, SceneRenderer, WgpuAtlas,
    wgpu::{self, TextureFormat},
};

use crate::{GpuiScene, image::GpuiImageRegistry};

#[derive(Clone, Default, Resource, ExtractResource)]
pub(crate) struct GpuiGpuBridge(Arc<Mutex<GpuiGpuBridgeState>>);

#[derive(Default)]
struct GpuiGpuBridgeState {
    atlas: Option<Arc<WgpuAtlas>>,
    generation: u64,
}

impl GpuiGpuBridge {
    pub(crate) fn snapshot(&self) -> Option<(u64, Arc<WgpuAtlas>)> {
        let state = self.0.lock().unwrap();
        Some((state.generation, state.atlas.clone()?))
    }

    fn publish(&self, atlas: Arc<WgpuAtlas>) -> u64 {
        let mut state = self.0.lock().unwrap();
        state.generation = state.generation.wrapping_add(1).max(1);
        state.atlas = Some(atlas);
        state.generation
    }
}

#[derive(Resource, Default)]
pub(crate) struct GpuiRenderState {
    atlas: Option<Arc<WgpuAtlas>>,
    renderers: HashMap<RendererKey, SceneRenderer>,
    recorded_views: HashSet<Entity>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct RendererKey {
    view: Entity,
    format: TextureFormat,
}

impl GpuiRenderState {
    fn atlas(
        &mut self,
        adapter: &wgpu::Adapter,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Arc<WgpuAtlas> {
        self.atlas
            .get_or_insert_with(|| {
                let required = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
                let format = [TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm]
                    .into_iter()
                    .find(|format| {
                        adapter
                            .get_texture_format_features(*format)
                            .allowed_usages
                            .contains(required)
                    })
                    .expect("Bevy's adapter must support a GPUI color atlas format");
                Arc::new(WgpuAtlas::new(
                    Arc::new(device.wgpu_device().clone()),
                    Arc::new((****queue).clone()),
                    format,
                ))
            })
            .clone()
    }
}

pub(crate) fn initialize_gpui_gpu(
    mut state: ResMut<GpuiRenderState>,
    bridge: Res<GpuiGpuBridge>,
    adapter: Res<RenderAdapter>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    // `RenderStartup` runs for the initial device and every replacement device.
    // Discard every resource created by the previous device before publishing
    // the new atlas to the embedded main-world runtime.
    *state = GpuiRenderState::default();
    let atlas = state.atlas(&adapter, &device, &queue);
    let generation = bridge.publish(atlas);
    bevy_log::info!(
        generation,
        "GPUI GPU resources initialized for Bevy render device"
    );
}

pub(crate) fn cleanup_gpui_renderers(
    mut state: ResMut<GpuiRenderState>,
    mut removed_scenes: RemovedComponents<GpuiScene>,
) {
    for view in removed_scenes.read() {
        state.renderers.retain(|key, _| key.view != view);
        state.recorded_views.remove(&view);
    }
}

pub(crate) fn gpui_pass(
    view: ViewQuery<(Entity, &ViewTarget, &ExtractedCamera, &GpuiScene)>,
    resources: GpuiRenderResources,
    mut context: RenderContext,
) {
    let GpuiRenderResources {
        mut state,
        adapter,
        device,
        queue,
        images,
        fallback_image,
        image_registry,
    } = resources;
    let (view_entity, target, camera, scene) = view.into_inner();
    let Some(physical_size) = camera.physical_viewport_size else {
        return;
    };
    if physical_size.x == 0 || physical_size.y == 0 {
        return;
    }
    let viewport_origin = camera
        .viewport
        .as_ref()
        .map_or(bevy_math::UVec2::ZERO, |viewport| {
            viewport.physical_position
        });
    let size = size(
        DevicePixels(physical_size.x as i32),
        DevicePixels(physical_size.y as i32),
    );
    let format = target.main_texture_format();
    let renderer_key = RendererKey {
        view: view_entity,
        format,
    };

    if !state.renderers.contains_key(&renderer_key) {
        let atlas = state.atlas(&adapter, &device, &queue);
        match SceneRenderer::from_external_device(
            &adapter,
            Arc::new(device.wgpu_device().clone()),
            Arc::new((****queue).clone()),
            atlas,
            format,
            size,
            ExternalAlphaMode::Premultiplied,
        ) {
            Ok(renderer) => {
                state.renderers.insert(renderer_key, renderer);
            }
            Err(error) => {
                bevy_log::error!(?error, "failed to create GPUI scene renderer");
                return;
            }
        }
    }

    let attachment = target.get_unsampled_color_attachment();
    let uses_filters =
        !scene.snapshot.backdrop_filters.is_empty() || !scene.snapshot.filter_boundaries.is_empty();
    let post_process = uses_filters.then(|| target.post_process_write());
    let (color, background) = post_process
        .as_ref()
        .map_or((attachment.view, None), |post_process| {
            (post_process.destination, Some(&**post_process.source))
        });
    let physical_target_size = camera.physical_target_size.unwrap_or(physical_size);
    let target_size = gpui::Size {
        width: DevicePixels(physical_target_size.x as i32),
        height: DevicePixels(physical_target_size.y as i32),
    };
    let mut external_textures = HashMap::new();
    for surface in &scene.snapshot.external_surfaces {
        let texture_view = image_registry
            .asset_id(surface.id)
            .and_then(|id| images.get(id))
            .map_or_else(
                || {
                    image_registry.mark_missing();
                    &*fallback_image.d2.texture_view
                },
                |image| &*image.texture_view,
            );
        external_textures.insert(surface.id, texture_view);
    }
    let renderer = state
        .renderers
        .get_mut(&renderer_key)
        .expect("renderer was inserted for the target view and format");
    let render_result = renderer.render(
        &scene.snapshot,
        ExternalRenderTarget {
            encoder: context.command_encoder(),
            color,
            format,
            origin: point(
                DevicePixels(viewport_origin.x as i32),
                DevicePixels(viewport_origin.y as i32),
            ),
            size,
            load: wgpu::LoadOp::Load,
            alpha_mode: ExternalAlphaMode::Premultiplied,
            external_textures: &external_textures,
            background,
            target_size,
        },
    );
    match render_result {
        Ok(()) => {
            if state.recorded_views.insert(view_entity) {
                bevy_log::info!(
                    ?view_entity,
                    ?format,
                    ?viewport_origin,
                    ?physical_size,
                    shadows = scene.snapshot.shadows.len(),
                    quads = scene.snapshot.quads.len(),
                    paths = scene.snapshot.paths.len(),
                    monochrome_sprites = scene.snapshot.monochrome_sprites.len(),
                    polychrome_sprites = scene.snapshot.polychrome_sprites.len(),
                    external_surfaces = scene.snapshot.external_surfaces.len(),
                    backdrop_filters = scene.snapshot.backdrop_filters.len(),
                    content_filter_groups = scene.snapshot.filter_boundaries.len() / 2,
                    "recorded first GPUI scene into Bevy ViewTarget"
                );
            }
        }
        Err(error) => bevy_log::error!(?error, "failed to record GPUI scene"),
    }
}

#[derive(SystemParam)]
pub(crate) struct GpuiRenderResources<'w> {
    state: ResMut<'w, GpuiRenderState>,
    adapter: Res<'w, RenderAdapter>,
    device: Res<'w, RenderDevice>,
    queue: Res<'w, RenderQueue>,
    images: Res<'w, RenderAssets<GpuImage>>,
    fallback_image: Res<'w, FallbackImage>,
    image_registry: Res<'w, GpuiImageRegistry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_bridge_starts_without_a_published_device() {
        assert!(GpuiGpuBridge::default().snapshot().is_none());
    }

    #[test]
    fn renderer_cache_key_isolated_by_view_and_format() {
        let first_view = Entity::from_raw_u32(1).unwrap();
        let second_view = Entity::from_raw_u32(2).unwrap();
        let format = TextureFormat::Rgba8UnormSrgb;

        assert_ne!(
            RendererKey {
                view: first_view,
                format,
            },
            RendererKey {
                view: second_view,
                format,
            }
        );
    }
}
