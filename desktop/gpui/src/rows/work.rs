// The work lines: group summaries, tool and thinking rows, what they open to,
// and the working line (HeaderRowCell, GroupRowCell, WorkingRowCell and
// BackgroundRowCell in Cells.swift).
use super::metrics::*;
use super::{row_wrap, tip};
use crate::style::{self, size};
use crate::thread::ThreadPane;
use gpui_kit::base::SelectableText;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use threadbear_core::reducer::{BackgroundStatus, BackgroundWork, Detail, Field, NumberedLine, Row, ToolItem, ToolKind, ToolPresentation, TurnGap, WorkGroup, WorkSummary};
use threadbear_core::reducer::{ApprovalDecision, ApprovalItem, ApprovalText, DiffBuilder, SecretItem, TextItem};

/// The SVG bytes of the Lucide icon standing in for an SF Symbol, from the
/// kit's own catalog (its `Assets` only carries 101 of them). A name map
/// like this belongs in icons.rs once that exists.
pub(crate) fn lucide(symbol: &str) -> &'static [u8] {
    use gpui_kit::assets::__private as lucide;
    match symbol {
        "brain" => lucide::Brain.1,
        "terminal" => lucide::Terminal.1,
        "square.dashed" => lucide::SquareDashed.1,
        "photo" => lucide::Image.1,
        "doc.text" => lucide::FileText.1,
        "pencil.line" => lucide::PencilLine.1,
        "square.and.pencil" => lucide::SquarePen.1,
        "magnifyingglass" => lucide::Search.1,
        "globe" => lucide::Globe.1,
        "puzzlepiece.extension" => lucide::Puzzle.1,
        "person.2" => lucide::Users.1,
        "checklist" => lucide::ListChecks.1,
        "wrench.and.screwdriver" => lucide::Wrench.1,
        "hammer" => lucide::Hammer.1,
        "xmark.circle" | "xmark.circle.fill" => lucide::CircleX.1,
        "checkmark.circle" | "checkmark.circle.fill" => lucide::CircleCheck.1,
        "checkmark" => lucide::Check.1,
        "clock" => lucide::Clock.1,
        "lock" => lucide::Lock.1,
        "lock.shield" => lucide::ShieldCheck.1,
        "info.circle" => lucide::Info.1,
        "chevron.right" => lucide::ChevronRight.1,
        "chevron.down" => lucide::ChevronDown.1,
        "exclamationmark.triangle" => lucide::TriangleAlert.1,
        "pawprint" => lucide::PawPrint.1,
        "rectangle.on.rectangle" => lucide::Images.1,
        "doc.on.doc" => lucide::Copy.1,
        _ => lucide::Circle.1,
    }
}

/// An SF Symbol's stand-in at `side` points, in `color`.
pub(crate) fn icon(symbol: &str, side: f32, color: Hsla) -> Icon {
    Icon::default().data(lucide(symbol)).size(px(side)).text_color(color)
}

/// A work line's icon: Lucide draws edge to edge, so a little smaller than SF
/// Symbols at the same point size.
const ICON_SIZE: f32 = 13.;
const CHEVRON_SIZE: f32 = 11.;
/// What a work line says, in order, each piece in its own colour.
pub(crate) type Runs = Vec<(String, Hsla)>;

/// One text element with coloured runs: one line that truncates as a whole.
pub(crate) fn run_text(runs: &Runs, cx: &App) -> StyledText {
    let mut text = String::new();
    let mut highlights = vec![];
    for (words, color) in runs {
        let start = text.len();
        text.push_str(words);
        // See `over_background`: a translucent highlight would blend away.
        highlights.push((start..text.len(), HighlightStyle { color: Some(style::over_background(*color, cx)), ..Default::default() }));
    }
    StyledText::new(text).with_highlights(highlights)
}

