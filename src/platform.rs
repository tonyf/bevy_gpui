use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use gpui::{
    Action, AnyWindowHandle, BackgroundExecutor, Bounds, Capslock, ClipboardItem, CursorStyle,
    DispatchEventResult, DisplayId, DummyKeyboardMapper, ForegroundExecutor, GpuSpecs, Keymap,
    Menu, MenuItem, Modifiers, OwnedMenu, PathPromptOptions, Pixels, Platform, PlatformAtlas,
    PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Point, PromptButton,
    RequestFrameOptions, Scene, SceneSnapshot, Size, Task, ThermalState, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowParams,
};
#[cfg(feature = "render")]
use gpui_wgpu::CosmicTextSystem;
use gpui_wgpu::WgpuAtlas;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use uuid::Uuid;

use crate::dispatcher::BevyDispatcher;

type ResizeCallback = Box<dyn FnMut(Size<Pixels>, f32)>;

#[derive(Default)]
pub(crate) struct BevyWindowOutput {
    pub(crate) title: Option<String>,
    pub(crate) logical_size: Option<Size<Pixels>>,
    pub(crate) fullscreen: Option<bool>,
    pub(crate) minimize: bool,
    pub(crate) maximize: bool,
    pub(crate) activate: bool,
    pub(crate) ime_position: Option<Point<Pixels>>,
    pub(crate) cursor_visible: Option<bool>,
    pub(crate) background_appearance: Option<WindowBackgroundAppearance>,
}

pub(crate) struct BevyPlatform {
    dispatcher: Arc<BevyDispatcher>,
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
    atlas: Arc<WgpuAtlas>,
    display: Rc<BevyDisplay>,
    displays: RefCell<HashMap<u64, Rc<BevyDisplay>>>,
    primary_display_id: Cell<u64>,
    windows: RefCell<HashMap<u64, BevyPlatformWindow>>,
    pending_context: Cell<Option<u64>>,
    active_context: Cell<Option<u64>>,
    clipboard: RefCell<Option<ClipboardItem>>,
    system_clipboard: RefCell<Option<arboard::Clipboard>>,
    cursor: Cell<CursorStyle>,
    appearance: Cell<WindowAppearance>,
    quit_requested: Cell<bool>,
    quit_callbacks: RefCell<Vec<Box<dyn FnMut()>>>,
    reopen_callbacks: RefCell<Vec<Box<dyn FnMut()>>>,
}

