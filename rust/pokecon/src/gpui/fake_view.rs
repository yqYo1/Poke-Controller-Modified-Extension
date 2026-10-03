//! Native fake-data view for the `GPUI` Phase 1 `PoC`.
//!
//! This view renders hard-coded fake data only. It never opens a backend
//! connection, never owns device state, and never spawns a worker. Its
//! purpose is to prove the native window, CJK text shaping, keyboard focus
//! traversal, the platform IME input path, clipboard writes, and the
//! accessibility tree on one real GPUI view before any production UI is
//! ported.

use std::ops::Range;

use gpui_kit::InteractiveElement as _;
use gpui_kit::{
    App, Bounds, ClickEvent, ClipboardItem, Context, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, IntoElement, KeyBinding, KeyDownEvent,
    ParentElement as _, Pixels, Render, Role, ShapedLine, SharedString,
    StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, TextAlign, TextRun,
    UTF16Selection, Window, actions, canvas, div, fill, point, px, rgb, rgba, size,
};

actions!([GpuiTab, GpuiTabPrev]);

/// Fake settings memo shown by the `PoC` view. The buffer is the single source
/// of truth for the IME input handler below, so platform composition events
/// and local keystrokes converge on the same text.
pub struct GpuiFakeView {
    focus_handle: FocusHandle,
    input_focus: FocusHandle,
    copy_focus: FocusHandle,
    quit_focus: FocusHandle,
    input_text: String,
    selected_range: Range<usize>,
    marked_range: Option<Range<usize>>,
    input_bounds: Option<Bounds<Pixels>>,
    last_copied: Option<String>,
    status_line: String,
}

impl GpuiFakeView {
    /// Creates the view with one focus handle per interactive control so Tab
    /// traversal has stable stops to move between.
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.bind_keys([
            KeyBinding::new("tab", GpuiTab, None),
            KeyBinding::new("shift-tab", GpuiTabPrev, None),
        ]);
        let input_text = String::from("設定メモ (fake)");
        let input_end = input_text.encode_utf16().count();
        Self {
            focus_handle: cx.focus_handle(),
            input_focus: cx.focus_handle().tab_stop(true),
            copy_focus: cx.focus_handle().tab_stop(true),
            quit_focus: cx.focus_handle().tab_stop(true),
            input_text,
            selected_range: input_end..input_end,
            marked_range: None,
            input_bounds: None,
            last_copied: None,
            status_line: String::from("起動しました (fake, backend 未接続)"),
        }
    }

    /// Replaces `range` (UTF-16 units, as reported by the platform) with
    /// `text`, or the marked/selected range when the platform passes no range.
    fn replace_input_text(&mut self, range: Option<Range<usize>>, text: &str) {
        let units: Vec<u16> = self.input_text.encode_utf16().collect();
        let range = range
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let (start, end) = (range.start.min(units.len()), range.end.min(units.len()));
        let (start, end) = (start.min(end), end.max(start));
        let replacement: Vec<u16> = text.replace(['\r', '\n'], " ").encode_utf16().collect();
        let mut next = Vec::with_capacity(units.len() + replacement.len());
        next.extend(units.iter().copied().take(start));
        next.extend(replacement.iter().copied());
        next.extend(units.iter().copied().skip(end));
        self.input_text = String::from_utf16_lossy(&next);
        let input_end = start + replacement.len();
        self.selected_range = input_end..input_end;
        self.marked_range = None;
        self.status_line = format!(
            "入力を更新しました (fake, {} 文字)",
            self.input_text.chars().count()
        );
    }

    fn delete_backward(&mut self) {
        let range = if self.selected_range.start != self.selected_range.end {
            self.selected_range.clone()
        } else if self.selected_range.start == 0 {
            return;
        } else {
            let units: Vec<u16> = self.input_text.encode_utf16().collect();
            let cursor = self.selected_range.start.min(units.len());
            let mut start = cursor - 1;
            if start > 0
                && (units[start] & 0xfc00) == 0xdc00
                && (units[start - 1] & 0xfc00) == 0xd800
            {
                start -= 1;
            }
            start..cursor
        };
        self.replace_input_text(Some(range), "");
        self.status_line = String::from("1文字削除しました (fake)");
    }

    fn replace_and_mark_input_text(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
    ) {
        let units: Vec<u16> = self.input_text.encode_utf16().collect();
        let range = range
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let (start, end) = (range.start.min(units.len()), range.end.min(units.len()));
        let (start, end) = (start.min(end), end.max(start));
        let replacement: Vec<u16> = new_text.replace(['\r', '\n'], " ").encode_utf16().collect();
        let mut next = Vec::with_capacity(units.len() + replacement.len());
        next.extend(units.iter().copied().take(start));
        next.extend(replacement.iter().copied());
        next.extend(units.iter().copied().skip(end));
        self.input_text = String::from_utf16_lossy(&next);
        let inserted_end = start + replacement.len();
        self.marked_range = (!replacement.is_empty()).then_some(start..inserted_end);
        self.selected_range = new_selected_range
            .map(|selected| {
                (start + selected.start.min(replacement.len()))
                    ..(start + selected.end.min(replacement.len()))
            })
            .unwrap_or(inserted_end..inserted_end);
        self.status_line = format!(
            "入力を更新しました (fake, {} 文字)",
            self.input_text.chars().count()
        );
    }

    fn slice_utf16(&self, range: Range<usize>) -> Option<String> {
        let units: Vec<u16> = self.input_text.encode_utf16().collect();
        let end = range.end.min(units.len());
        let start = range.start.min(end);
        let selected: Vec<u16> = units.iter().copied().take(end).skip(start).collect();
        String::from_utf16(&selected).ok()
    }

    /// Converts a platform UTF-16 offset to a UTF-8 byte boundary for GPUI's
    /// shaping APIs. Offsets inside a surrogate pair resolve to the end of the
    /// scalar so the result is always a valid Rust string boundary.
    fn byte_index_for_utf16(&self, target: usize) -> usize {
        let mut units = 0;
        for (index, character) in self.input_text.char_indices() {
            if units >= target {
                return index;
            }
            units += character.len_utf16();
            if units >= target {
                return index + character.len_utf8();
            }
        }
        self.input_text.len()
    }

    /// Converts a UTF-8 byte boundary returned by GPUI's shaped line back to
    /// the UTF-16 units required by the platform input handler.
    fn utf16_index_for_byte(&self, target: usize) -> usize {
        let target = target.min(self.input_text.len());
        let boundary = self
            .input_text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(self.input_text.len()))
            .find(|index| *index >= target)
            .unwrap_or(self.input_text.len());
        self.input_text[..boundary].encode_utf16().count()
    }

    /// Shapes the exact single-line string rendered by the input element. The
    /// displayed `入力: ` prefix is included so IME candidate and hit-test
    /// positions refer to the pixels the user actually sees.
    fn input_line(&self, window: &Window) -> (ShapedLine, usize) {
        let displayed = format!("入力: {}", self.input_text);
        let prefix_bytes = "入力: ".len();
        let style = window.text_style();
        let run = TextRun {
            len: displayed.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            SharedString::new(displayed),
            style.font_size.to_pixels(window.rem_size()),
            &[run],
            None,
        );
        (line, prefix_bytes)
    }
}