/// `WorkLine`: an icon in a slot at the column's edge, the text from
/// `titleIndent`, the chevron right after the words. Hovering fills from the
/// icon to the chevron. A row that doesn't open just reads.
#[allow(clippy::too_many_arguments)]
pub(crate) fn work_line(
    id: ElementId,
    symbol: &str,
    icon_color: Hsla,
    runs: &Runs,
    extra: Option<AnyElement>,
    chevron: Option<bool>,
    tooltip: Option<&str>,
    cx: &App,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
) -> Stateful<Div> {
    let faint_icon = style::faint_icon(cx);
    let hover = style::hover_fill(cx);
    let opens = chevron.is_some();
    let mut line = h_flex()
        .id(id)
        .h(px(WORK_HEIGHT - 4.))
        .my(px(2.))
        .ml(px(-6.))
        .pl(px(6.))
        .pr(px(6.))
        .max_w_full()
        .rounded(px(6.))
        .items_center()
        .text_size(px(size::WORK))
        .line_height(px(size::WORK_LINE))
        .child(div().flex_none().w(px(TITLE_INDENT)).h_full().flex().items_center().child(icon(symbol, ICON_SIZE, icon_color)))
        .child(div().min_w_0().truncate().child(run_text(runs, cx)));
    if let Some(extra) = extra {
        line = line.child(extra);
    }
    if let Some(open) = chevron {
        line = line.child(div().flex_none().ml(px(3.)).child(icon(if open { "chevron.down" } else { "chevron.right" }, CHEVRON_SIZE, faint_icon)));
    }
    if opens {
        line = line.cursor_pointer().hover(move |s| s.bg(hover));
    }
    if let Some(on_click) = on_click {
        line = line.on_click(move |e, window, cx| on_click(e, window, cx));
    }
    if let Some(tooltip) = tooltip {
        line = line.tooltip(tip(tooltip));
    }
    line
}