impl BevyPlatform {
    #[cfg(feature = "render")]
    pub(crate) fn new(
        atlas: Arc<WgpuAtlas>,
        wake_event_loop: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Rc<Self> {
        let dispatcher = BevyDispatcher::new(wake_event_loop);
        let background_executor = BackgroundExecutor::new(dispatcher.clone());
        let foreground_executor = ForegroundExecutor::new(dispatcher.clone());
        Rc::new(Self {
            dispatcher,
            background_executor,
            foreground_executor,
            text_system: Arc::new(CosmicTextSystem::new("Arial")),
            atlas,
            display: Rc::new(BevyDisplay::default()),
            displays: RefCell::new(HashMap::new()),
            primary_display_id: Cell::new(1),
            windows: RefCell::new(HashMap::new()),
            pending_context: Cell::new(None),
            active_context: Cell::new(None),
            clipboard: RefCell::new(None),
            system_clipboard: RefCell::new(arboard::Clipboard::new().ok()),
            cursor: Cell::new(CursorStyle::Arrow),
            appearance: Cell::new(WindowAppearance::Dark),
            quit_requested: Cell::new(false),
            quit_callbacks: RefCell::new(Vec::new()),
            reopen_callbacks: RefCell::new(Vec::new()),
        })
    }

    pub(crate) fn dispatcher(&self) -> &Arc<BevyDispatcher> {
        &self.dispatcher
    }

    #[cfg_attr(not(feature = "render"), allow(dead_code))]
    pub(crate) fn prepare_open_context(&self, id: u64) -> Result<()> {
        if self.pending_context.replace(Some(id)).is_some() {
            return Err(anyhow!("another embedded GPUI window is already opening"));
        }
        Ok(())
    }

    #[cfg_attr(not(feature = "render"), allow(dead_code))]
    pub(crate) fn cancel_open_context(&self, id: u64) {
        if self.pending_context.get() == Some(id) {
            self.pending_context.set(None);
        }
    }

    pub(crate) fn remove_context(&self, id: u64) {
        self.windows.borrow_mut().remove(&id);
    }

    pub(crate) fn take_window_output(&self, id: u64) -> BevyWindowOutput {
        self.windows
            .borrow()
            .get(&id)
            .map_or_else(BevyWindowOutput::default, BevyPlatformWindow::take_output)
    }

    pub(crate) fn cursor_style(&self, id: u64) -> CursorStyle {
        self.windows
            .borrow()
            .get(&id)
            .map_or_else(|| self.cursor.get(), BevyPlatformWindow::cursor_style)
    }

    pub(crate) fn request_frame(&self, id: u64) {
        if let Some(window) = self.windows.borrow().get(&id).cloned() {
            let previous = self.active_context.replace(Some(id));
            window.request_frame();
            self.active_context.set(previous);
        }
    }

    pub(crate) fn take_latest_snapshot(&self, id: u64) -> Option<(u64, SceneSnapshot)> {
        self.windows
            .borrow()
            .get(&id)
            .and_then(BevyPlatformWindow::take_latest_snapshot)
    }

    pub(crate) fn quit_requested(&self) -> bool {
        self.quit_requested.replace(false)
    }

    pub(crate) fn dispatch_input(&self, id: u64, input: PlatformInput) -> DispatchEventResult {
        let window = self.windows.borrow().get(&id).cloned();
        let Some(window) = window else {
            return DispatchEventResult::default();
        };
        let previous = self.active_context.replace(Some(id));
        let result = window.dispatch_input(input);
        self.active_context.set(previous);
        result
    }

    pub(crate) fn mouse_position(&self, id: u64) -> Point<Pixels> {
        self.windows
            .borrow()
            .get(&id)
            .map_or_else(Point::default, BevyPlatformWindow::mouse_position)
    }

    pub(crate) fn pressed_button(&self, id: u64) -> Option<gpui::MouseButton> {
        self.windows
            .borrow()
            .get(&id)
            .and_then(BevyPlatformWindow::pressed_button)
    }

    pub(crate) fn set_keyboard_state(&self, id: u64, modifiers: Modifiers, capslock: Capslock) {
        if let Some(window) = self.windows.borrow().get(&id) {
            window.set_keyboard_state(modifiers, capslock);
        }
    }

    pub(crate) fn accepts_text_input(&self, id: u64) -> bool {
        self.windows
            .borrow()
            .get(&id)
            .is_some_and(BevyPlatformWindow::accepts_text_input)
    }

    pub(crate) fn prefers_ime_for_printable_keys(&self, id: u64) -> bool {
        self.windows
            .borrow()
            .get(&id)
            .is_some_and(BevyPlatformWindow::prefers_ime_for_printable_keys)
    }

    pub(crate) fn ime_preedit(
        &self,
        id: u64,
        value: &str,
        selected_range: Option<Range<usize>>,
    ) -> bool {
        self.windows
            .borrow()
            .get(&id)
            .is_some_and(|window| window.ime_preedit(value, selected_range))
    }

    pub(crate) fn ime_commit(&self, id: u64, value: &str) -> bool {
        self.windows
            .borrow()
            .get(&id)
            .is_some_and(|window| window.ime_commit(value))
    }

    pub(crate) fn ime_cancel(&self, id: u64) {
        if let Some(window) = self.windows.borrow().get(&id) {
            window.ime_cancel();
        }
    }

    pub(crate) fn set_hovered(&self, id: u64, hovered: bool) {
        if let Some(window) = self.windows.borrow().get(&id) {
            window.set_hovered(hovered);
        }
    }

    pub(crate) fn sync_window(&self, id: u64, viewport: bevy_math::Rect, scale: f32, active: bool) {
        if let Some(window) = self.windows.borrow().get(&id).cloned() {
            window.sync_host_state(
                Bounds::new(
                    Point::default(),
                    Size::new(gpui::px(viewport.width()), gpui::px(viewport.height())),
                ),
                scale,
                active,
            );
        }
    }

    pub(crate) fn notify_window_moved(&self, id: u64) {
        if let Some(window) = self.windows.borrow().get(&id).cloned() {
            window.notify_host_moved();
        }
    }

    pub(crate) fn sync_display(
        &self,
        id: u64,
        physical_position: bevy_math::IVec2,
        physical_size: bevy_math::UVec2,
        scale_factor: f64,
        primary: bool,
    ) {
        let display = self
            .displays
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| Rc::new(BevyDisplay::new(id)))
            .clone();
        display.sync(physical_position, physical_size, scale_factor);
        if primary {
            self.primary_display_id.set(id);
            self.display
                .sync(physical_position, physical_size, scale_factor);
        }
    }

