// The layout numbers from RowRenderer.swift's RowMetrics (and a few of
// Theme.swift's), one place for the transcript's spacing. All at 100% zoom:
// zoom isn't ported yet (foundation.zoom). Colours are in style.rs.
pub const ROW_TOP: f32 = 6.;
pub const ROW_BOTTOM: f32 = 6.;
/// Space above a user message, which starts a turn: the biggest gap in the
/// transcript, so turns read apart.
pub const USER_TOP: f32 = 26.;
/// Space under the bubble before the turn's work.
pub const USER_BOTTOM: f32 = 22.;
pub const WAITING_NOTE_GAP: f32 = 4.;
pub const WAITING_NOTE_HEIGHT: f32 = 16.;
/// One line of work: a group summary, a tool call, a thinking block.
pub const WORK_HEIGHT: f32 = 32.;
pub const WORKING_HEIGHT: f32 = 34.;
pub const BUBBLE_PAD_H: f32 = 14.;
pub const BUBBLE_PAD_V: f32 = 10.;
pub const BUBBLE_RADIUS: f32 = 18.;
/// The widest a user bubble gets, as a share of the column.
pub const BUBBLE_SHARE: f32 = 0.8;
pub const BOX_PAD: f32 = 10.;
pub const DIFF_PAD_V: f32 = 6.;
/// Where a work line's text starts, after its icon.
pub const TITLE_INDENT: f32 = 22.;
/// How far an open group's items sit in from its summary line.
pub const GROUP_INDENT: f32 = 18.;
pub const CARD_PAD: f32 = 14.;
pub const CARD_RADIUS: f32 = 12.;
/// The bar down the left of a card that needs Mike.
pub const ACCENT_BAR: f32 = 3.;
pub const BUTTON_HEIGHT: f32 = 28.;
pub const OPTION_HEIGHT: f32 = 24.;
pub const OPTION_DESCRIPTION: f32 = 17.;
/// The question card's "Something else" field and the secret card's.
pub const OTHER_FIELD_HEIGHT: f32 = 30.;
pub const ANSWERED_GAP: f32 = 6.;
pub const ANSWER_HEIGHT: f32 = 20.;
pub const USER_THUMB_HEIGHT: f32 = 120.;
pub const GRID_MAX_HEIGHT: f32 = 240.;
pub const GRID_MAX_WIDTH: f32 = 320.;
pub const IMAGE_GAP: f32 = 8.;
/// A picture the agent made, shown large (ImageLayout.generatedMaxSide).
pub const GENERATED_MAX_SIDE: f32 = 420.;
/// The most lines an opened diff shows once Mike asks for all of it.
pub const MAX_DIFF_LINES: usize = 1500;
pub const FOOTER_HEIGHT: f32 = 27.;
/// A message the runner sent by itself: a centred note under the restart's own
/// note, with room before the work it starts.
pub const RUNNER_TOP: f32 = 4.;
pub const RUNNER_BOTTOM: f32 = 18.;
pub const CHANGES_HEADER: f32 = 42.;
pub const CHANGES_FILE: f32 = 34.;
pub const GAP_HEIGHT: f32 = 34.;

/// A code block's box reaches this far past its text (MarkdownRenderer.codePad).
pub const CODE_PAD: f32 = 12.;

/// The room left for the bottom fade under the last row, and the fades over
/// the list's top and bottom (TranscriptController).
pub const FADE_HEIGHT: f32 = 36.;
pub const TOP_FADE_HEIGHT: f32 = 14.;
pub const LIST_TOP_INSET: f32 = 10.;

/// `StackedApproval` in RowRenderer.swift.
pub mod stacked {
    pub const GAP: f32 = 22.;
    pub const TOGGLE_HEIGHT: f32 = 24.;
    pub const TOGGLE_WIDTH: f32 = 34.;
}