impl ThreadPane {
    fn toggle_listener(&self, row: &Row, cx: &mut Context<Self>) -> Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static> {
        let row = row.clone();
        Box::new(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_row(&row, cx)))
    }

    pub(crate) fn group_row(&mut self, ix: usize, row: &Row, group: &WorkGroup, cx: &mut Context<Self>) -> AnyElement {
        let (quiet, faint, failed, label) = (style::quiet_text(cx), style::faint_text(cx), style::failed_text(cx), style::label_text(cx));
        let mut runs: Runs = vec![(group.summary.clone(), quiet)];
        if group.failed_count > 0 {
            // Only the count is red; the comma belongs to the quiet summary.
            runs.push((", ".to_owned(), quiet));
            runs.push((format!("{} failed", group.failed_count), failed));
        }
        if group.is_live {
            if let Some(action) = &group.current_action {
                // What's happening now is what Mike watches while a turn
                // runs, so it's the brightest work text on screen.
                runs.push(("  ·  ".to_owned(), faint));
                runs.push((ToolPresentation::short_title(action, 90), label));
            }
        }
        let open = group.is_expanded;
        let click = self.toggle_listener(row, cx);
        let line = work_line(("group", ix).into(), "hammer", style::faint_icon(cx), &runs, None, Some(open), None, cx, Some(click));
        row_wrap(row, div().child(line))
    }

    pub(crate) fn thinking_row(&mut self, ix: usize, row: &Row, item: &TextItem, cx: &mut Context<Self>) -> AnyElement {
        let title = if item.is_streaming { "Thinking" } else { "Thought" };
        let runs: Runs = vec![(title.to_owned(), style::quiet_text(cx)), (format!("  {}", WorkSummary::thought(&item.text)), style::faint_text(cx))];
        let open = self.row_open(row);
        let click = self.toggle_listener(row, cx);
        let line = work_line(("think", ix).into(), "brain", style::faint_icon(cx), &runs, None, Some(open), None, cx, Some(click));
        let body = open.then(|| {
            v_flex()
                .ml(px(TITLE_INDENT))
                .mb(px(12.))
                .text_size(px(size::WORK))
                .line_height(px(21.))
                .text_color(style::quiet_text(cx))
                .child(SelectableText::new(("think-body", ix), item.text.clone()))
        });
        row_wrap(row, v_flex().child(line).children(body))
    }

    pub(crate) fn tool_row(&mut self, ix: usize, row: &Row, tool: &ToolItem, cx: &mut Context<Self>) -> AnyElement {
        let (quiet, faint, added, removed) = (style::quiet_text(cx), style::faint_text(cx), style::added_text(cx), style::removed_text(cx));
        let (symbol, icon_color) = if tool.failed() { ("xmark.circle.fill", style::failed_text(cx)) } else { (ToolPresentation::symbol(tool.tool_kind(), &tool.name()), style::faint_icon(cx)) };
        let file = ToolPresentation::file_title(tool);
        let runs: Runs = match &file {
            Some(f) => vec![(format!("{} {}", f.verb, f.name), quiet)],
            None => vec![(ToolPresentation::short_title(&tool.title(), 160), quiet)],
        };
        let diffs: Vec<_> = tool.effective_diffs().into_iter().filter(|d| !d.hunks.is_empty()).collect();
        // The folder dimmed and the +N -M after it, as in the changes card.
        let mut extras = h_flex().flex_none().items_center().gap(px(6.)).ml(px(6.));
        let mut any_extra = false;
        if let Some(folder) = file.as_ref().filter(|f| !f.folder.is_empty()).map(|f| f.folder.clone()) {
            extras = extras.child(div().text_size(px(size::DETAIL)).text_color(faint).truncate().child(folder));
            any_extra = true;
        }
        if !diffs.is_empty() {
            let (mut a, mut r) = (0, 0);
            for d in &diffs {
                let (x, y) = DiffBuilder::stats(d);
                a += x;
                r += y;
            }
            extras = extras.child(stats_text(a, r, added, removed));
            any_extra = true;
        }
        let tooltip = file.as_ref().map(|f| f.path.clone());
        let open = self.row_open(row);
        let click = self.toggle_listener(row, cx);
        let line = work_line(("tool", ix).into(), symbol, icon_color, &runs, any_extra.then(|| extras.into_any_element()), Some(open), tooltip.as_deref(), cx, Some(click));
        let mut col = v_flex().child(line);
        if open {
            if !diffs.is_empty() {
                col = col.child(self.diff_box(ix, row, &diffs, tool.root.as_deref(), cx));
            } else {
                col = col.child(tool_body_box(ix, tool, self.summary.is_remote(), cx));
            }
        }
        row_wrap(row, col)
    }

    /// `resolvedApprovalText`: "Allowed  Write release.sh", a quiet work line
    /// that opens to what was asked when its tool has no row of its own.
    pub(crate) fn settled_approval_row(&mut self, ix: usize, row: &Row, item: &ApprovalItem, cx: &mut Context<Self>) -> AnyElement {
        let resolution = item.resolution.as_ref().expect("a settled approval");
        let (faint, quiet) = (style::faint_text(cx), style::quiet_text(cx));
        let mut runs: Runs = vec![(ApprovalText::decision(resolution.decision).to_owned(), faint)];
        if resolution.decision != ApprovalDecision::Expired {
            runs.push((format!("  {}", ApprovalText::target(&item.request)), quiet));
        }
        let (symbol, icon_color) = match resolution.decision {
            ApprovalDecision::Deny => ("xmark.circle", style::failed_text(cx)),
            ApprovalDecision::Expired => ("clock", style::faint_icon(cx)),
            _ => ("checkmark.circle", style::faint_icon(cx)),
        };
        let can_open = item.opens();
        let open = can_open && self.row_open(row);
        let click = can_open.then(|| self.toggle_listener(row, cx));
        let tooltip = ToolPresentation::short_title(&item.request.title, 400);
        let line = work_line(("approved", ix).into(), symbol, icon_color, &runs, None, can_open.then_some(open), Some(&tooltip), cx, click);
        let mut col = v_flex().child(line);
        if open {
            let body = super::cards::approval_payload(ix, 0, &item.request, item.root.as_deref(), cx);
            col = col.child(div().ml(px(TITLE_INDENT)).mt(px(2.)).mb(px(8.)).p(px(BOX_PAD)).rounded(px(8.)).bg(style::code_background(cx)).border_1().border_color(style::card_border(cx)).child(body));
        }
        row_wrap(row, col)
    }

    /// `secretResolvedText`: the label, then how it ended.
    pub(crate) fn settled_secret_row(&mut self, ix: usize, row: &Row, item: &SecretItem, cx: &mut Context<Self>) -> AnyElement {
        let resolved = item.resolved_text().unwrap_or_default();
        let ending = resolved.get(item.request.label.len()..).unwrap_or("").to_owned();
        let runs: Runs = vec![(item.request.label.clone(), style::quiet_text(cx)), (ending, style::faint_text(cx))];
        let symbol = match item.resolution.as_ref().map(|r| r.decision) {
            Some(ApprovalDecision::Answered) => "lock",
            Some(ApprovalDecision::Expired) => "clock",
            _ => "xmark.circle",
        };
        let tooltip = if item.request.reason.is_empty() { item.request.label.clone() } else { format!("{}: {}", item.request.label, item.request.reason) };
        let line = work_line(("secret-done", ix).into(), symbol, style::faint_icon(cx), &runs, None, None, Some(&tooltip), cx, None);
        row_wrap(row, div().child(line))
    }

    /// A system note folded into an open group reads as one of its lines.
    pub(crate) fn grouped_note_row(&mut self, ix: usize, row: &Row, note: &str, cx: &mut Context<Self>) -> AnyElement {
        let first = note.split('\n').find(|l| !l.is_empty()).unwrap_or(note);
        let line = work_line(("note-line", ix).into(), "info.circle", style::faint_icon(cx), &vec![(first.to_owned(), style::quiet_text(cx))], None, None, None, cx, None);
        row_wrap(row, div().child(line))
    }

    /// The bear and elapsed time that say a turn is running. It counts by
    /// itself once a second while it's on screen.
    pub(crate) fn working_row(&mut self, started_at: Option<f64>, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_ticking(started_at.is_some(), cx);
        let quiet = style::quiet_text(cx);
        let text = match started_at {
            Some(started) => {
                let elapsed = (crate::now_ms() as f64 / 1000. - started).max(1.);
                format!("Bear with me · {}", threadbear_core::reducer::duration(elapsed * 1000.))
            }
            None => "Bear with me".to_owned(),
        };
        let line = h_flex()
            .h(px(WORKING_HEIGHT))
            .items_center()
            .text_size(px(size::WORK))
            .text_color(quiet)
            .child(div().flex_none().w(px(TITLE_INDENT)).child(icon("pawprint", 15., quiet)))
            .child(div().truncate().child(text));
        h_flex().w_full().px(px(crate::style::SIDE_INSET)).justify_center().child(div().w_full().min_w_0().max_w(px(crate::style::MAX_CONTENT_WIDTH)).child(line)).into_any_element()
    }

    /// `BackgroundWork.lineWithDetail`: the bear sits, nothing ticks, and the
    /// words say what's running.
    pub(crate) fn background_row(&mut self, status: &BackgroundStatus, cx: &mut Context<Self>) -> AnyElement {
        let quiet = style::quiet_text(cx);
        let tip_text = status.tasks.iter().map(|t| t.description.clone()).filter(|d| !d.is_empty()).collect::<Vec<_>>().join("\n");
        let line = h_flex()
            .id("background-line")
            .h(px(WORKING_HEIGHT))
            .items_center()
            .text_size(px(size::WORK))
            .text_color(quiet)
            .tooltip(tip(&tip_text))
            .child(div().flex_none().w(px(TITLE_INDENT)).child(icon("pawprint", 15., quiet)))
            .child(div().truncate().child(BackgroundWork::line_with_detail(&status.tasks)));
        h_flex().w_full().px(px(crate::style::SIDE_INSET)).justify_center().child(div().w_full().min_w_0().max_w(px(crate::style::MAX_CONTENT_WIDTH)).child(line)).into_any_element()
    }

    /// `GapRowCell`: a thin rule across the column with the wait in the middle.
    pub(crate) fn gap_row(&mut self, gap: &TurnGap, cx: &mut Context<Self>) -> AnyElement {
        let (hairline, faint) = (style::hairline(cx), style::faint_text(cx));
        let rule = || div().flex_1().h(px(1.)).bg(hairline);
        let tip_text = gap_tip(gap);
        let line = h_flex()
            .h(px(GAP_HEIGHT))
            .items_center()
            .gap(px(12.))
            .child(rule())
            .child(div().id("gap-label").text_size(px(size::SMALL)).text_color(faint).flex_none().tooltip(tip(&tip_text)).child(gap.label()))
            .child(rule());
        h_flex().w_full().px(px(crate::style::SIDE_INSET)).justify_center().child(div().w_full().min_w_0().max_w(px(crate::style::MAX_CONTENT_WIDTH)).child(line)).into_any_element()
    }
}