    pub(crate) fn retain_displays(&self, live: &[u64]) {
        self.displays.borrow_mut().retain(|id, _| live.contains(id));
    }

    pub(crate) fn set_window_display(&self, id: u64, display_id: u64) {
        let display = self.displays.borrow().get(&display_id).cloned();
        if let (Some(window), Some(display)) = (self.windows.borrow().get(&id).cloned(), display) {
            window.set_host_display(display);
        }
    }

    pub(crate) fn set_window_raw_handles(
        &self,
        id: u64,
        window: RawWindowHandle,
        display: RawDisplayHandle,
    ) {
        if let Some(platform_window) = self.windows.borrow().get(&id) {
            let mut state = platform_window.0.borrow_mut();
            state.raw_window_handle = Some(window);
            state.raw_display_handle = Some(display);
        }
    }

    pub(crate) fn set_window_appearance(&self, id: u64, appearance: WindowAppearance) {
        self.appearance.set(appearance);
        if let Some(window) = self.windows.borrow().get(&id).cloned() {
            window.set_host_appearance(appearance);
        }
    }

    pub(crate) fn set_context_active(&self, id: u64, active: bool) {
        if let Some(window) = self.windows.borrow().get(&id).cloned() {
            let (bounds, scale) = {
                let state = window.0.borrow();
                (state.bounds, state.scale_factor)
            };
            window.sync_host_state(bounds, scale, active);
        }
    }

    pub(crate) fn should_close(&self, id: u64) -> bool {
        self.windows
            .borrow()
            .get(&id)
            .is_none_or(BevyPlatformWindow::should_close)
    }
}

