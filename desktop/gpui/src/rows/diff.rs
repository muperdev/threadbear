// Diffs: RowRenderer's diffText, diffWithNote, fileNameLine and changesRender,
// and the Edited N files card (ChangesRowCell). A diff row is one flex row per
// line: GPUI has no tab stops, so the number, sign and text sit in columns.
use super::metrics::*;
use super::work::{icon, stats_text, DIGIT_WIDTH};
use super::row_wrap;
use crate::style::{self, size};
use crate::thread::ThreadPane;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use threadbear_core::reducer::{ChangedFiles, DiffBuilder, DiffDisplayLine, DiffLineKind, DiffPreview, EditDiff, PathDisplay, Row};

/// `previewLines`: how many lines a diff opened short shows (a break between
/// two diffs counts as a line, as `diffText` draws it).
pub(crate) fn preview_lines(diffs: &[EditDiff]) -> usize {
    let between = if names_files(diffs) { diffs.len() } else { diffs.len().saturating_sub(1) };
    let total: usize = diffs.iter().map(|d| DiffBuilder::display_lines(d, usize::MAX).len()).sum::<usize>() + between;
    DiffPreview::shown(total)
}

/// `namesFiles`: more than one file (a Codex patch), so each gets its name
/// over it; several edits to one file don't.
pub(crate) fn names_files(diffs: &[EditDiff]) -> bool {
    let mut paths: Vec<&String> = diffs.iter().map(|d| &d.path).collect();
    paths.sort();
    paths.dedup();
    paths.len() > 1
}

