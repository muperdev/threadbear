// The window's shell: BearDock's panels with the activity bar down the left and
// the rail of the thread's panels down the right (RootView in WindowDock.swift),
// the title bar handed over to the panels' bars, and the app's commands, their
// keys, the menu bar and back and forward (ThreadbearApp.swift, AppShortcuts,
// Navigation.swift).
use crate::app::Threadbear;
use crate::icons;
use crate::settings::WindowFrame;
use crate::style::{self, px};
use gpui_kit::component::{v_flex, ActiveTheme as _, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use threadbear_core::shortcuts::{ShortcutBindings, ShortcutCommand};
use threadbear_core::zoom;
use threadbear_dock::{DockContent, DockDrop, DockEdge, DockGroup, DockLayout, DockNode, DockOptions, DockSplit, SplitAxis};

// The panels' ids, the same as the Swift app's so a saved layout carries over
// (WindowDock, PaneSection, BuiltinPanels, AgentBrowser.panelId).
pub(crate) const THREADS: &str = "threadbear.threads";
pub(crate) const MAIN: &str = "threadbear.thread";
pub(crate) const HIGHLIGHTS: &str = "threadbear.highlights";
pub(crate) const FILES: &str = "threadbear.files";
pub(crate) const PATCHES: &str = "threadbear.patches";
pub(crate) const USAGE: &str = "threadbear.usage";
pub(crate) const TERMINAL: &str = "threadbear.terminal";

/// How often the settings file is written during a burst of changes.
const SAVE_THROTTLE_MS: i64 = 300;

/// `SidebarMode`: which view the sidebar shows, from the activity bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SidebarMode {
    #[default]
    Inbox,
    Scheduled,
    Projects,
    Librarian,
}

impl SidebarMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            SidebarMode::Inbox => "inbox",
            SidebarMode::Scheduled => "scheduled",
            SidebarMode::Projects => "projects",
            SidebarMode::Librarian => "librarian",
        }
    }
}

/// `NavigationPlace`: where the window is, for back and forward. A project's
/// page and the sub-agent view come with those features.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NavPlace {
    pub(crate) mode: SidebarMode,
    pub(crate) thread: Option<String>,
}

/// "Archived" and the thread's name, floating over the main panel with Undo
/// (`ArchivedNote`).
#[derive(Clone, Debug)]
pub(crate) struct ArchivedNote {
    pub(crate) id: u64,
    pub(crate) thread: String,
    pub(crate) title: String,
    pub(crate) was_open: bool,
}

/// Next Inbox Thread with nowhere to go (`InboxEndNote`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct InboxEndNote {
    pub(crate) id: u64,
    pub(crate) empty: bool,
}

/// Swift's `WindowDock.defaultLayout`: the sidebar without a title, the
/// thread, and Highlights, side by side.
pub(crate) fn default_layout() -> DockLayout {
    let group = |id: &str, panel: &str| DockNode::Group(DockGroup::new(id, vec![panel.to_string()], None));
    let root = DockNode::Split(DockSplit::new("window", SplitAxis::Horizontal, vec![group("sidebar", THREADS), group("main", MAIN), group("pane", HIGHLIGHTS)], Some(vec![0.29, 0.47, 0.24])));
    DockLayout::new(root, [THREADS.to_string()], vec![])
}

/// The saved layout, or the default; without the main panel there's nowhere
/// to type, so one saved that way gets it back (`makeController`).
pub(crate) fn make_layout(saved: Option<&Value>) -> DockLayout {
    let mut layout = saved.and_then(|v| serde_json::from_value::<DockLayout>(v.clone()).ok()).unwrap_or_else(default_layout);
    if !layout.contains(MAIN) {
        layout.add(MAIN, DockDrop::window(DockEdge::Right));
    }
    layout
}

/// The `DockOptions` for this window, as `RootView.options` makes them: the
/// bars are the window's title bar, with room for the traffic lights in the
/// top-left panel's bar (the activity bar has the first 44 points of them).
pub(crate) fn dock_options(cx: &App) -> DockOptions {
    let traffic_lights = if cfg!(target_os = "macos") { (78. - f32::from(px(crate::rails::BAR_WIDTH))).max(0.) } else { 0. };
    DockOptions {
        header_height: f32::from(px(crate::header::HEADER_HEIGHT)),
        title_size: f32::from(px(style::size::LABEL)),
        traffic_lights,
        corner_moves_window: true,
        bars_move_window: true,
        accent: Some(style::control_accent(cx)),
        window_content: None,
        prepare_window: None,
    }
}

/// What each panel shows and is called: the seam BearDock draws through
/// (Swift's `DockContent` in `WindowDock`).
pub(crate) struct Panels {
    pub(crate) app: WeakEntity<Threadbear>,
}

impl DockContent for Panels {
    fn title(&self, panel: &str, cx: &App) -> SharedString {
        self.app.upgrade().map(|a| SharedString::from(a.read(cx).panel_title(panel))).unwrap_or_default()
    }

    fn icon(&self, panel: &str, _window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let symbol = panel_symbol(panel)?;
        let color = cx.theme().muted_foreground;
        Some(icons::symbol(symbol).with_size(px(13.)).text_color(color).into_any_element())
    }

    fn view(&self, panel: &str, ghost: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        match self.app.upgrade() {
            Some(app) => app.update(cx, |t, cx| t.panel_view(panel, ghost, window, cx)),
            None => div().into_any_element(),
        }
    }

    fn header(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        if panel != MAIN {
            return None;
        }
        let app = self.app.upgrade()?;
        Some(app.update(cx, |t, cx| t.render_header_title(window, cx)))
    }

    fn accessory(&self, panel: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        if panel != MAIN {
            return None;
        }
        let app = self.app.upgrade()?;
        Some(app.update(cx, |t, cx| t.render_header_buttons(window, cx)))
    }

    fn closable(&self, panel: &str, _cx: &App) -> bool {
        panel != MAIN
    }

    fn available(&self, panel: &str, cx: &App) -> bool {
        let Some(app) = self.app.upgrade() else { return true };
        app.read(cx).panel_available(panel)
    }
}

