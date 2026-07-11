//! Keyboard and IME text entry through a Bevy-owned window.

use std::ops::Range;

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
    },
    prelude::*,
    window::{Ime, PrimaryWindow},
};
use bevy_gpui::{
    GpuiContexts, GpuiPlugin, GpuiRuntimeStatus, GpuiViewHandle,
    gpui::{
        App as GpuiApp, Bounds, Context, Element, ElementId, ElementInputHandler,
        Entity as GpuiEntity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId,
        IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent, Pixels, Point, Render,
        SharedString, Style, UTF16Selection, Window, actions, div, prelude::*, px, relative, rgb,
    },
};

actions!(bevy_gpui_text_input, [Backspace, Left, Right]);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GpuiPlugin::default()))
        .add_systems(Startup, setup)
        .init_resource::<InputProbe>()
        .add_systems(Update, verify_input_bridge)
        .run();
}

fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
    let camera = commands.spawn(Camera2d).id();
    let root = gpui
        .set_root(camera, |window, cx| {
            cx.bind_keys([
                KeyBinding::new("backspace", Backspace, Some("BevyGpuiTextField")),
                KeyBinding::new("left", Left, Some("BevyGpuiTextField")),
                KeyBinding::new("right", Right, Some("BevyGpuiTextField")),
            ]);
            let field = cx.new(|cx| TextField {
                focus: cx.focus_handle(),
                content: SharedString::default(),
                selection: 0..0,
                marked: None,
                last_bounds: None,
            });
            let focus = field.read(cx).focus.clone();
            window.focus(&focus, cx);
            field
        })
        .expect("text input root should be queued");
    commands.insert_resource(TextRoot(root));
}

#[derive(Resource)]
struct TextRoot(GpuiViewHandle<TextField>);

#[derive(Default, Resource)]
struct InputProbe(u8);

fn verify_input_bridge(
    status: Res<GpuiRuntimeStatus>,
    root: Option<Res<TextRoot>>,
    window: Single<Entity, With<PrimaryWindow>>,
    mut keyboard: MessageWriter<KeyboardInput>,
    mut ime: MessageWriter<Ime>,
    mut probe: ResMut<InputProbe>,
    mut gpui: GpuiContexts,
) {
    let Some(root) = root else {
        return;
    };
    if status.roots == 0 {
        return;
    }
    let content = gpui
        .update(&root.0, |field, _, _| field.content.to_string())
        .unwrap_or_default();
    match probe.0 {
        0 => {
            keyboard.write(KeyboardInput {
                key_code: KeyCode::KeyH,
                logical_key: Key::Character("h".into()),
                state: ButtonState::Pressed,
                text: Some("Hello ".into()),
                repeat: false,
                window: *window,
            });
            probe.0 = 1;
        }
        1 if content == "Hello " => {
            ime.write(Ime::Preedit {
                window: *window,
                value: "世界".into(),
                cursor: Some((6, 6)),
            });
            probe.0 = 2;
        }
        2 if content == "Hello 世界" => {
            ime.write(Ime::Commit {
                window: *window,
                value: "世界".into(),
            });
            probe.0 = 3;
        }
        3 if content == "Hello 世界" => {
            info!("bevy_gpui keyboard and IME input bridge verified: {content:?}");
            probe.0 = 4;
        }
        _ => {}
    }
}

struct TextField {
    focus: FocusHandle,
    content: SharedString,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    last_bounds: Option<Bounds<Pixels>>,
}

impl TextField {
    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selection = offset..offset;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() && self.selection.start > 0 {
            let previous = self.content[..self.selection.start]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
            self.selection.start = previous;
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let previous = self.content[..self.selection.start]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        self.move_to(previous, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let current = self.selection.end;
        let next = self.content[current..]
            .char_indices()
            .nth(1)
            .map_or(self.content.len(), |(index, _)| current + index);
        self.move_to(next, cx);
    }

    fn focus(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.move_to(self.content.len(), cx);
    }

    fn utf8_offset_for_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for character in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf8 += character.len_utf8();
            utf16 += character.len_utf16();
        }
        utf8
    }

    fn to_utf16(&self, offset: usize) -> usize {
        self.content[..offset].encode_utf16().count()
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.utf8_offset_for_utf16(range.start)..self.utf8_offset_for_utf16(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.to_utf16(range.start)..self.to_utf16(range.end)
    }

    fn replace(&mut self, range: Range<usize>, text: &str) -> Range<usize> {
        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            text,
            &self.content[range.end..]
        )
        .into();
        range.start..range.start + text.len()
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &GpuiApp) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        *actual_range = Some(self.range_to_utf16(&range));
        Some(self.content[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selection),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let inserted = self.replace(range, text);
        self.selection = inserted.end..inserted.end;
        self.marked = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let inserted = self.replace(range, text);
        self.marked = (!inserted.is_empty()).then_some(inserted.clone());
        self.selection = selected_utf16.map_or(inserted.end..inserted.end, |selected| {
            let start = Self::utf8_offset_in(text, selected.start);
            let end = Self::utf8_offset_in(text, selected.end);
            inserted.start + start..inserted.start + end
        });
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.last_bounds.unwrap_or(bounds))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.to_utf16(self.selection.end))
    }
}

impl TextField {
    fn utf8_offset_in(text: &str, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for character in text.chars() {
            if utf16 >= offset {
                break;
            }
            utf8 += character.len_utf8();
            utf16 += character.len_utf16();
        }
        utf8
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        let text = if self.content.is_empty() {
            "Type here, including IME composition…".to_owned()
        } else {
            self.content.to_string()
        };
        div()
            .size_full()
            .p_8()
            .bg(rgb(0x0f_17_2a))
            .text_color(rgb(0xf8_fa_fc))
            .child("Bevy keyboard + IME → GPUI InputHandler")
            .child(
                div()
                    .id("text-field")
                    .key_context("BevyGpuiTextField")
                    .track_focus(&focus)
                    .mt_4()
                    .p_4()
                    .min_h_16()
                    .rounded_md()
                    .bg(rgb(0x1e_29_3b))
                    .on_action(cx.listener(Self::backspace))
                    .on_action(cx.listener(Self::left))
                    .on_action(cx.listener(Self::right))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::focus))
                    .child(text)
                    .child(InputSink { input: cx.entity() }),
            )
    }
}

struct InputSink {
    input: GpuiEntity<TextField>,
}

impl IntoElement for InputSink {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InputSink {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&bevy_gpui::gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut GpuiApp,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = px(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&bevy_gpui::gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut GpuiApp,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&bevy_gpui::gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut GpuiApp,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        self.input
            .update(cx, |field, _| field.last_bounds = Some(bounds));
    }
}