impl Platform for BevyPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }

    fn run(&self, _: Box<dyn FnOnce()>) {
        panic!("embedded Bevy GPUI must not enter Platform::run")
    }

    fn quit(&self) {
        for callback in self.quit_callbacks.borrow_mut().iter_mut() {
            callback();
        }
        self.quit_requested.set(true);
    }

    fn restart(&self, _: Option<PathBuf>) {
        bevy_log::warn!("application restart is not supported by embedded bevy_gpui");
    }
    fn activate(&self, _: bool) {
        bevy_log::warn!("application activation is owned by Bevy's window backend");
    }
    fn hide(&self) {
        bevy_log::warn!("application hiding is not supported by embedded bevy_gpui");
    }
    fn hide_other_apps(&self) {
        bevy_log::warn!("hiding other applications is not supported by embedded bevy_gpui");
    }
    fn unhide_other_apps(&self) {
        bevy_log::warn!("unhiding other applications is not supported by embedded bevy_gpui");
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        let mut displays: Vec<_> = self.displays.borrow().values().cloned().collect();
        displays.sort_by_key(|display| u64::from(display.id()));
        if displays.is_empty() {
            vec![self.display.clone()]
        } else {
            displays
                .into_iter()
                .map(|display| display as Rc<dyn PlatformDisplay>)
                .collect()
        }
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(
            self.displays
                .borrow()
                .get(&self.primary_display_id.get())
                .cloned()
                .unwrap_or_else(|| self.display.clone()),
        )
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        let windows = self.windows.borrow();
        windows
            .values()
            .find(|window| window.0.borrow().is_active)
            .or_else(|| windows.values().next())
            .map(BevyPlatformWindow::handle)
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let id = self
            .pending_context
            .take()
            .ok_or_else(|| anyhow!("GPUI opened a window without a Bevy context binding"))?;
        let window = BevyPlatformWindow::new(
            handle,
            options.bounds,
            self.display.clone(),
            self.atlas.clone(),
        );
        self.windows.borrow_mut().insert(id, window.clone());
        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.appearance.get()
    }

    fn open_url(&self, url: &str) {
        if let Err(error) = open::that_detached(url) {
            bevy_log::warn!(?error, %url, "failed to open URL requested by GPUI");
        }
    }
    fn on_open_urls(&self, _: Box<dyn FnMut(Vec<String>)>) {
        bevy_log::warn!("incoming URL callbacks are not implemented by bevy_gpui");
    }

    fn register_url_scheme(&self, _: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "URL scheme registration is not implemented by bevy_gpui"
        )))
    }

    fn prompt_for_paths(
        &self,
        _: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Err(anyhow!(
            "native file prompts are not implemented by bevy_gpui"
        )));
        rx
    }

    fn prompt_for_new_path(
        &self,
        _: &Path,
        _: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Err(anyhow!(
            "native save prompts are not implemented by bevy_gpui"
        )));
        rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }
    fn reveal_path(&self, path: &Path) {
        let target = path.parent().unwrap_or(path);
        if let Err(error) = open::that_detached(target) {
            bevy_log::warn!(?error, ?path, "failed to reveal path requested by GPUI");
        }
    }
    fn open_with_system(&self, path: &Path) {
        if let Err(error) = open::that_detached(path) {
            bevy_log::warn!(?error, ?path, "failed to open path requested by GPUI");
        }
    }
    fn on_quit(&self, callback: Box<dyn FnMut()>) {
        self.quit_callbacks.borrow_mut().push(callback);
    }
    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.reopen_callbacks.borrow_mut().push(callback);
    }
    fn set_menus(&self, _: Vec<Menu>, _: &Keymap) {
        bevy_log::warn!("native application menus are not implemented by bevy_gpui");
    }
    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        None
    }
    fn set_dock_menu(&self, _: Vec<MenuItem>, _: &Keymap) {
        bevy_log::warn!("native dock menus are not implemented by bevy_gpui");
    }
    fn on_app_menu_action(&self, _: Box<dyn FnMut(&dyn Action)>) {
        bevy_log::warn!("native application menu actions are not implemented by bevy_gpui");
    }
    fn on_will_open_app_menu(&self, _: Box<dyn FnMut()>) {
        bevy_log::warn!("native application menus are not implemented by bevy_gpui");
    }
    fn on_validate_app_menu_command(&self, _: Box<dyn FnMut(&dyn Action) -> bool>) {
        bevy_log::warn!("native application menu validation is not implemented by bevy_gpui");
    }

    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }
    fn on_thermal_state_change(&self, _: Box<dyn FnMut()>) {}

    fn app_path(&self) -> Result<PathBuf> {
        std::env::current_exe().map_err(Into::into)
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        let mut path = self.app_path()?;
        path.set_file_name(name);
        Ok(path)
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        self.cursor.set(style);
        if let Some(id) = self.active_context.get()
            && let Some(window) = self.windows.borrow().get(&id)
        {
            window.set_cursor_style(style);
        }
    }
    fn hide_cursor_until_mouse_moves(&self) {
        if let Some(id) = self.active_context.get()
            && let Some(window) = self.windows.borrow().get(&id)
        {
            window.set_cursor_visible(false);
        }
    }
    fn is_cursor_visible(&self) -> bool {
        self.active_context
            .get()
            .and_then(|id| {
                self.windows
                    .borrow()
                    .get(&id)
                    .map(BevyPlatformWindow::cursor_visible)
            })
            .unwrap_or(true)
    }
    fn should_auto_hide_scrollbars(&self) -> bool {
        false
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        if let Some(clipboard) = self.system_clipboard.borrow_mut().as_mut()
            && let Ok(text) = clipboard.get_text()
        {
            let item = ClipboardItem::new_string(text);
            self.clipboard.replace(Some(item.clone()));
            return Some(item);
        }
        self.clipboard.borrow().clone()
    }
    fn write_to_clipboard(&self, item: ClipboardItem) {
        if let Some(text) = item.text()
            && let Some(clipboard) = self.system_clipboard.borrow_mut().as_mut()
            && let Err(error) = clipboard.set_text(text)
        {
            bevy_log::warn!(?error, "failed to write GPUI text to the system clipboard");
        }
        self.clipboard.replace(Some(item));
    }

    #[cfg(target_os = "macos")]
    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
        self.read_from_clipboard()
    }
    #[cfg(target_os = "macos")]
    fn write_to_find_pasteboard(&self, item: ClipboardItem) {
        self.write_to_clipboard(item);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.read_from_clipboard()
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn write_to_primary(&self, item: ClipboardItem) {
        self.write_to_clipboard(item);
    }

    fn write_credentials(&self, _: &str, _: &str, _: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("credential storage is not implemented")))
    }
    fn read_credentials(&self, _: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Err(anyhow!("credential storage is not implemented")))
    }
    fn delete_credentials(&self, _: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("credential storage is not implemented")))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(BevyKeyboardLayout)
    }
    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }
    fn on_keyboard_layout_change(&self, _: Box<dyn FnMut()>) {}
}