impl Focusable for GpuiFakeView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for GpuiFakeView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        self.slice_utf16(range)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selected_range.clone(),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_input_text(range, text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_and_mark_input_text(range, new_text, new_selected_range);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.input_bounds = Some(element_bounds);
        let (line, prefix_bytes) = self.input_line(window);
        let start = self
            .byte_index_for_utf16(range_utf16.start)
            .saturating_add(prefix_bytes);
        let end = self
            .byte_index_for_utf16(range_utf16.end)
            .saturating_add(prefix_bytes);
        let start_x = line.x_for_index(start);
        let end_x = line.x_for_index(end.max(start));
        let width = if end_x > start_x {
            end_x - start_x
        } else {
            px(1.)
        };
        let height = if element_bounds.size.height > px(0.) {
            element_bounds.size.height
        } else {
            window.line_height()
        };
        Some(Bounds::new(
            point(element_bounds.left() + start_x, element_bounds.top()),
            size(width, height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.input_bounds?;
        let (line, prefix_bytes) = self.input_line(window);
        let local_x = point.x - bounds.left();
        let local_x = if local_x <= px(0.) {
            px(0.)
        } else if local_x >= line.width() {
            line.width()
        } else {
            local_x
        };
        let byte_index = line.closest_index_for_x(local_x);
        Some(if byte_index <= prefix_bytes {
            0
        } else {
            self.utf16_index_for_byte(byte_index - prefix_bytes)
        })
    }
}

impl Render for GpuiFakeView {
    #[allow(clippy::too_many_lines)]
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view: Entity<Self> = cx.entity();
        let input_view = view.clone();
        let input_focus = self.input_focus.clone();

        div()
            .id("gpui-fake-root")
            .role(Role::Group)
            .aria_label("Poke-Con GPUI PoC 操作画面")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .bg(rgb(0x001e_1e2e))
            .text_color(rgb(0x00cd_d6f4))
            .on_action(cx.listener(|_, _: &GpuiTab, window, cx| {
                window.focus_next(cx);
            }))
            .on_action(cx.listener(|_, _: &GpuiTabPrev, window, cx| {
                window.focus_prev(cx);
            }))
            .child(
                div()
                    .id("gpui-fake-title")
                    .role(Role::Label)
                    .aria_label("Poke-Con GPUI PoC タイトル")
                    .child("Poke-Con GPUI PoC — 設定スナップショット (fake data)"),
            )
            .child(
                div()
                    .id("gpui-fake-profile")
                    .role(Role::Label)
                    .aria_label("プロファイル表示 (fake)")
                    .child("プロファイル: デフォルト (fake)"),
            )
            .child(
                div()
                    .id("gpui-fake-status-row")
                    .role(Role::Label)
                    .aria_label("接続状態表示 (fake)")
                    .child("接続: 切断中 (fake) ／ 遅延: 未計測 (fake)"),
            )
            .child(
                div()
                    .id("gpui-fake-input")
                    .role(Role::TextInput)
                    .aria_label("設定メモ入力 (fake, 日本語IME対応)")
                    .aria_description(
                        "日本語IMEの変換確定や貼り付けはこの欄に反映されます。内容は保存されません。",
                    )
                    .test_support()
                    .track_focus(&self.input_focus)
                    .tab_stop(true)
                    .w_full()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x0031_3244))
                    .text_color(rgb(0x00cd_d6f4))
                    .border_1()
                    .border_color(rgb(0x0058_5b70))
                    .on_click(cx.listener(
                        |view: &mut Self,
                         _event: &ClickEvent,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
                            window.focus(&view.input_focus, cx);
                        },
                    ))
                    .on_key_down(cx.listener(
                        |view: &mut Self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>| {
                            let key = event.keystroke.key.as_str();
                            if event.keystroke.modifiers.control && key == "v" {
                                if let Some(text) = cx
                                    .read_from_clipboard()
                                    .and_then(|item| item.text())
                                {
                                    view.replace_input_text(None, &text);
                                    cx.notify();
                                }
                            } else if key == "backspace" {
                                view.delete_backward();
                                cx.notify();
                            }
                        },
                    ))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, (), window: &mut Window, cx: &mut App| {
                                let (line, caret_byte, marked_bytes) = {
                                    let view = input_view.read(cx);
                                    let (line, prefix_bytes) = view.input_line(window);
                                    let caret_byte =
                                        prefix_bytes + view.byte_index_for_utf16(view.selected_range.end);
                                    let marked_bytes = view.marked_range.as_ref().map(|range| {
                                        (
                                            prefix_bytes + view.byte_index_for_utf16(range.start),
                                            prefix_bytes + view.byte_index_for_utf16(range.end),
                                        )
                                    });
                                    (line, caret_byte, marked_bytes)
                                };
                                let bounds_view = input_view.clone();
                                bounds_view.update(cx, |view, _| {
                                    view.input_bounds = Some(bounds);
                                });
                                window.handle_input(
                                    &input_focus,
                                    ElementInputHandler::new(bounds, input_view.clone()),
                                    cx,
                                );
                                if let Some((marked_start, marked_end)) = marked_bytes {
                                    window.paint_quad(fill(
                                        Bounds::from_corners(
                                            point(
                                                bounds.left() + line.x_for_index(marked_start),
                                                bounds.top(),
                                            ),
                                            point(
                                                bounds.left() + line.x_for_index(marked_end.max(marked_start)),
                                                bounds.bottom(),
                                            ),
                                        ),
                                        rgba(0x89b4_fa55),
                                    ));
                                }
                                line.paint(
                                    bounds.origin,
                                    window.line_height(),
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                )
                                .ok();
                                if input_focus.is_focused(window) {
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(
                                                bounds.left() + line.x_for_index(caret_byte),
                                                bounds.top(),
                                            ),
                                            size(px(2.), bounds.size.height),
                                        ),
                                        rgb(0x0089_b4fa),
                                    ));
                                }
                            },
                        )
                        .w_full()
                        .h(px(24.)),
                    ),
            )
            .child(
                div()
                    .id("gpui-fake-copy")
                    .role(Role::Button)
                    .aria_label("表示テキストをクリップボードへコピー")
                    .aria_description("入力欄の内容をOSのクリップボードへ書き込みます。")
                    .test_support()
                    .track_focus(&self.copy_focus)
                    .tab_stop(true)
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x0089_b4fa))
                    .text_color(rgb(0x001e_1e2e))
                    .cursor_pointer()
                    .on_click(cx.listener(
                        |view: &mut Self,
                         _event: &ClickEvent,
                         _window: &mut Window,
                         cx: &mut Context<Self>| {
                            let text = format!("Poke-Con GPUI PoC fake snapshot: {}", view.input_text);
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            view.last_copied = Some(text);
                            view.status_line = String::from("コピーしました (fake)");
                            cx.notify();
                        },
                    ))
                    .child("表示テキストをコピー"),
            )
            .child(
                div()
                    .id("gpui-fake-quit")
                    .role(Role::Button)
                    .aria_label("GPUI PoC を終了")
                    .aria_description("ウィンドウを閉じてbackendと共に終了します。")
                    .test_support()
                    .track_focus(&self.quit_focus)
                    .tab_stop(true)
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x00f3_8ba8))
                    .text_color(rgb(0x001e_1e2e))
                    .cursor_pointer()
                    .on_click(cx.listener(
                        |_view: &mut Self,
                         _event: &ClickEvent,
                         _window: &mut Window,
                         cx: &mut Context<Self>| {
                            cx.quit();
                        },
                    ))
                    .child("GPUI PoC を終了"),
            )
            .child(
                div()
                    .id("gpui-fake-status")
                    .role(Role::Label)
                    .aria_label("状態表示")
                    .child(format!(
                        "状態: {} ／ 最終コピー: {}",
                        self.status_line,
                        self.last_copied.as_deref().unwrap_or("なし")
                    )),
            )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, ClipboardItem, TestAppContext, px};

    use super::GpuiFakeView;

    /// Exercise the real native element tree and event dispatcher in the
    /// headless GPUI test harness: CJK input, focus traversal, accessibility
    /// labels, and the OS clipboard callback all use the production view.
    #[gpui_kit::test]
    fn fake_view_accepts_cjk_input_focus_and_clipboard(cx: &mut TestAppContext) {
        let handle = cx.add_window(|_, cx| GpuiFakeView::new(cx));
        cx.update_window(handle.into(), |_, window, cx| {
            assert_eq!(
                window.find("gpui-fake-input").label(),
                Some("設定メモ入力 (fake, 日本語IME対応)"),
            );
            assert_eq!(
                window.find("gpui-fake-copy").label(),
                Some("表示テキストをクリップボードへコピー"),
            );
            for id in [
                "gpui-fake-root",
                "gpui-fake-input",
                "gpui-fake-copy",
                "gpui-fake-quit",
            ] {
                let snapshot = window.find(id);
                assert!(
                    snapshot.visible(),
                    "{id} must be visible in the completed frame"
                );
                assert!(
                    snapshot.bounds().size.width > px(0.) && snapshot.bounds().size.height > px(0.),
                    "{id} must have non-zero layout bounds: {:?}",
                    snapshot.bounds()
                );
            }
            window.click("gpui-fake-input", cx);
            assert_eq!(window.find("gpui-fake-input").focused(), Some(true));
            cx.write_to_clipboard(ClipboardItem::new_string("貼り付け".to_string()));
            window.press("ctrl-v", cx);
            window.input("日本語🦀", cx);
            window.press("tab", cx);
            assert_eq!(window.find("gpui-fake-copy").focused(), Some(true));
            window.click("gpui-fake-copy", cx);
            let copied = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap_or_default();
            assert!(copied.contains("日本語🦀"), "clipboard={copied:?}");

            window.click("gpui-fake-input", cx);
            window.press("backspace", cx);
            window.press("tab", cx);
            window.click("gpui-fake-copy", cx);
            let copied_after_backspace = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap_or_default();
            assert!(copied_after_backspace.contains("日本語"));
            assert!(!copied_after_backspace.contains('🦀'));
        })
        .expect("fake view interaction should render and dispatch");

        handle
            .update(cx, |view, _, _| {
                assert!(view.input_text.contains("貼り付け"));
                assert!(view.input_text.contains("日本語"));
                assert!(!view.input_text.contains('🦀'));
                assert_eq!(
                    view.last_copied
                        .as_deref()
                        .map(|text| text.contains("日本語")),
                    Some(true)
                );

                // The platform-facing selection is UTF-16 while Rust string
                // replacement and shaping use UTF-8 byte boundaries. Keep the
                // surrogate-pair conversion and Unicode-safe backspace under
                // the same real view fixture as the event-dispatch assertions.
                view.input_text = String::from("A🦀B");
                assert_eq!(view.byte_index_for_utf16(1), 1);
                assert_eq!(view.byte_index_for_utf16(2), 5);
                assert_eq!(view.byte_index_for_utf16(3), 5);
                assert_eq!(view.utf16_index_for_byte(1), 1);
                assert_eq!(view.utf16_index_for_byte(5), 3);
                assert_eq!(view.utf16_index_for_byte(6), 4);
                view.selected_range = 1..3;
                view.replace_input_text(None, "🧪");
                assert_eq!(view.input_text, "A🧪B");
                assert_eq!(view.selected_range, 3..3);
                view.delete_backward();
                assert_eq!(view.input_text, "AB");
                assert_eq!(view.selected_range, 1..1);
            })
            .expect("view state should remain readable after interaction");
    }

    /// Headless IME geometry slice on the real view: marked UTF-16 state plus
    /// `bounds_for_range` / `character_index_for_point` behavior. Uses only
    /// the headless GPUI shaper; it makes no claim about platform
    /// composition, rendered pixels, or candidate windows.
    #[allow(clippy::too_many_lines)]
    #[gpui_kit::test]
    fn fake_view_ime_marked_range_and_geometry_headless(cx: &mut TestAppContext) {
        use gpui_kit::{Bounds, EntityInputHandler as _, point, size};

        let handle = cx.add_window(|_, cx| GpuiFakeView::new(cx));
        handle
            .update(cx, |view, window, cx| {
                view.input_text = String::from("ABC日本語");
                let insert_at = view.input_text.encode_utf16().count();
                view.selected_range = insert_at..insert_at;
                view.marked_range = None;

                let marked_text = "かき🦀";
                let marked_units = marked_text.encode_utf16().count();
                view.replace_and_mark_text_in_range(
                    None,
                    marked_text,
                    Some(0..marked_units),
                    window,
                    cx,
                );
                let marked = insert_at..insert_at + marked_units;
                assert_eq!(view.marked_range, Some(marked.clone()));
                assert_eq!(view.selected_range, marked.clone());
                assert_eq!(view.marked_text_range(window, cx), Some(marked.clone()));
                assert_eq!(
                    view.selected_text_range(false, window, cx)
                        .map(|selection| selection.range),
                    Some(marked.clone())
                );
                let mut adjusted = None;
                assert_eq!(
                    view.text_for_range(marked.clone(), &mut adjusted, window, cx)
                        .as_deref(),
                    Some(marked_text)
                );
                view.unmark_text(window, cx);
                assert_eq!(view.marked_range, None);
                assert_eq!(view.marked_text_range(window, cx), None);
                assert!(
                    view.input_text.ends_with(marked_text),
                    "unmark keeps text: {:?}",
                    view.input_text
                );

                let element = Bounds::new(point(px(10.), px(20.)), size(px(320.), px(24.)));
                let candidate = view
                    .bounds_for_range(marked.clone(), element, window, cx)
                    .expect("marked range must produce candidate bounds");
                assert!(candidate.size.width > px(0.));
                assert!(candidate.size.height > px(0.));
                assert_eq!(candidate.origin.y, element.origin.y);
                assert!(candidate.origin.x >= element.origin.x);
                let caret = view
                    .bounds_for_range(marked.end..marked.end, element, window, cx)
                    .expect("caret must produce bounds");
                assert_eq!(caret.size.width, px(1.));
                assert!(
                    candidate.size.width > caret.size.width,
                    "marked candidate {candidate:?} must be wider than caret {caret:?}"
                );
                let shifted = Bounds::new(point(px(60.), px(80.)), size(px(320.), px(24.)));
                let candidate_shifted = view
                    .bounds_for_range(marked.clone(), shifted, window, cx)
                    .expect("shifted element must produce bounds");
                let dx = candidate_shifted.origin.x - candidate.origin.x;
                let dy = candidate_shifted.origin.y - candidate.origin.y;
                assert!(
                    dx > px(49.) && dx < px(51.),
                    "candidate origin must track element origin: dx={dx:?}"
                );
                assert!(
                    dy > px(59.) && dy < px(61.),
                    "candidate origin must track element origin: dy={dy:?}"
                );

                let _ = view.bounds_for_range(marked.clone(), element, window, cx);
                let utf16_end = view.input_text.encode_utf16().count();
                assert_eq!(
                    view.character_index_for_point(element.origin, window, cx),
                    Some(0)
                );
                assert_eq!(
                    view.character_index_for_point(
                        point(element.origin.x - px(40.), element.origin.y),
                        window,
                        cx
                    ),
                    Some(0)
                );
                let (line, prefix_bytes) = view.input_line(window);
                let prefix_x = line.x_for_index(prefix_bytes);
                let mut previous = 0;
                for advance in [px(0.), px(2.), px(8.), px(32.), px(128.), px(512.)] {
                    let index = view
                        .character_index_for_point(
                            point(element.origin.x + prefix_x + advance, element.origin.y),
                            window,
                            cx,
                        )
                        .expect("hit test inside the element must resolve");
                    assert!(index <= utf16_end, "index={index} end={utf16_end}");
                    assert!(index >= previous, "index={index} previous={previous}");
                    previous = index;
                }
                let past_line = element.origin.x + line.width() + px(100.);
                assert_eq!(
                    view.character_index_for_point(point(past_line, element.origin.y), window, cx),
                    Some(utf16_end)
                );
            })
            .expect("ime geometry slice should run on the real view");
    }

    /// Headless keyboard-focus traversal and accessibility label slice on the
    /// real view: `Tab` cycles input -> copy -> quit with wrap, `Shift-Tab`
    /// reverses, and the observed role/label tree exposes
    /// `Group`/`TextInput`/`Button` with visible non-zero bounds. The status
    /// row carries no `.test_support()` registration in production, so it is
    /// not headlessly observable; this packet narrows to a `try_find`
    /// absence for it rather than claiming its tree. It makes no claim
    /// about platform IME, rendered pixels, live candidate windows, the OS
    /// clipboard, or AccessKit/AT-SPI enumeration.
    #[gpui_kit::test]
    fn fake_view_focus_cycle_and_accessibility_labels_headless(cx: &mut TestAppContext) {
        use gpui_kit::Role;

        let handle = cx.add_window(|_, cx| GpuiFakeView::new(cx));
        cx.update_window(handle.into(), |_, window, cx| {
            for (id, role, label) in [
                ("gpui-fake-root", Role::Group, "Poke-Con GPUI PoC 操作画面"),
                (
                    "gpui-fake-input",
                    Role::TextInput,
                    "設定メモ入力 (fake, 日本語IME対応)",
                ),
                (
                    "gpui-fake-copy",
                    Role::Button,
                    "表示テキストをクリップボードへコピー",
                ),
                ("gpui-fake-quit", Role::Button, "GPUI PoC を終了"),
            ] {
                let snapshot = window.find(id);
                assert_eq!(snapshot.role(), Some(role), "{id} role");
                assert_eq!(snapshot.label(), Some(label), "{id} label");
                assert!(
                    snapshot.visible(),
                    "{id} must be visible in the completed frame"
                );
                assert!(
                    snapshot.bounds().size.width > px(0.)
                        && snapshot.bounds().size.height > px(0.),
                    "{id} must have non-zero layout bounds: {:?}",
                    snapshot.bounds()
                );
            }
            assert!(
                window.try_find("gpui-fake-status").is_none(),
                "status row has no .test_support() registration, so it stays headlessly unobservable"
            );

            window.click("gpui-fake-input", cx);
            assert_eq!(window.find("gpui-fake-input").focused(), Some(true));
            window.press("tab", cx);
            assert_eq!(window.find("gpui-fake-copy").focused(), Some(true));
            assert_eq!(window.find("gpui-fake-input").focused(), Some(false));
            window.press("tab", cx);
            assert_eq!(window.find("gpui-fake-quit").focused(), Some(true));
            assert_eq!(window.find("gpui-fake-copy").focused(), Some(false));
            window.press("tab", cx);
            assert_eq!(
                window.find("gpui-fake-input").focused(),
                Some(true),
                "tab wraps from quit back to input"
            );
            window.press("shift-tab", cx);
            assert_eq!(
                window.find("gpui-fake-quit").focused(),
                Some(true),
                "shift-tab reverses the wrap"
            );
            window.press("shift-tab", cx);
            assert_eq!(window.find("gpui-fake-copy").focused(), Some(true));
            window.press("shift-tab", cx);
            assert_eq!(window.find("gpui-fake-input").focused(), Some(true));
        })
        .expect("focus cycle and accessibility labels should resolve on the real view");
    }
}