/// `GapRowCell.tip`: everything that finished, when the line only had room
/// for the first.
fn gap_tip(gap: &TurnGap) -> String {
    let names: Vec<String> = gap.woke_by.iter().map(|t| t.description.clone()).filter(|n| !n.is_empty()).collect();
    if names.is_empty() {
        "Waited between these turns".to_owned()
    } else {
        format!("Finished: {}", names.join(", "))
    }
}

/// `statsText`: "+12 −3" in the stats font, for a diff row and a changes card.
pub(crate) fn stats_text(added: i64, removed: i64, added_color: Hsla, removed_color: Hsla) -> Div {
    h_flex().flex_none().text_size(px(size::DETAIL)).child(div().text_color(added_color).child(format!("+{added}"))).child(div().text_color(removed_color).ml(px(4.)).child(format!("−{removed}")))
}

/// `toolBody` in its code box.
fn tool_body_box(ix: usize, tool: &ToolItem, remote: bool, cx: &App) -> Div {
    div().ml(px(TITLE_INDENT)).mt(px(2.)).mb(px(8.)).p(px(BOX_PAD)).rounded(px(8.)).bg(style::code_background(cx)).border_1().border_color(style::card_border(cx)).child(tool_body(ix, tool, remote, cx))
}

/// One monospaced run in the output font.
pub(crate) fn mono_text(id: impl Into<ElementId>, words: impl Into<SharedString>, color: Hsla, cx: &App) -> Stateful<Div> {
    mono_block(id, cx).text_color(color).child(SelectableText::new("t", words.into()))
}

