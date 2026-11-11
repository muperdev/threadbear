// How a thread's events become transcript rows: a port of the Swift app's
// ThreadbearCore/Transcript (and what it depends on in Protocol/). The Swift
// is the definition of right: desktop/core/tests/golden holds the rows the
// Swift reducer makes for every mock-runner fixture, and both the Swift
// tests (app/Tests/ThreadbearCoreTests/GoldenTranscriptTests.swift) and the
// Rust ones (tests/golden.rs) have to match them.
//
// Each file below says which Swift type or function it ports. Events stay as
// serde_json::Value right up to `Event::decode`, whose lenient decoders
// mirror the Swift ones field for field (docs/protocol.md: "keep both sides
// tolerant of unknown fields and event types").
pub mod card_text;
pub mod diff;
pub mod event;
pub mod feed;
pub mod format;
pub mod groups;
pub mod model;
pub mod pending;
pub mod row;
pub mod row_debug;
pub mod text;
pub mod tool_presentation;
pub mod turn_gap;
pub mod work_links;

pub use card_text::{ApprovalText, PlanCardText, PlanSummary};
pub use diff::{DiffBuilder, DiffDisplayLine, DiffLineKind};
pub use event::*;
pub use feed::{FeedAction, ThreadFeed};
pub use format::{cost_text, duration, token_line, TurnTokens};
pub use groups::{BackgroundStatus, BackgroundWork, ChangedFile, ChangedFiles, DiffPreview, WorkGroup, WorkSummary, WorkingStatus};
pub use model::{PrependResult, TranscriptChange, TranscriptModel};
pub use pending::PendingMessage;
pub use row::{AgentHeader, ApprovalItem, AskNote, AsksCard, Pending, PlanCard, PlanCardState, QuestionItem, Row, RowContent, RowKind, ScheduledHeader, SecretItem, TextItem, ToolItem, UserMessage};
pub use row_debug::RowDebug;
pub use tool_presentation::{Detail, Field, FileTitle, NumberedLine, PathDisplay, ToolPresentation};
pub use turn_gap::TurnGap;
pub use work_links::WorkLinks;