#[derive(Debug)]
struct BevyDisplay {
    id: DisplayId,
    bounds: RefCell<Bounds<Pixels>>,
}

impl Default for BevyDisplay {
    fn default() -> Self {
        Self {
            id: DisplayId::new(1),
            bounds: RefCell::new(Bounds::new(
                Point::default(),
                Size::new(gpui::px(1920.0), gpui::px(1080.0)),
            )),
        }
    }
}

impl BevyDisplay {
    fn new(id: u64) -> Self {
        Self {
            id: DisplayId::new(id),
            ..Self::default()
        }
    }

    fn sync(
        &self,
        physical_position: bevy_math::IVec2,
        physical_size: bevy_math::UVec2,
        scale_factor: f64,
    ) {
        let scale = scale_factor.max(f64::EPSILON) as f32;
        *self.bounds.borrow_mut() = Bounds::new(
            Point::new(
                gpui::px(physical_position.x as f32 / scale),
                gpui::px(physical_position.y as f32 / scale),
            ),
            Size::new(
                gpui::px(physical_size.x as f32 / scale),
                gpui::px(physical_size.y as f32 / scale),
            ),
        );
    }
}

impl PlatformDisplay for BevyDisplay {
    fn id(&self) -> DisplayId {
        self.id
    }

    fn uuid(&self) -> Result<Uuid> {
        Ok(Uuid::from_u128(u128::from(u64::from(self.id))))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        *self.bounds.borrow()
    }
}

struct BevyKeyboardLayout;

impl PlatformKeyboardLayout for BevyKeyboardLayout {
    fn id(&self) -> &str {
        "bevy.keyboard"
    }

    fn name(&self) -> &str {
        "Bevy keyboard"
    }
}

struct BevyPlatformWindowState {
    handle: AnyWindowHandle,
    bounds: Bounds<Pixels>,
    display: Rc<dyn PlatformDisplay>,
    atlas: Arc<WgpuAtlas>,
    latest_snapshot: Option<SceneSnapshot>,
    scene_generation: u64,
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active: Option<Box<dyn FnMut(bool)>>,
    hovered: Option<Box<dyn FnMut(bool)>>,
    resized: Option<ResizeCallback>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    hit_test: Option<Box<dyn FnMut() -> Option<WindowControlArea>>>,
    appearance_changed: Option<Box<dyn FnMut()>>,
    input_handler: Option<PlatformInputHandler>,
    title: String,
    fullscreen: bool,
    scale_factor: f32,
    is_active: bool,
    is_hovered: bool,
    mouse_position: Point<Pixels>,
    pressed_button: Option<gpui::MouseButton>,
    modifiers: Modifiers,
    capslock: Capslock,
    output: BevyWindowOutput,
    cursor_style: CursorStyle,
    appearance: WindowAppearance,
    cursor_visible: bool,
    background_appearance: WindowBackgroundAppearance,
    raw_window_handle: Option<RawWindowHandle>,
    raw_display_handle: Option<RawDisplayHandle>,
}

#[derive(Clone)]
pub(crate) struct BevyPlatformWindow(Rc<RefCell<BevyPlatformWindowState>>);

