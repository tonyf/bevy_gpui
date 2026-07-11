use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use bevy_asset::{AssetId, Handle};
use bevy_ecs::resource::Resource;
use bevy_image::Image;
use gpui::{App, Canvas, ExternalSurfaceId, Global, Window, canvas};

static NEXT_EXTERNAL_IMAGE_ID: AtomicU64 = AtomicU64::new(1);

/// A Bevy image that can be painted directly inside a retained GPUI view.
///
/// Construct this from a strong Bevy [`Handle<Image>`], then pass it to
/// [`bevy_image()`]. The render bridge resolves the prepared `GpuImage` without
/// copying it to CPU memory or into GPUI's sprite atlas.
#[derive(Clone)]
pub struct GpuiBevyImage {
    registration: Arc<ImageRegistration>,
}

struct ImageRegistration {
    id: ExternalSurfaceId,
    handle: Handle<Image>,
    registries: Mutex<Vec<Weak<ImageRegistryInner>>>,
}

impl Drop for ImageRegistration {
    fn drop(&mut self) {
        for registry in self.registries.get_mut().unwrap().drain(..) {
            if let Some(registry) = registry.upgrade() {
                registry.images.write().unwrap().remove(&self.id);
            }
        }
    }
}

impl GpuiBevyImage {
    /// Creates a stable GPUI reference to a Bevy image asset.
    pub fn new(handle: Handle<Image>) -> Self {
        Self {
            registration: Arc::new(ImageRegistration {
                id: ExternalSurfaceId(NEXT_EXTERNAL_IMAGE_ID.fetch_add(1, Ordering::Relaxed)),
                handle,
                registries: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Returns the underlying Bevy image handle.
    pub fn handle(&self) -> &Handle<Image> {
        &self.registration.handle
    }
}

/// Creates a styleable GPUI canvas that paints a Bevy-owned image directly.
pub fn bevy_image(image: GpuiBevyImage) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, (), window: &mut Window, cx: &mut App| {
            let registry = cx.global::<GpuiImageRegistry>().clone();
            registry.register(&image);
            window.paint_external_surface(bounds, image.registration.id);
        },
    )
}

#[derive(Default)]
struct ImageRegistryInner {
    images: RwLock<HashMap<ExternalSurfaceId, AssetId<Image>>>,
    missing: AtomicBool,
}

#[derive(Clone, Default, Resource)]
pub(crate) struct GpuiImageRegistry(Arc<ImageRegistryInner>);

impl Global for GpuiImageRegistry {}

impl GpuiImageRegistry {
    fn register(&self, image: &GpuiBevyImage) {
        let registration = &image.registration;
        self.0
            .images
            .write()
            .unwrap()
            .insert(registration.id, registration.handle.id());
        let mut registries = registration.registries.lock().unwrap();
        if !registries
            .iter()
            .filter_map(Weak::upgrade)
            .any(|registry| Arc::ptr_eq(&registry, &self.0))
        {
            registries.push(Arc::downgrade(&self.0));
        }
    }

    pub(crate) fn asset_id(&self, id: ExternalSurfaceId) -> Option<AssetId<Image>> {
        self.0.images.read().unwrap().get(&id).copied()
    }

    pub(crate) fn mark_missing(&self) {
        self.0.missing.store(true, Ordering::Release);
    }

    pub(crate) fn take_missing(&self) -> bool {
        self.0.missing.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_does_not_keep_dropped_bevy_images_alive() {
        let registry = GpuiImageRegistry::default();
        let image = GpuiBevyImage::new(Handle::default());
        let id = image.registration.id;
        let retained_by_view = image.clone();
        registry.register(&image);
        assert!(registry.asset_id(id).is_some());

        drop(image);
        assert!(registry.asset_id(id).is_some());
        drop(retained_by_view);
        assert!(registry.asset_id(id).is_none());
    }
}