/// `WindowDock.symbol(of:)`: the icon on a panel's tab.
fn panel_symbol(panel: &str) -> Option<&'static str> {
    Some(match panel {
        THREADS => "bubble.left.and.bubble.right",
        MAIN => "text.alignleft",
        USAGE => "gauge.with.dots.needle.33percent",
        TERMINAL => "terminal",
        HIGHLIGHTS => "pin",
        FILES => "folder",
        PATCHES => "square.dashed",
        _ => return None,
    })
}

// The commands: one per menu item or key, each with the action GPUI dispatches.
// The macro keeps the action, the Swift ShortcutCommand and the handler in one
// line per command.
actions!(
    threadbear,
    [
        NewThread,
        ArchiveThread,
        PickHibernateTime,
        WakeWhenPrompt,
        StopTurn,
        CopyTranscript,
        GoBack,
        GoForward,
        NextInboxThread,
        ShowInbox,
        ShowScheduled,
        ShowProjects,
        ShowLibrarian,
        ShowHibernating,
        ShowArchived,
        OpenSettings,
        ZoomIn,
        ZoomOut,
        ActualSize,
        ToggleThreads,
        TogglePanels,
        ToggleTerminal,
        ResetPanelLayout,
        ToggleFrameRate,
        AboutApp,
        TakeTheTour,
        FinishSettingUp,
        HibernateHour,
        HibernateEvening,
        HibernateTomorrow,
        HibernateMonday,
    ]
);