impl BevyPlatformWindow {
    fn new(
        handle: AnyWindowHandle,
        bounds: Bounds<Pixels>,
        display: Rc<dyn PlatformDisplay>,
        atlas: Arc<WgpuAtlas>,
    ) -> Self {
        Self(Rc::new(RefCell::new(BevyPlatformWindowState {
            handle,
            bounds,
            display,
            atlas,
            latest_snapshot: None,
            scene_generation: 0,
            request_frame: None,
            input: None,
            active: None,
            hovered: None,
            resized: None,
            moved: None,
            should_close: None,
            close: None,
            hit_test: None,
            appearance_changed: None,
            input_handler: None,
            title: String::new(),
            fullscreen: false,
            scale_factor: 1.0,
            is_active: true,
            is_hovered: false,
            mouse_position: Point::default(),
            pressed_button: None,
            modifiers: Modifiers::default(),
            capslock: Capslock::default(),
            output: BevyWindowOutput::default(),
            cursor_style: CursorStyle::Arrow,
            appearance: WindowAppearance::Dark,
            cursor_visible: true,
            background_appearance: WindowBackgroundAppearance::Transparent,
            raw_window_handle: None,
            raw_display_handle: None,
        })))
    }

    fn handle(&self) -> AnyWindowHandle {
        self.0.borrow().handle
    }

    fn request_frame(&self) {
        let callback = self.0.borrow_mut().request_frame.take();
        if let Some(mut callback) = callback {
            callback(RequestFrameOptions {
                require_presentation: false,
                force_render: false,
            });
            self.0.borrow_mut().request_frame = Some(callback);
        }
    }

    fn take_latest_snapshot(&self) -> Option<(u64, SceneSnapshot)> {
        let mut state = self.0.borrow_mut();
        let generation = state.scene_generation;
        state
            .latest_snapshot
            .take()
            .map(|snapshot| (generation, snapshot))
    }

    fn dispatch_input(&self, input: PlatformInput) -> DispatchEventResult {
        {
            let mut state = self.0.borrow_mut();
            match &input {
                PlatformInput::MouseMove(event) => state.mouse_position = event.position,
                PlatformInput::MouseDown(event) => {
                    state.mouse_position = event.position;
                    state.pressed_button = Some(event.button);
                }
                PlatformInput::MouseUp(event) => {
                    state.mouse_position = event.position;
                    state.pressed_button = None;
                }
                _ => {}
            }
            if matches!(&input, PlatformInput::MouseMove(_)) && !state.cursor_visible {
                state.cursor_visible = true;
                state.output.cursor_visible = Some(true);
            }
        }
        let callback = self.0.borrow_mut().input.take();
        let Some(mut callback) = callback else {
            return DispatchEventResult::default();
        };
        let result = callback(input);
        self.0.borrow_mut().input = Some(callback);
        result
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.0.borrow().mouse_position
    }

    fn pressed_button(&self) -> Option<gpui::MouseButton> {
        self.0.borrow().pressed_button
    }

    fn take_output(&self) -> BevyWindowOutput {
        std::mem::take(&mut self.0.borrow_mut().output)
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        self.0.borrow_mut().cursor_style = style;
    }

    fn cursor_style(&self) -> CursorStyle {
        self.0.borrow().cursor_style
    }

    fn set_cursor_visible(&self, visible: bool) {
        let mut state = self.0.borrow_mut();
        state.cursor_visible = visible;
        state.output.cursor_visible = Some(visible);
    }

    fn cursor_visible(&self) -> bool {
        self.0.borrow().cursor_visible
    }

    fn set_keyboard_state(&self, modifiers: Modifiers, capslock: Capslock) {
        let mut state = self.0.borrow_mut();
        state.modifiers = modifiers;
        state.capslock = capslock;
    }

    fn with_input_handler<R>(
        &self,
        update: impl FnOnce(&mut PlatformInputHandler) -> R,
    ) -> Option<R> {
        let mut handler = self.0.borrow_mut().input_handler.take()?;
        let result = update(&mut handler);
        self.0.borrow_mut().input_handler = Some(handler);
        Some(result)
    }

    fn accepts_text_input(&self) -> bool {
        self.with_input_handler(PlatformInputHandler::query_accepts_text_input)
            .unwrap_or(false)
    }

    fn prefers_ime_for_printable_keys(&self) -> bool {
        self.with_input_handler(PlatformInputHandler::query_prefers_ime_for_printable_keys)
            .unwrap_or(false)
    }