pub(crate) fn mono_block(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div().id(id.into()).font_family(cx.theme().mono_font_family.clone()).text_size(px(size::CODE)).line_height(px(size::CODE_LINE)).min_w_0()
}

/// What an expanded tool shows: a command and its output, a file with line
/// numbers, a search and its matches, or the input as fields.
fn tool_body(ix: usize, tool: &ToolItem, remote: bool, cx: &App) -> AnyElement {
    let (label, quiet, faint, failed) = (style::label_text(cx), style::quiet_text(cx), style::faint_text(cx), style::failed_text(cx));
    // A generated image's revised prompt is prose: the reading font, wrapped
    // at words, in the output font it broke mid-word.
    if tool.tool_kind() == ToolKind::Image {
        if let Some(text) = tool.result.as_ref().map(|r| r.text.clone()).filter(|t| !t.is_empty()) {
            return div().text_size(px(size::WORK)).line_height(px(21.)).text_color(if tool.failed() { failed } else { quiet }).child(SelectableText::new(("tool-prose", ix), text)).into_any_element();
        }
    }
    let mut col = v_flex().min_w_0();
    let mut has_output = false;
    match ToolPresentation::detail(tool, 400) {
        Detail::Command { command, output } => {
            let shown: String = clamp_lines(&command, 40);
            col = col.child(h_flex().items_start().child(mono_text(("dollar", ix), "$ ", faint, cx).flex_none()).child(mono_text(("cmd", ix), shown, label, cx).flex_1()));
            if let Some(block) = output_block(ix, tool, &output, cx) {
                has_output = true;
                col = col.child(block);
            }
        }
        Detail::File { lines, .. } => {
            if !lines.is_empty() {
                has_output = true;
                col = col.child(numbered(ix, &lines, cx));
            }
        }
        Detail::Search { query, scope, output } => {
            col = col.child(h_flex().flex_wrap().child(mono_text(("query", ix), query, label, cx)).when_some(scope, |d, scope| d.child(mono_text(("scope", ix), format!("  in {scope}"), faint, cx))));
            if let Some(block) = output_block(ix, tool, &output, cx) {
                has_output = true;
                col = col.child(block);
            }
        }
        Detail::Fields { fields, output } => {
            if !fields.is_empty() {
                col = col.child(fields_text(ix, &fields, cx));
            }
            if let Some(block) = output_block(ix, tool, &output, cx) {
                has_output = true;
                col = col.child(block);
            }
        }
    }
    let truncated = tool.result.as_ref().is_some_and(|r| r.truncated);
    if truncated {
        let note = div().mt(px(6.)).text_size(px(size::SMALL)).line_height(px(17.)).text_color(faint);
        let blob = tool.result.as_ref().and_then(|r| r.blob_path.clone()).filter(|p| !p.is_empty());
        col = col.child(match blob {
            Some(path) => {
                // The runner names the file, so it goes through the open gate
                // like any other path: another Mac's isn't on this one.
                let origin = if remote { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac };
                note.child(
                    h_flex().child("Output shortened · ").child(
                        div().id(("show-all", ix)).cursor_pointer().text_color(style::link_text(cx)).tooltip(tip("The whole output, in a file")).on_click(move |_, _, cx| crate::open_gate::open_file(&path, origin, cx)).child("Show all"),
                    ),
                )
            }
            None => note.child("Output shortened"),
        });
    } else if !has_output {
        col = col.child(mono_text(("no-output", ix), if tool.is_finished() { "No output" } else { "Running…" }, faint, cx));
    }
    col.into_any_element()
}