/// One row of a diff, in order: what `diffText` writes as a line.
enum DiffRow {
    FileName { name: String, folder: String, added: i64, removed: i64, first: bool },
    Break,
    Line { kind: DiffLineKind, number: Option<i64>, sign: &'static str, text: String, background: Option<Hsla>, new_file: bool },
}

/// The rows `diffText` lays out, and how many lines it left out.
fn diff_rows(diffs: &[EditDiff], max_lines: usize, root: Option<&str>, cx: &App) -> (Vec<DiffRow>, usize) {
    let (added_bg, removed_bg) = (style::added_line_bg(cx), style::removed_line_bg(cx));
    let named = names_files(diffs);
    let mut lines: Vec<DiffDisplayLine> = vec![];
    let mut placed = vec![];
    let mut hidden = 0usize;
    for (index, diff) in diffs.iter().enumerate() {
        let room = max_lines.saturating_sub(lines.len());
        if room == 0 {
            hidden += DiffBuilder::display_lines(diff, usize::MAX).len();
            continue;
        }
        if named {
            let (name, folder) = (diff.path.rsplit('/').next().unwrap_or(&diff.path).to_owned(), PathDisplay::folder_relative(&diff.path, root));
            let (a, r) = DiffBuilder::stats(diff);
            placed.push((lines.len(), DiffRow::FileName { name, folder, added: a, removed: r, first: index == 0 }));
        } else if index > 0 {
            placed.push((lines.len(), DiffRow::Break));
        }
        for line in DiffBuilder::display_lines(diff, room) {
            if line.kind == DiffLineKind::Elided {
                hidden += line.text.split(' ').next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
            } else {
                lines.push(line);
            }
        }
    }
    // A new file is all additions: shown as plain content with green signs
    // rather than a solid green slab, the loudest thing on screen.
    let new_file = lines.iter().all(|l| l.kind != DiffLineKind::Removed && l.kind != DiffLineKind::Context);
    let mut rows = vec![];
    let mut placed = placed.into_iter().peekable();
    for (i, line) in lines.iter().enumerate() {
        while placed.peek().is_some_and(|(at, _)| *at == i) {
            rows.push(placed.next().unwrap().1);
        }
        rows.push(match line.kind {
            // A quiet break between hunks rather than the @@ header.
            DiffLineKind::HunkHeader | DiffLineKind::Elided => DiffRow::Break,
            DiffLineKind::Added => DiffRow::Line { kind: DiffLineKind::Added, number: line.new_number, sign: "+", text: tabs_to_spaces(&line.text), background: (!new_file).then_some(added_bg), new_file },
            // One column of new-file numbers that only counts up: a removed
            // line's old number made it run 20, 20, ... 27, 25.
            DiffLineKind::Removed => DiffRow::Line { kind: DiffLineKind::Removed, number: None, sign: "−", text: tabs_to_spaces(&line.text), background: Some(removed_bg), new_file },
            DiffLineKind::Context => DiffRow::Line { kind: DiffLineKind::Context, number: line.new_number, sign: "", text: tabs_to_spaces(&line.text), background: None, new_file },
        });
    }
    for (_, row) in placed {
        rows.push(row);
    }
    (rows, hidden)
}

fn tabs_to_spaces(text: &str) -> String {
    text.replace('\t', "    ")
}

/// What sits under a diff that was cut short.
pub(crate) enum DiffNote {
    /// `diffWithNote`'s "Show 30 more lines", opening the rest.
    ShowMore(Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>),
    /// All of it is already open, up to the cap: a quiet "N more lines not shown".
    Quiet,
    /// Nothing: an approval card's preview shows its own start and no note.
    None,
}

/// `diffText` and what `diffWithNote` adds under it.
pub(crate) fn diff_with_note(key: impl Into<SharedString>, diffs: &[EditDiff], max_lines: usize, inset: f32, root: Option<&str>, note: DiffNote, cx: &App) -> Div {
    let key: SharedString = key.into();
    let (faint, quiet, label) = (style::faint_text(cx), style::quiet_text(cx), style::label_text(cx));
    let (rows, hidden) = diff_rows(diffs, max_lines, root, cx);
    let digits = rows
        .iter()
        .filter_map(|r| match r {
            DiffRow::Line { number: Some(n), .. } => Some(*n),
            _ => None,
        })
        .max()
        .map(|n| n.to_string().len())
        .unwrap_or(0)
        .max(2);
    let number_width = digits as f32 * DIGIT_WIDTH;
    let added_text = style::added_text(cx);
    let mono = cx.theme().mono_font_family.clone();
    let mut col = v_flex().font_family(mono).text_size(px(size::CODE)).line_height(px(size::CODE_LINE));
    for (i, row) in rows.into_iter().enumerate() {
        col = col.child(match row {
            DiffRow::FileName { name, folder, added, removed, first } => h_flex()
                .when(!first, |d| d.mt(px(8.)))
                .px(px(inset))
                .text_size(px(size::DETAIL))
                .line_height(px(size::CODE_LINE))
                .gap(px(2.))
                .child(div().text_color(quiet).child(name))
                .when(!folder.is_empty(), |d| d.child(div().ml(px(8.)).text_color(faint).child(folder)))
                .child(div().ml(px(8.)).child(stats_text(added, removed, added_text, style::removed_text(cx))))
                .into_any_element(),
            DiffRow::Break => h_flex().pl(px(inset + number_width + 10.)).text_color(faint).child("⋯").into_any_element(),
            DiffRow::Line { kind, number, sign, text, background, new_file } => {
                let sign_color = if kind == DiffLineKind::Added && new_file { added_text } else { faint };
                h_flex()
                    .id((key.clone(), i))
                    .w_full()
                    .items_start()
                    .pl(px(inset))
                    .when_some(background, |d, bg| d.bg(bg))
                    .child(div().flex_none().w(px(number_width)).text_right().text_color(faint).child(number.map(|n| n.to_string()).unwrap_or_default()))
                    .child(div().flex_none().w(px(10.)))
                    .child(div().flex_none().w(px(14.)).text_color(sign_color).child(sign))
                    // Wrapped lines stop as far from the box's right edge as
                    // the numbers start from its left.
                    .child(div().flex_1().min_w_0().pr(px(inset)).text_color(label).child(gpui_kit::base::SelectableText::new(("t", i), text)))
                    .into_any_element()
            }
        });
    }
    if hidden > 0 && !matches!(note, DiffNote::None) {
        let lines = if hidden == 1 { "1 more line".to_owned() } else { format!("{hidden} more lines") };
        col = col.child(match note {
            DiffNote::ShowMore(on_click) => div()
                .id((key.clone(), 1_000_000usize))
                .pl(px(inset))
                .h(px(26.))
                .flex()
                .items_center()
                .text_size(px(size::DETAIL))
                .text_color(quiet)
                .cursor_pointer()
                .hover(|s| s.underline())
                .tooltip(super::tip("Show the whole diff"))
                .aria_label(SharedString::from(format!("Show {lines}")))
                .role(Role::Button)
                .on_click(move |e, window, cx| on_click(e, window, cx))
                .child(format!("Show {lines}"))
                .test_support()
                .into_any_element(),
            _ => div().pl(px(inset)).h(px(24.)).flex().items_center().text_size(px(size::SMALL)).text_color(faint).child(format!("{lines} not shown")).into_any_element(),
        });
    }
    col
}

impl ThreadPane {
    /// The open diff under a tool row, in its box.
    pub(crate) fn diff_box(&mut self, ix: usize, row: &Row, diffs: &[EditDiff], root: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let all_key = DiffPreview::all_key(&row.id);
        let show_all = self.open_rows.contains(&all_key);
        let max_lines = if show_all { MAX_DIFF_LINES } else { preview_lines(diffs) };
        let note = if show_all {
            DiffNote::Quiet
        } else {
            let (row_id, key) = (row.id.clone(), all_key.clone());
            DiffNote::ShowMore(Box::new(cx.listener(move |this, _: &ClickEvent, _, cx| this.show_all_lines(&key, &row_id, cx))))
        };
        let body = diff_with_note(SharedString::from(format!("diff-{ix}")), diffs, max_lines, 10., root, note, cx);
        div().ml(px(TITLE_INDENT)).mt(px(2.)).mb(px(8.)).py(px(DIFF_PAD_V)).rounded(px(10.)).bg(style::card_background(cx)).border_1().border_color(style::card_border(cx)).child(body).into_any_element()
    }