    fn ime_preedit(&self, value: &str, selected_range: Option<Range<usize>>) -> bool {
        self.with_input_handler(|handler| {
            if value.is_empty() {
                handler.unmark_text();
            } else {
                handler.replace_and_mark_text_in_range(None, value, selected_range);
            }
        })
        .is_some()
    }

    fn ime_commit(&self, value: &str) -> bool {
        self.with_input_handler(|handler| {
            handler.replace_text_in_range(None, value);
            handler.unmark_text();
        })
        .is_some()
    }

    fn ime_cancel(&self) {
        let _ = self.with_input_handler(PlatformInputHandler::unmark_text);
    }

    fn sync_host_state(&self, bounds: Bounds<Pixels>, scale_factor: f32, is_active: bool) {
        let (mut resized, mut moved, mut active_changed) = {
            let mut state = self.0.borrow_mut();
            let resized = if state.bounds.size != bounds.size
                || (state.scale_factor - scale_factor).abs() > f32::EPSILON
            {
                state.resized.take()
            } else {
                None
            };
            let moved = if state.bounds.origin != bounds.origin {
                state.moved.take()
            } else {
                None
            };
            let active_changed = if state.is_active != is_active {
                state.active.take()
            } else {
                None
            };
            state.bounds = bounds;
            state.scale_factor = scale_factor;
            state.is_active = is_active;
            (resized, moved, active_changed)
        };

        if let Some(callback) = resized.as_mut() {
            callback(bounds.size, scale_factor);
        }
        if let Some(callback) = active_changed.as_mut() {
            callback(is_active);
        }
        if let Some(callback) = moved.as_mut() {
            callback();
        }

        let mut state = self.0.borrow_mut();
        if resized.is_some() {
            state.resized = resized;
        }
        if active_changed.is_some() {
            state.active = active_changed;
        }
        if moved.is_some() {
            state.moved = moved;
        }
    }

    fn set_host_appearance(&self, appearance: WindowAppearance) {
        let mut callback = {
            let mut state = self.0.borrow_mut();
            if state.appearance == appearance {
                return;
            }
            state.appearance = appearance;
            state.appearance_changed.take()
        };
        if let Some(callback) = callback.as_mut() {
            callback();
        }
        if callback.is_some() {
            self.0.borrow_mut().appearance_changed = callback;
        }
    }

    fn notify_host_moved(&self) {
        let mut callback = self.0.borrow_mut().moved.take();
        if let Some(callback) = callback.as_mut() {
            callback();
        }
        if callback.is_some() {
            self.0.borrow_mut().moved = callback;
        }
    }

    fn set_host_display(&self, display: Rc<dyn PlatformDisplay>) {
        self.0.borrow_mut().display = display;
    }

    fn should_close(&self) -> bool {
        let callback = self.0.borrow_mut().should_close.take();
        let Some(mut callback) = callback else {
            return true;
        };
        let should_close = callback();
        self.0.borrow_mut().should_close = Some(callback);
        should_close
    }

    fn set_hovered(&self, hovered: bool) {
        let mut callback = {
            let mut state = self.0.borrow_mut();
            if state.is_hovered == hovered {
                return;
            }
            state.is_hovered = hovered;
            state.hovered.take()
        };
        if let Some(callback) = callback.as_mut() {
            callback(hovered);
        }
        if callback.is_some() {
            self.0.borrow_mut().hovered = callback;
        }
    }
}

impl HasWindowHandle for BevyPlatformWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let raw = self
            .0
            .borrow()
            .raw_window_handle
            .ok_or(HandleError::Unavailable)?;
        // SAFETY: Bevy's RawHandleWrapper retains the native window and the
        // adapter is only accessed on the main thread where it was synchronized.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for BevyPlatformWindow {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        let raw = self
            .0
            .borrow()
            .raw_display_handle
            .ok_or(HandleError::Unavailable)?;
        // SAFETY: the display handle is copied from Bevy's live native window
        // and this platform adapter never outlives the Bevy application.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