macro_rules! commands {
    ($(($variant:ident, $action:ident, $shortcut:expr)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum Command { $($variant),* }

        impl Command {
            pub(crate) const ALL: &'static [Command] = &[$(Command::$variant),*];

            /// The action GPUI dispatches for this command, for the menu bar.
            fn action(self) -> Box<dyn Action> {
                match self { $(Command::$variant => Box::new($action)),* }
            }

            fn binding(self, chord: &str) -> KeyBinding {
                let chord = leak(chord);
                // No context: a binding with no context wins at any depth, so
                // these keys always do their menu command (as a Mac app's
                // menu key equivalents do), even in the composer, where a
                // text field's own binding for ⌘[ or ⌥⌘↓ would otherwise take
                // the key first.
                match self { $(Command::$variant => KeyBinding::new(chord, $action, None)),* }
            }

            /// The shortcut this command's key comes from, if it has one.
            pub(crate) fn shortcut(self) -> Option<ShortcutCommand> {
                match self { $(Command::$variant => $shortcut),* }
            }
        }

        fn register_commands(root: Stateful<Div>, cx: &mut Context<Threadbear>) -> Stateful<Div> {
            root $(.on_action(cx.listener(|this, _: &$action, window, cx| this.run_command(Command::$variant, window, cx))))*
        }
    };
}

commands!(
    (NewThread, NewThread, Some(ShortcutCommand::NewThread)),
    (Archive, ArchiveThread, Some(ShortcutCommand::Archive)),
    (Hibernate, PickHibernateTime, Some(ShortcutCommand::Hibernate)),
    (WakeWhen, WakeWhenPrompt, None),
    (Stop, StopTurn, Some(ShortcutCommand::Stop)),
    (CopyTranscript, CopyTranscript, Some(ShortcutCommand::CopyTranscript)),
    (Back, GoBack, Some(ShortcutCommand::Back)),
    (Forward, GoForward, Some(ShortcutCommand::Forward)),
    (NextInboxThread, NextInboxThread, Some(ShortcutCommand::NextInboxThread)),
    (ShowInbox, ShowInbox, Some(ShortcutCommand::ShowInbox)),
    (ShowScheduled, ShowScheduled, Some(ShortcutCommand::ShowScheduled)),
    (ShowProjects, ShowProjects, Some(ShortcutCommand::ShowProjects)),
    (ShowLibrarian, ShowLibrarian, Some(ShortcutCommand::ShowLibrarian)),
    (ShowHibernating, ShowHibernating, Some(ShortcutCommand::ShowHibernating)),
    (ShowArchived, ShowArchived, Some(ShortcutCommand::ShowArchived)),
    (Settings, OpenSettings, Some(ShortcutCommand::Settings)),
    (ZoomIn, ZoomIn, Some(ShortcutCommand::ZoomIn)),
    (ZoomOut, ZoomOut, Some(ShortcutCommand::ZoomOut)),
    (ActualSize, ActualSize, Some(ShortcutCommand::ActualSize)),
    (ToggleThreads, ToggleThreads, Some(ShortcutCommand::ToggleThreads)),
    (TogglePanels, TogglePanels, Some(ShortcutCommand::TogglePanels)),
    (ToggleTerminal, ToggleTerminal, Some(ShortcutCommand::ToggleTerminal)),
    (ResetPanelLayout, ResetPanelLayout, Some(ShortcutCommand::ResetPanelLayout)),
    (ToggleFrameRate, ToggleFrameRate, None),
    (About, AboutApp, None),
    (TakeTheTour, TakeTheTour, None),
    (FinishSettingUp, FinishSettingUp, None),
    (HibernateHour, HibernateHour, None),
    (HibernateEvening, HibernateEvening, None),
    (HibernateTomorrow, HibernateTomorrow, None),
    (HibernateMonday, HibernateMonday, None),
);

impl Threadbear {
    pub(crate) fn panel_title(&self, panel: &str) -> String {
        match panel {
            THREADS => "Threads".into(),
            MAIN => self.main_panel_title(),
            USAGE => "Usage".into(),
            TERMINAL => "Terminal".into(),
            HIGHLIGHTS => "Highlights".into(),
            FILES => "Files".into(),
            PATCHES => "Patches".into(),
            _ => self.open_thread().and_then(|t| t.panels.iter().flatten().find(|p| p["id"].as_str() == Some(panel))).and_then(|p| p["title"].as_str()).map(String::from).unwrap_or_else(|| panel.to_string()),
        }
    }

    /// `WindowDock.title(of: .main)` and the window's own title.
    pub(crate) fn main_panel_title(&self) -> String {
        match self.mode {
            SidebarMode::Projects => "Projects".into(),
            SidebarMode::Librarian => "Librarian".into(),
            SidebarMode::Scheduled => "Scheduled".into(),
            SidebarMode::Inbox => match self.open_thread() {
                Some(t) if t.title.is_empty() => "Untitled".into(),
                Some(t) => t.title.clone(),
                None => "New thread".into(),
            },
        }
    }

    pub(crate) fn open_thread(&self) -> Option<&threadbear_core::inbox::ThreadSummary> {
        self.selected.as_ref().and_then(|id| self.threads.get(id))
    }

    /// `WindowDock.section(_:)` != nil: a pane section's panel, not the
    /// sidebar, the main panel or one of the activity bar's panels.
    pub(crate) fn is_pane_section(panel: &str) -> bool {
        !matches!(panel, THREADS | MAIN | USAGE | TERMINAL)
    }

    /// `available` in `RootView.content`: Highlights, Files and a thread's own
    /// panels for a thread that has them; the rest always.
    pub(crate) fn panel_available(&self, panel: &str) -> bool {
        if !Self::is_pane_section(panel) {
            return true;
        }
        let Some(thread) = self.open_thread() else { return true };
        match panel {
            HIGHLIGHTS | FILES => true,
            PATCHES => thread.patches.as_ref().is_some_and(|p| !p.is_empty()),
            other => thread.panels.iter().flatten().any(|p| p["id"].as_str() == Some(other)),
        }
    }

    fn is_panel_showing(&self, panel: &str, cx: &App) -> bool {
        self.dock.read(cx).controller.is_showing(panel)
    }

    /// The pane sections open in the dock, in layout order (`openPanes`).
    fn open_panes(&self, cx: &App) -> Vec<String> {
        self.dock.read(cx).layout().panels().into_iter().filter(|p| Self::is_pane_section(p)).collect()
    }

    /// Shows a section's panel, beside another pane section if one's open,
    /// else down the right of the window (`showPane`).
    pub(crate) fn show_pane(&mut self, panel: &str, cx: &mut Context<Self>) {
        let near = self.open_panes(cx);
        let near_refs: Vec<&str> = near.iter().map(String::as_str).collect();
        self.dock.update(cx, |m, cx| m.show(panel, &near_refs, DockDrop::window(DockEdge::Right), cx));
    }

    /// A rail button, VS Code's way: closes the section's panel if it's
    /// showing, else brings it up (`clickRail`).
    pub(crate) fn click_rail(&mut self, panel: &str, cx: &mut Context<Self>) {
        if self.is_panel_showing(panel, cx) {
            self.dock.update(cx, |m, cx| m.close(panel, cx));
        } else {
            self.show_pane(panel, cx);
        }
    }

    /// One of the activity bar's panels (Usage): as a tab beside another of
    /// them if one's open, else under the threads list (`setShownUnderThreads`).
    pub(crate) fn toggle_under_threads(&mut self, panel: &str, cx: &mut Context<Self>) {
        if self.is_panel_showing(panel, cx) {
            self.dock.update(cx, |m, cx| m.close(panel, cx));
            return;
        }
        let under = self.dock.read(cx).layout().group_containing(THREADS).map(|g| DockDrop::edge(g.id, DockEdge::Bottom)).unwrap_or(DockDrop::window(DockEdge::Left));
        self.dock.update(cx, |m, cx| m.show(panel, &[], under, cx));
    }

    fn toggle_terminal(&mut self, cx: &mut Context<Self>) {
        if self.is_panel_showing(TERMINAL, cx) {
            self.dock.update(cx, |m, cx| m.close(TERMINAL, cx));
            return;
        }
        let drop = self.dock.read(cx).layout().group_containing(MAIN).map(|g| DockDrop::edge(g.id, DockEdge::Bottom)).unwrap_or(DockDrop::window(DockEdge::Bottom));
        self.dock.update(cx, |m, cx| m.show(TERMINAL, &[], drop, cx));
    }

    fn toggle_threads_panel(&mut self, cx: &mut Context<Self>) {
        let contains = self.dock.read(cx).layout().contains(THREADS);
        self.dock.update(cx, |m, cx| {
            if contains {
                m.close(THREADS, cx);
            } else {
                m.open(THREADS, DockDrop::window(DockEdge::Left), cx);
            }
        });
    }

    /// `panelsShown.toggle()`: the pane sections go away, remembered, and
    /// come back as they were (Highlights if none were open).
    fn toggle_panels(&mut self, cx: &mut Context<Self>) {
        let open = self.open_panes(cx);
        if open.is_empty() {
            let panes = if self.hidden_panes.is_empty() { vec![HIGHLIGHTS.to_string()] } else { std::mem::take(&mut self.hidden_panes) };
            for panel in &panes {
                self.show_pane(panel, cx);
            }
        } else {
            self.dock.update(cx, |m, cx| {
                for panel in &open {
                    m.close(panel, cx);
                }
            });
            self.hidden_panes = open;
        }
    }

    pub(crate) fn reset_panel_layout(&mut self, cx: &mut Context<Self>) {
        self.dock.update(cx, |m, cx| m.set_layout(default_layout(), cx));
    }

    /// Saves the panel layout (`dock.onChange`), a moment after it last moved:
    /// a divider drag changes it every frame.
    pub(crate) fn dock_layout_changed(&mut self, layout: DockLayout, cx: &mut Context<Self>) {
        self.settings.saved.dock_layout = serde_json::to_value(&layout).ok();
        self.save_settings_soon(cx);
        cx.notify();
    }

    /// Writes the settings file now, or just after a burst of changes (a
    /// divider drag changes the layout every frame): a change on its own is
    /// on disk straight away, so nothing's lost if the app is killed next.
    pub(crate) fn save_settings_soon(&mut self, cx: &mut Context<Self>) {
        let now = crate::now_ms();
        if self.settings_save_task.is_none() && now - self.settings_last_save_ms >= SAVE_THROTTLE_MS {
            self.settings_last_save_ms = now;
            self.settings_dirty = false;
            self.settings.save();
            return;
        }
        self.settings_dirty = true;
        if self.settings_save_task.is_some() {
            return;
        }
        self.settings_save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(SAVE_THROTTLE_MS as u64)).await;
            this.update(cx, |this, _| {
                this.settings_save_task = None;
                this.settings_last_save_ms = crate::now_ms();
                if std::mem::take(&mut this.settings_dirty) {
                    this.settings.save();
                }
            })
            .ok();
        }));
    }

    /// The window's frame, whenever it moves or resizes, unless this is a
    /// scripted run: those frames aren't Mike's (`MainWindowFrame.scripted`).
    pub(crate) fn window_frame_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::env::var_os("THREADBEAR_AUTOMATION").is_some() {
            return;
        }
        let WindowBounds::Windowed(b) = window.window_bounds() else { return };
        self.settings.saved.window_frame = Some(WindowFrame { x: f64::from(b.origin.x), y: f64::from(b.origin.y), width: f64::from(b.size.width), height: f64::from(b.size.height) });
        self.save_settings_soon(cx);
    }

    /// Folds and the Inbox's sort, saved as they change.
    pub(crate) fn sidebar_settings_changed(&mut self, cx: &mut Context<Self>) {
        self.settings.saved.collapsed_sections = Some(self.folded.iter().map(|k| k.to_string()).collect());
        self.settings.set_inbox_order(self.sidebar.order);
        self.save_settings_soon(cx);
    }

    /// Zoom In, Zoom Out, Actual Size (TextZoom): the zoom goes in `style`, so
    /// every `px` in the window scales; the panels' bars and the transcript's
    /// measured rows follow, and it's saved (and `textZoomPercent` is the
    /// setting's name so a Swift import carries it).
    pub(crate) fn set_zoom(&mut self, percent: u32, window: &mut Window, cx: &mut Context<Self>) {
        let percent = zoom::sanitize(Some(percent));
        if percent == style::zoom_percent() {
            return;
        }
        style::set_zoom_percent(percent);
        apply_zoom_to_theme(percent, cx);
        let options = dock_options(cx);
        self.dock_view.update(cx, |d, cx| d.set_options(options, cx));
        if let Some(pane) = &self.pane {
            pane.update(cx, |p, cx| p.remeasure_rows(cx));
        }
        self.settings.set_zoom_percent(percent);
        self.save_settings_soon(cx);
        window.refresh();
        cx.notify();
    }

    fn zoom_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(zoom::zoom_in(style::zoom_percent()), window, cx);
    }

    fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(zoom::zoom_out(style::zoom_percent()), window, cx);
    }

    // Commands.

    pub(crate) fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::NewThread => self.new_thread(cx),
            Command::Archive => self.toggle_archive(window, cx),
            Command::Hibernate => {
                if let Some(id) = self.selected.clone() {
                    self.open_pick_time(id, window, cx);
                }
            }
            Command::WakeWhen => {
                if let Some(id) = self.selected.clone() {
                    self.open_wake_when(id, window, cx);
                }
            }
            Command::Stop => self.stop_open(cx),
            Command::CopyTranscript => {
                if let Some(pane) = &self.pane {
                    pane.update(cx, |p, cx| p.copy_transcript(cx));
                }
            }
            Command::Back => self.go_back(window, cx),
            Command::Forward => self.go_forward(window, cx),
            Command::NextInboxThread => self.next_inbox_thread(window, cx),
            Command::ShowInbox => self.set_mode(SidebarMode::Inbox, cx),
            Command::ShowScheduled => self.set_mode(SidebarMode::Scheduled, cx),
            Command::ShowProjects => self.set_mode(SidebarMode::Projects, cx),
            Command::ShowLibrarian => self.set_mode(SidebarMode::Librarian, cx),
            Command::ShowHibernating => self.show_folded_section("hibernating", cx),
            Command::ShowArchived => self.show_folded_section("archived", cx),
            Command::Settings => window.push_notification("Settings isn't in this app yet.", cx),
            Command::ZoomIn => self.zoom_in(window, cx),
            Command::ZoomOut => self.zoom_out(window, cx),
            Command::ActualSize => self.set_zoom(zoom::DEFAULT_PERCENT, window, cx),
            Command::ToggleThreads => self.toggle_threads_panel(cx),
            Command::TogglePanels => self.toggle_panels(cx),
            Command::ToggleTerminal => self.toggle_terminal(cx),
            Command::ResetPanelLayout => self.reset_panel_layout(cx),
            Command::ToggleFrameRate => self.toggle_frame_rate(window, cx),
            Command::About => window.push_notification("Threadbear (GPUI)", cx),
            Command::TakeTheTour | Command::FinishSettingUp => window.push_notification("That isn't in this app yet.", cx),
            Command::HibernateHour => self.hibernate_open("hour", window, cx),
            Command::HibernateEvening => self.hibernate_open("evening", window, cx),
            Command::HibernateTomorrow => self.hibernate_open("tomorrow", window, cx),
            Command::HibernateMonday => self.hibernate_open("monday", window, cx),
        }
        self.sync_menus(cx);
    }

    /// ⌘E: archives the open thread, or brings it back if it's archived
    /// (`toggleArchive`).
    pub(crate) fn toggle_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        let archived = self.threads.get(&id).is_some_and(|t| t.is_archived());
        if archived {
            self.unarchive_row(&id, cx);
        } else {
            self.archive_row(id, window, cx);
        }
    }

    fn hibernate_open(&mut self, preset: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        let until = threadbear_core::sidebar::snooze_date(preset, &threadbear_core::sidebar::local_now());
        self.snooze_row(id, until, window, cx);
    }

    /// A thread named from outside the list: the Inbox view, with its section
    /// unfolded if it's put away, as a chip in a reply does (`openThread`).
    pub(crate) fn reveal_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.threads.get(id) {
            let section = if t.is_archived() {
                Some("archived")
            } else if t.snoozed(crate::now_ms()) && !t.is_starred() {
                Some("hibernating")
            } else {
                None
            };
            if let Some(section) = section {
                self.folded.remove(section);
                self.sidebar_settings_changed(cx);
            }
        }
        self.mode = SidebarMode::Inbox;
        cx.notify();
    }

    /// Show Hibernating / Show Archived: the inbox view with that section open.
    fn show_folded_section(&mut self, section: &'static str, cx: &mut Context<Self>) {
        self.mode = SidebarMode::Inbox;
        self.folded.remove(section);
        self.sidebar_settings_changed(cx);
        self.note_navigation();
        cx.notify();
    }

    /// `app.sidebarMode = ...` and the pages' `showProjects()` etc.: the view
    /// on show, recorded for back and forward.
    pub(crate) fn set_mode(&mut self, mode: SidebarMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            self.note_navigation();
        }
        cx.notify();
    }

    // Back and forward (Navigation.swift).

    fn place(&self) -> NavPlace {
        NavPlace { mode: self.mode, thread: self.selected.clone() }
    }

    /// `noteNavigation`: one step per place the window has been.
    pub(crate) fn note_navigation(&mut self) {
        let place = self.place();
        self.history.record(place);
    }

    fn can_show(&self, place: &NavPlace) -> bool {
        place.thread.as_ref().is_none_or(|id| self.threads.contains_key(id))
    }

    pub(crate) fn can_go_back(&self) -> bool {
        self.history.can_go_back()
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        self.history.can_go_forward()
    }

    pub(crate) fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let threads = self.threads.keys().cloned().collect::<std::collections::HashSet<_>>();
        if let Some(place) = self.history.back(|p| p.thread.as_ref().is_none_or(|id| threads.contains(id))) {
            self.show_place(place, window, cx);
        }
    }

    pub(crate) fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let threads = self.threads.keys().cloned().collect::<std::collections::HashSet<_>>();
        if let Some(place) = self.history.forward(|p| p.thread.as_ref().is_none_or(|id| threads.contains(id))) {
            self.show_place(place, window, cx);
        }
    }

    /// Puts the window back at `place`; the history already moved, so the step
    /// this records is where it is now and adds nothing.
    fn show_place(&mut self, place: NavPlace, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = place.mode;
        match place.thread {
            Some(id) => self.open(id, window, cx),
            None => {
                self.select_none(cx);
            }
        }
        self.note_navigation();
        cx.notify();
    }

    /// Where Back or Forward would go, named for its tooltip (`destination`).
    pub(crate) fn destination(&self, back: bool) -> Option<String> {
        let place = self.history.peek(back, |p| self.can_show(p))?;
        Some(match place.thread.and_then(|id| self.threads.get(&id).cloned()) {
            Some(t) if t.title.is_empty() => "Untitled".to_string(),
            Some(t) => t.title,
            None => match place.mode {
                SidebarMode::Inbox => "New thread".to_string(),
                SidebarMode::Scheduled => "Scheduled".to_string(),
                SidebarMode::Projects => "Projects".to_string(),
                SidebarMode::Librarian => "Librarian".to_string(),
            },
        })
    }

    /// Next Inbox Thread (⌥⌘↓, `nextInboxThread`): the thread below the open
    /// one in the Inbox, or the Inbox's first; past the last one, a note.
    pub(crate) fn next_inbox_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = crate::now_ms();
        let inbox: Vec<String> = self.shown_sections(cx, now).into_iter().find(|s| s.key == "inbox").map(|s| s.rows.into_iter().map(|t| t.id).collect()).unwrap_or_default();
        let next = match self.selected.as_ref().and_then(|s| inbox.iter().position(|id| id == s)) {
            Some(i) => inbox.get(i + 1).cloned(),
            None => inbox.first().cloned(),
        };
        let Some(next) = next else {
            self.archived_note = None;
            self.show_inbox_end(inbox.is_empty(), cx);
            return;
        };
        self.inbox_end_note = None;
        self.mode = SidebarMode::Inbox;
        // A folded Inbox has no row to show it by.
        if self.folded.remove("inbox") {
            self.sidebar_settings_changed(cx);
        }
        self.open(next, window, cx);
    }

    pub(crate) fn show_inbox_end(&mut self, empty: bool, cx: &mut Context<Self>) {
        self.note_counter += 1;
        let id = self.note_counter;
        self.inbox_end_note = Some(InboxEndNote { id, empty });
        // Three seconds, then it goes (`InboxEndToast.shownFor`), unless the
        // pointer's on it.
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let keep = this.update(cx, |this, cx| {
                    if this.inbox_end_note.is_some_and(|n| n.id == id) {
                        if this.toast_hovered {
                            return true;
                        }
                        this.inbox_end_note = None;
                        cx.notify();
                    }
                    false
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        });
        self.note_task = Some(task);
        cx.notify();
    }

    pub(crate) fn dismiss_inbox_end(&mut self, cx: &mut Context<Self>) {
        self.inbox_end_note = None;
        cx.notify();
    }

    /// The archived note: six seconds, unless the pointer's on it
    /// (`ArchivedToast.shownFor`).
    pub(crate) fn show_archived_note(&mut self, thread: String, title: String, was_open: bool, cx: &mut Context<Self>) {
        self.note_counter += 1;
        let id = self.note_counter;
        self.archived_note = Some(ArchivedNote { id, thread, title, was_open });
        self.inbox_end_note = None;
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(6)).await;
                let keep = this.update(cx, |this, cx| {
                    if this.archived_note.as_ref().is_some_and(|n| n.id == id) {
                        if this.toast_hovered {
                            return true;
                        }
                        this.archived_note = None;
                        cx.notify();
                    }
                    false
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        });
        self.archived_task = Some(task);
        cx.notify();
    }

    /// The archived note's Undo: back in the inbox, and open again if it was
    /// the one open when it went (`undoArchive`).
    pub(crate) fn undo_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.archived_note.take() else { return };
        self.unarchive_row(&note.thread, cx);
        if note.was_open {
            self.mode = SidebarMode::Inbox;
            self.open(note.thread, window, cx);
        }
        cx.notify();
    }

    /// A click on the archived note: opens the thread where it now is
    /// (`openArchivedNote`).
    pub(crate) fn open_archived_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.archived_note.take() else { return };
        self.show_folded_section("archived", cx);
        self.open(note.thread, window, cx);
        cx.notify();
    }

    // Keys and menus.

    /// Applies the shortcuts in force: bindings added for what's new, and a
    /// `NoAction` on what's gone, since GPUI's bindings are additive and
    /// clearing the keymap would lose gpui-kit's own.
    pub(crate) fn apply_shortcuts(&mut self, cx: &mut App) {
        let macos = cfg!(target_os = "macos");
        let mut desired: HashMap<String, Command> = HashMap::new();
        for command in Command::ALL {
            if let Some(key) = command.shortcut().and_then(|c| self.shortcuts.key(c)) {
                desired.insert(key.chord(macos), *command);
            }
        }
        let mut added = Vec::new();
        for (chord, command) in &desired {
            if self.bound_keys.get(chord) != Some(command) {
                added.push(command.binding(chord));
            }
        }
        for chord in self.bound_keys.keys() {
            if !desired.contains_key(chord) {
                added.push(KeyBinding::new(leak(chord), NoAction, None));
            }
        }
        if !added.is_empty() {
            cx.bind_keys(added);
        }
        self.bound_keys = desired;
    }

    /// shortcuts.json changed (or the app just started): the keys in use, the
    /// keymap and the menus' keys follow it. One that won't parse leaves the
    /// keys in use alone: an agent may be halfway through editing it (`load`).
    fn reload_shortcuts(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        match text {
            None => self.shortcuts = ShortcutBindings::default(),
            Some(text) => {
                if let Ok(read) = ShortcutBindings::parse(&text) {
                    self.shortcuts = read;
                }
            }
        }
        self.apply_shortcuts(cx);
        self.menu_signature.clear();
        self.sync_menus(cx);
        cx.notify();
    }

    /// Watches shortcuts.json: a change to the file shows straight away, as
    /// the Swift app's DispatchSource watch does. A poll rather than a file
    /// watcher crate: the file is tiny and this needs no dependency.
    pub(crate) fn watch_shortcuts(&mut self, cx: &mut Context<Self>) {
        let path = self.settings.path().parent().map(|d| d.join("shortcuts.json")).unwrap_or_else(|| "shortcuts.json".into());
        self.reload_shortcuts(std::fs::read_to_string(&path).ok(), cx);
        self.shortcuts_path = Some(path.clone());
        self.shortcuts_watch = Some(cx.spawn(async move |this, cx| {
            let mut last = std::fs::read_to_string(&path).ok();
            loop {
                cx.background_executor().timer(Duration::from_millis(600)).await;
                let now = std::fs::read_to_string(&path).ok();
                if now != last {
                    last = now.clone();
                    if this.update(cx, |this, cx| this.reload_shortcuts(now, cx)).is_err() {
                        break;
                    }
                }
            }
        }));
    }

    /// The menu bar, rebuilt only when what it says changes (a title like
    /// Archive/Unarchive, a greyed-out item, a key).
    pub(crate) fn sync_menus(&mut self, cx: &mut Context<Self>) {
        let menus = self.menu_model(cx);
        let signature = serde_json::to_string(&menus_json(&menus, &self.shortcuts)).unwrap_or_default();
        if signature != self.menu_signature {
            self.menu_signature = signature;
            cx.set_menus(gpui_menus(&menus));
        }
    }

    /// The window's title (Window menu, Mission Control), as `windowTitle`.
    pub(crate) fn sync_window_title(&mut self, window: &mut Window) {
        let title = self.main_panel_title();
        if title != self.last_window_title {
            window.set_window_title(&title);
            self.last_window_title = title;
        }
    }

    fn menu_model(&self, cx: &App) -> Vec<TopMenu> {
        let t = self.open_thread();
        let has_thread = t.is_some();
        let zoom_now = style::zoom_percent();
        let panels_shown = !self.open_panes(cx).is_empty();
        let threads_contains = self.dock.read(cx).layout().contains(THREADS);
        let terminal_shown = self.is_panel_showing(TERMINAL, cx);
        let archived = t.is_some_and(|t| t.is_archived());
        let busy = t.is_some_and(|t| t.busy());
        use MenuEntry::*;
        let item = |title: &str, command: Command| MenuEntry::item(title, command);
        vec![
            TopMenu::new("Threadbear", vec![item("About Threadbear", Command::About), Separator, item("Settings…", Command::Settings)]),
            TopMenu::new("File", vec![item("New Thread", Command::NewThread)]),
            TopMenu::new(
                "View",
                vec![
                    item("Zoom In", Command::ZoomIn).enabled(zoom::can_zoom_in(zoom_now)),
                    item("Zoom Out", Command::ZoomOut).enabled(zoom::can_zoom_out(zoom_now)),
                    item("Actual Size", Command::ActualSize).enabled(zoom_now != zoom::DEFAULT_PERCENT),
                    Separator,
                    item(if threads_contains { "Hide Threads" } else { "Show Threads" }, Command::ToggleThreads),
                    item(if panels_shown { "Hide Panels" } else { "Show Panels" }, Command::TogglePanels),
                    item(if terminal_shown { "Hide Terminal" } else { "Show Terminal" }, Command::ToggleTerminal),
                    item("Reset Panel Layout", Command::ResetPanelLayout),
                ]
                .into_iter()
                .chain(self.frame_rate_items())
                .collect(),
            ),
            TopMenu::new(
                "Go",
                vec![
                    item("Back", Command::Back).enabled(self.can_go_back()),
                    item("Forward", Command::Forward).enabled(self.can_go_forward()),
                    Separator,
                    item("Next Inbox Thread", Command::NextInboxThread),
                ],
            ),
            TopMenu::new(
                "Thread",
                vec![
                    item(if archived { "Unarchive" } else { "Archive" }, Command::Archive).enabled(has_thread),
                    Submenu { title: "Hibernate".into(), items: self.hibernate_menu_items() },
                    item("Stop", Command::Stop).enabled(busy),
                    Separator,
                    item("Copy Transcript as Markdown", Command::CopyTranscript).enabled(has_thread),
                    Separator,
                    item("Show Inbox", Command::ShowInbox),
                    item("Show Scheduled", Command::ShowScheduled).enabled(self.schedules_supported),
                    item("Show Projects", Command::ShowProjects).enabled(self.projects_supported),
                    item("Show Librarian", Command::ShowLibrarian).enabled(self.projects_supported),
                    Separator,
                    item("Show Hibernating", Command::ShowHibernating),
                    item("Show Archived", Command::ShowArchived),
                ],
            ),
            TopMenu::new("Help", vec![item("Take the Tour", Command::TakeTheTour), item("Finish Setting Up", Command::FinishSettingUp)]),
        ]
    }

    /// Show Frame Rate, in a build that can measure it (`frame-stats`).
    fn frame_rate_items(&self) -> Vec<MenuEntry> {
        #[cfg(feature = "frame-stats")]
        {
            let title = if self.frame_rate.is_some() { "Hide Frame Rate" } else { "Show Frame Rate" };
            vec![MenuEntry::Separator, MenuEntry::item(title, Command::ToggleFrameRate)]
        }
        #[cfg(not(feature = "frame-stats"))]
        Vec::new()
    }

    fn toggle_frame_rate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(feature = "frame-stats")]
        {
            self.frame_rate = match self.frame_rate.take() {
                Some(_) => None,
                None => Some(cx.new(|cx| crate::frame_stats::FrameRate::new(window, cx))),
            };
            cx.notify();
        }
        let _ = (window, cx);
    }

    fn hibernate_menu_items(&self) -> Vec<MenuEntry> {
        let presets = threadbear_core::sidebar::snooze_presets(&threadbear_core::sidebar::local_now());
        let mut items: Vec<MenuEntry> = presets
            .iter()
            .filter_map(|p| {
                let command = match p.id {
                    "hour" => Command::HibernateHour,
                    "evening" => Command::HibernateEvening,
                    "tomorrow" => Command::HibernateTomorrow,
                    "monday" => Command::HibernateMonday,
                    _ => return None,
                };
                Some(MenuEntry::item(p.title, command))
            })
            .collect();
        items.push(MenuEntry::Separator);
        items.push(MenuEntry::item("Pick a time…", Command::Hibernate));
        items.push(MenuEntry::item(threadbear_core::sidebar::WAKE_MENU_TITLE, Command::WakeWhen));
        items
    }

    /// Every menu as data, for the specs and `app-menu` (Automation's `menus`).
    #[cfg(feature = "automation")]
    pub(crate) fn menus_for_dump(&self, cx: &App) -> Value {
        menus_json(&self.menu_model(cx), &self.shortcuts)
    }

    #[cfg(feature = "automation")]
    pub(crate) fn run_menu_item(&mut self, title: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let menus = self.menu_model(cx);
        fn find(entries: &[MenuEntry], title: &str) -> Option<Command> {
            entries.iter().find_map(|e| match e {
                MenuEntry::Item { title: t, command, .. } if t == title => Some(*command),
                MenuEntry::Submenu { items, .. } => find(items, title),
                _ => None,
            })
        }
        let command = menus.iter().find_map(|m| find(&m.items, title)).ok_or_else(|| format!("no menu item {title:?}"))?;
        self.run_command(command, window, cx);
        Ok(())
    }

    // The shell's drawing.

    pub(crate) fn render_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        self.sync_window_title(window);
        // With nothing focused GPUI's key dispatch doesn't reach this element's
        // key context or actions, so the shell keeps the keyboard when nothing
        // else has it (no thread open, a closed dialog).
        if window.focused(cx).is_none() {
            window.focus(&self.root_focus, cx);
        }
        let has_rail = self.open_thread().is_some();
        let rail = has_rail.then(|| self.render_right_rail(cx));
        let root = div()
            .id("threadbear-shell")
            .track_focus(&self.root_focus)
            .flex()
            .flex_row()
            .key_context("Threadbear")
            .size_full()
            .items_start()
            .child(self.render_activity_bar(cx))
            .child(div().flex_1().min_w_0().h_full().child(self.dock_view.clone()))
            .when_some(rail, |s, r| s.child(r))
            // The mouse's side buttons (`MouseNavigation`).
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), cx.listener(|this, _: &MouseDownEvent, window, cx| this.go_back(window, cx)))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), cx.listener(|this, _: &MouseDownEvent, window, cx| this.go_forward(window, cx)));
        register_commands(root, cx)
    }

    /// What a panel shows (`RootView.content`'s `view`).
    pub(crate) fn panel_view(&mut self, panel: &str, ghost: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match panel {
            // A drag's ghost of it is drawn in place, not from the cached view,
            // which is already drawn where the panel sits.
            THREADS if ghost => self.render_threads_panel(window, cx),
            THREADS => {
                let app = cx.entity();
                let view = self.sidebar_view.get_or_insert_with(|| cx.new(|cx| crate::sidebar::SidebarView::new(&app, cx))).clone();
                AnyView::from(view).cached(StyleRefinement::default().size_full()).into_any_element()
            }
            MAIN => self.render_main_panel(window, cx).into_any_element(),
            // Highlights, Files, Patches and a patch's own panels are sections of
            // the one pane beside the thread (highlights.rs), each its own panel
            // in the dock, as in the Swift app.
            _ if !ghost && self.is_side_section(panel) => self.render_side_section(panel, cx),
            _ => self.render_pane_placeholder(panel, cx).into_any_element(),
        }
    }

    /// What the Threads panel shows: the thread list, or the page of the
    /// rail's other modes (drawn by `SidebarView`).
    pub(crate) fn render_threads_panel(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.mode {
            SidebarMode::Inbox => self.render_sidebar(cx).into_any_element(),
            other => self.render_mode_sidebar(other, cx).into_any_element(),
        }
    }

    /// The Scheduled, Projects and Librarian views aren't in this app yet
    /// (their rows are the next stages of the port): their place in the
    /// sidebar says so plainly, so the rail and the keys already work.
    fn render_mode_sidebar(&mut self, mode: SidebarMode, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match mode {
            SidebarMode::Scheduled => "Scheduled",
            SidebarMode::Projects => "Projects",
            _ => "Librarian",
        };
        let muted = cx.theme().muted_foreground;
        v_flex().size_full().p(px(16.)).gap(px(6.)).child(div().text_size(px(style::size::HEADER)).font_weight(FontWeight::SEMIBOLD).child(title)).child(div().text_size(px(style::size::LABEL)).text_color(muted).child("This view isn't in this app yet."))
    }

    /// The main panel: banners over the thread, the notes floating over it.
    fn render_main_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let page = match self.mode {
            SidebarMode::Inbox => None,
            SidebarMode::Scheduled => Some("Scheduled"),
            SidebarMode::Projects => Some("Projects"),
            SidebarMode::Librarian => Some("Librarian"),
        };
        let banners = self.render_banners(window, cx);
        let main: AnyElement = match (&self.pane, page) {
            (Some(pane), None) => div().size_full().child(pane.clone()).into_any_element(),
            (_, Some(page)) => div().size_full().flex().items_center().justify_center().text_color(muted).child(format!("The {page} page isn't in this app yet.")).into_any_element(),
            // No thread open is the new-thread page, as in the Swift app.
            (None, None) => self.render_new_page(window, cx),
        };
        let toasts = self.render_toasts(window, cx);
        div().relative().size_full().child(v_flex().size_full().child(banners).child(div().flex_1().min_h_0().child(main))).child(toasts)
    }

    fn render_pane_placeholder(&mut self, panel: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let name = self.panel_title(panel);
        let text = if self.open_thread().is_some() { format!("{name} isn't in this app yet.") } else { format!("Pick a thread to see its {name}.") };
        div().size_full().flex().items_start().justify_center().pt(px(40.)).text_size(px(style::size::LABEL)).text_color(muted).child(text)
    }

    /// The Highlights panel. Its content is being built in `highlights.rs`
    /// (another branch of the port); until that lands this is the empty panel
    /// it will replace, with the same title and id.
    fn is_side_section(&self, panel: &str) -> bool {
        matches!(panel, HIGHLIGHTS | FILES | PATCHES) || self.open_thread().is_some_and(|t| t.panels.iter().flatten().any(|p| p["id"].as_str() == Some(panel)))
    }

    /// The side pane, showing this panel's section. Its entity draws one section
    /// at a time, so two of them shown side by side would show the same one
    /// (the Swift app draws each separately): for later, it needs a split of
    /// `SidePane` into sections.
    fn render_side_section(&mut self, section: &str, cx: &mut Context<Self>) -> AnyElement {
        if let Some(thread) = self.open_thread().cloned() {
            if self.side.read(cx).section_for(&thread) != section {
                // Afterwards, as this runs while the app's own state is held and
                // the pane reads that.
                let (side, section) = (self.side.clone(), section.to_owned());
                cx.defer(move |cx| side.update(cx, |side, cx| side.choose_section(&thread.id, &section, cx)));
            }
        }
        // Cached, like the Threads panel, so scrolling the transcript doesn't
        // redraw it every frame: it draws again when the app changes
        // (SidePane::app_changed) or its own state does.
        AnyView::from(self.side.clone()).cached(StyleRefinement::default().size_full()).into_any_element()
    }
}