/// One run of output lines, in order. A failed tool's output is ordinary text
/// except the lines that say what went wrong: in red the whole of it read as
/// removed code and was the loudest thing in the window.
fn output_block(ix: usize, tool: &ToolItem, text: &str, cx: &App) -> Option<Div> {
    if text.is_empty() {
        return None;
    }
    let (quiet, failed) = (style::quiet_text(cx), style::failed_text(cx));
    let mut runs: Vec<(Vec<&str>, bool)> = vec![];
    for line in text.split('\n') {
        let is_error = tool.failed() && ToolPresentation::is_error_line(line);
        match runs.last_mut() {
            Some((lines, err)) if *err == is_error => lines.push(line),
            _ => runs.push((vec![line], is_error)),
        }
    }
    let mut block = v_flex().mt(px(8.));
    for (i, (lines, err)) in runs.into_iter().enumerate() {
        block = block.child(mono_text(("output", ix * 100 + i), lines.join("\n"), if err { failed } else { quiet }, cx));
    }
    Some(block)
}

/// `fieldsText`: "key:  value" with the values in one column, long values
/// wrapping under their value.
pub(crate) fn fields_text(ix: usize, fields: &[Field], cx: &App) -> Div {
    let key_width = ToolPresentation::key_width(fields, 16);
    let column = (key_width + 2) as f32 * DIGIT_WIDTH;
    let (faint, label) = (style::faint_text(cx), style::label_text(cx));
    let mut col = v_flex();
    for (i, field) in fields.iter().enumerate() {
        let label_text = format!("{}:", field.key);
        let runs_over = label_text.chars().count() > key_width;
        col = col.child(
            h_flex()
                .items_start()
                .child(mono_text(("fkey", ix * 100 + i), if runs_over { format!("{label_text} ") } else { label_text }, faint, cx).flex_none().when(!runs_over, |d| d.w(px(column))))
                .child(mono_text(("fval", ix * 100 + i), field.value.clone(), label, cx).flex_1()),
        );
    }
    col
}

/// Monospaced digit width at the code size: columns of numbers and field keys
/// line up by count of characters.
pub(crate) const DIGIT_WIDTH: f32 = 7.83;

/// `numbered`: file lines with their numbers right-aligned in a gutter.
fn numbered(ix: usize, lines: &[NumberedLine], cx: &App) -> Div {
    let digits = lines.iter().filter_map(|l| l.number).max().map(|n| n.to_string().len()).unwrap_or(0).max(3);
    let gutter = digits as f32 * DIGIT_WIDTH + 2.;
    let (faint, quiet) = (style::faint_text(cx), style::quiet_text(cx));
    let mut col = v_flex();
    for (i, line) in lines.iter().enumerate() {
        let number = line.number.map(|n| n.to_string()).unwrap_or_default();
        col = col.child(
            h_flex()
                .items_start()
                .gap(px(14.))
                .child(mono_block(("linenum", ix * 10000 + i), cx).flex_none().w(px(gutter)).text_right().text_color(faint).child(number))
                .child(mono_text(("line", ix * 10000 + i), line.text.replace('\t', "    "), quiet, cx).flex_1()),
        );
    }
    col
}

/// `clampLines`.
fn clamp_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() <= count {
        text.to_owned()
    } else {
        format!("{}\n…", lines[..count].join("\n"))
    }
}