impl PlatformWindow for BevyPlatformWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }
    fn is_maximized(&self) -> bool {
        false
    }
    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }
    fn content_size(&self) -> Size<Pixels> {
        self.bounds().size
    }
    fn resize(&mut self, size: Size<Pixels>) {
        let mut state = self.0.borrow_mut();
        state.bounds.size = size;
        state.output.logical_size = Some(size);
    }
    fn scale_factor(&self) -> f32 {
        self.0.borrow().scale_factor
    }
    fn appearance(&self) -> WindowAppearance {
        self.0.borrow().appearance
    }
    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.0.borrow().display.clone())
    }
    fn mouse_position(&self) -> Point<Pixels> {
        self.mouse_position()
    }
    fn modifiers(&self) -> Modifiers {
        self.0.borrow().modifiers
    }
    fn capslock(&self) -> Capslock {
        self.0.borrow().capslock
    }
    fn set_input_handler(&mut self, handler: PlatformInputHandler) {
        self.0.borrow_mut().input_handler = Some(handler);
    }
    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.borrow_mut().input_handler.take()
    }
    fn prompt(
        &self,
        _: gpui::PromptLevel,
        _: &str,
        _: Option<&str>,
        _: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        bevy_log::warn!("native message prompts are not implemented by bevy_gpui");
        None
    }
    fn activate(&self) {
        self.0.borrow_mut().output.activate = true;
    }
    fn is_active(&self) -> bool {
        self.0.borrow().is_active
    }
    fn is_hovered(&self) -> bool {
        self.0.borrow().is_hovered
    }
    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.0.borrow().background_appearance
    }
    fn set_title(&mut self, title: &str) {
        let mut state = self.0.borrow_mut();
        state.title = title.to_owned();
        state.output.title = Some(title.to_owned());
    }
    fn set_background_appearance(&self, appearance: WindowBackgroundAppearance) {
        let mut state = self.0.borrow_mut();
        state.background_appearance = appearance;
        state.output.background_appearance = Some(appearance);
    }
    fn minimize(&self) {
        self.0.borrow_mut().output.minimize = true;
    }
    fn zoom(&self) {
        self.0.borrow_mut().output.maximize = true;
    }
    fn toggle_fullscreen(&self) {
        let mut state = self.0.borrow_mut();
        state.fullscreen = !state.fullscreen;
        state.output.fullscreen = Some(state.fullscreen);
    }
    fn is_fullscreen(&self) -> bool {
        self.0.borrow().fullscreen
    }
    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.borrow_mut().request_frame = Some(callback);
    }
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.borrow_mut().input = Some(callback);
    }
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.borrow_mut().active = Some(callback);
    }
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.borrow_mut().hovered = Some(callback);
    }
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.borrow_mut().resized = Some(callback);
    }
    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().moved = Some(callback);
    }
    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.borrow_mut().should_close = Some(callback);
    }
    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        self.0.borrow_mut().hit_test = Some(callback);
    }
    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.borrow_mut().close = Some(callback);
    }
    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().appearance_changed = Some(callback);
    }
    fn draw(&self, scene: &Scene) {
        match scene.snapshot() {
            Ok(snapshot) => {
                let mut state = self.0.borrow_mut();
                state.scene_generation = state.scene_generation.wrapping_add(1);
                state.latest_snapshot = Some(snapshot);
            }
            Err(error) => bevy_log::error!(?error, "failed to snapshot GPUI scene"),
        }
    }
    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.borrow().atlas.clone()
    }
    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }
    fn gpu_specs(&self) -> Option<GpuSpecs> {
        None
    }
    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().output.ime_position = Some(bounds.origin);
    }

    #[cfg(target_os = "windows")]
    fn get_raw_handle(&self) -> windows::Win32::Foundation::HWND {
        match self.0.borrow().raw_window_handle {
            Some(RawWindowHandle::Win32(handle)) => {
                windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _)
            }
            _ => windows::Win32::Foundation::HWND::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bevy_monitor_physical_geometry_becomes_gpui_logical_bounds() {
        let display = BevyDisplay::new(9);
        display.sync(
            bevy_math::IVec2::new(200, -100),
            bevy_math::UVec2::new(3840, 2160),
            2.0,
        );

        assert_eq!(display.id(), DisplayId::new(9));
        assert_eq!(f32::from(display.bounds().origin.x), 100.0);
        assert_eq!(f32::from(display.bounds().origin.y), -50.0);
        assert_eq!(f32::from(display.bounds().size.width), 1920.0);
        assert_eq!(f32::from(display.bounds().size.height), 1080.0);
    }
}