/// `KeyBinding::new` wants `&'static str` keystrokes; a chord only changes
/// when shortcuts.json does, so the few bytes each change leaks don't matter.
fn leak(chord: &str) -> &'static str {
    Box::leak(chord.to_string().into_boxed_str())
}

/// The app's theme fonts for this zoom: gpui-kit's rem sizes and everything
/// built on them (menus, inputs, tooltips) follow.
pub(crate) fn apply_zoom_to_theme(percent: u32, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.font_size = gpui_kit::px(zoom::scaled(16., percent));
    theme.mono_font_size = gpui_kit::px(zoom::scaled(13., percent));
}

/// One top-level menu: a model so the menu bar, `menus` and `menu-item` all
/// read the same thing.
pub(crate) struct TopMenu {
    title: String,
    items: Vec<MenuEntry>,
}

impl TopMenu {
    fn new(title: &str, items: Vec<MenuEntry>) -> Self {
        Self { title: title.to_string(), items }
    }
}

pub(crate) enum MenuEntry {
    Item { title: String, command: Command, enabled: bool },
    Submenu { title: String, items: Vec<MenuEntry> },
    Separator,
}

impl MenuEntry {
    fn item(title: &str, command: Command) -> Self {
        MenuEntry::Item { title: title.to_string(), command, enabled: true }
    }