    /// "Edited 3 files": each file with its folder and line counts; a click
    /// opens its diff under it (`ChangesRowCell`).
    pub(crate) fn changes_card(&mut self, ix: usize, row: &Row, changes: &ChangedFiles, cx: &mut Context<Self>) -> AnyElement {
        let (card_bg, border, hairline, label, faint, faint_icon, hover) = (
            style::card_background(cx),
            style::card_border(cx),
            style::hairline(cx),
            style::label_text(cx),
            style::faint_text(cx),
            style::faint_icon(cx),
            style::hover_fill(cx),
        );
        let (added, removed) = (style::added_text(cx), style::removed_text(cx));
        let header = h_flex()
            .h(px(CHANGES_HEADER))
            .px(px(CARD_PAD))
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(hairline)
            .child(div().text_size(px(size::CARD_TITLE)).line_height(px(18.)).font_weight(FontWeight::MEDIUM).text_color(label).child(changes.title()))
            .child(stats_text(changes.added(), changes.removed(), added, removed));
        let mut card = v_flex().rounded(px(10.)).bg(card_bg).border_1().border_color(border).child(header);
        for (i, file) in changes.files.iter().enumerate() {
            let key = ChangedFiles::expansion_key(&changes.turn_id, &file.path);
            let toggle_key = key.clone();
            let line = h_flex()
                .id(("change-file", ix * 1000 + i))
                .w_full()
                .h(px(CHANGES_FILE))
                .px(px(CARD_PAD - 2.))
                .items_center()
                .cursor_pointer()
                .rounded(px(6.))
                .mx(px(4.))
                .hover(move |s| s.bg(hover))
                .tooltip(super::tip(&file.path))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_changed_file(&toggle_key, cx)))
                .child(div().flex_none().w(px(16.)).flex().items_center().child(icon(if file.is_expanded { "chevron.down" } else { "chevron.right" }, 11., faint_icon)))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .items_baseline()
                        .text_size(px(size::WORK))
                        .line_height(px(20.))
                        .text_color(label)
                        .child(div().min_w_0().flex_shrink_0().child(file.name()))
                        .when(!file.folder().is_empty(), |d| d.child(div().ml(px(8.)).min_w_0().truncate().text_size(px(size::DETAIL)).text_color(faint).child(file.folder()))),
                )
                .child(div().flex_none().ml(px(12.)).child(stats_text(file.added, file.removed, added, removed)));
            card = card.child(line);
            if file.is_expanded {
                let note = if file.shows_all {
                    DiffNote::Quiet
                } else {
                    let all = DiffPreview::all_key(&key);
                    DiffNote::ShowMore(Box::new(cx.listener(move |this, _: &ClickEvent, _, cx| this.show_all_changed(&all, cx))))
                };
                let max = if file.shows_all { 600 } else { preview_lines(&file.diffs) };
                let body = diff_with_note(SharedString::from(format!("change-diff-{ix}-{i}")), &file.diffs, max, CARD_PAD, None, note, cx);
                card = card.child(div().pb(px(8.)).child(body));
            }
        }
        row_wrap(row, div().pt(px(8.)).pb(px(6.)).child(card.pb(px(6.))))
    }
}