    fn enabled(mut self, on: bool) -> Self {
        if let MenuEntry::Item { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }
}

fn menus_json(menus: &[TopMenu], shortcuts: &ShortcutBindings) -> Value {
    fn entries(items: &[MenuEntry], shortcuts: &ShortcutBindings) -> Vec<Value> {
        items
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item { title, command, enabled } => {
                    let key = command.shortcut().and_then(|c| shortcuts.key(c)).map(|k| k.display());
                    let mut out = serde_json::json!({ "title": title, "enabled": enabled });
                    if let Some(key) = key {
                        out["key"] = Value::String(key);
                    }
                    Some(out)
                }
                MenuEntry::Submenu { title, items } => Some(serde_json::json!({ "title": title, "items": entries(items, shortcuts) })),
                MenuEntry::Separator => None,
            })
            .collect()
    }
    Value::Array(menus.iter().map(|m| serde_json::json!({ "title": m.title, "items": entries(&m.items, shortcuts) })).collect())
}

fn gpui_menus(menus: &[TopMenu]) -> Vec<Menu> {
    fn entries(items: &[MenuEntry]) -> Vec<MenuItem> {
        items
            .iter()
            .map(|e| match e {
                MenuEntry::Item { title, command, enabled } => MenuItem::Action { name: title.clone().into(), action: command.action(), os_action: None, checked: false, disabled: !enabled },
                MenuEntry::Submenu { title, items } => MenuItem::submenu(Menu::new(title.clone()).items(entries(items))),
                MenuEntry::Separator => MenuItem::Separator,
            })
            .collect()
    }
    menus.iter().map(|m| Menu::new(m.title.clone()).items(entries(&m.items))).collect()
}
