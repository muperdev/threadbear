// The icons the app draws in place of SF Symbols, which can't ship outside
// Apple's systems. Lucide comes with gpui-kit's assets; main.rs registers
// `AllAssets`, so any Lucide icon can be named here. This is the one place
// that maps Swift's symbol names to a bundled icon: add a line when a view
// needs a new one. A symbol with no match draws a question mark, which is
// easy to spot in a screenshot; a project's own icon falls back to its letter
// instead (see `try_symbol`).
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;

/// `star.fill` has no Lucide twin (Lucide's stars are outlines), so it's drawn
/// here: the star's shape, filled with the current text colour.
const STAR_FILLED: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 2.5l2.9 6.4 7 .7-5.2 4.7 1.5 6.9L12 17.7l-6.2 3.5 1.5-6.9-5.2-4.7 7-.7z" fill="black"/></svg>"#;

/// `RunnerIcon.symbol`: a runner's own icon name as a symbol this app has a
/// drawing of, or the plain desktop.
pub fn runner_symbol(icon: Option<&str>) -> &'static str {
    match icon {
        Some("macmini") => "macmini",
        Some("laptopcomputer") => "laptopcomputer",
        Some("cloud") => "cloud",
        Some("server.rack") => "server.rack",
        Some("smartphone") | Some("iphone") => "smartphone",
        _ => "desktopcomputer",
    }
}

/// The bundled icon for one of the app's SF Symbol names.
pub fn symbol(name: &str) -> Icon {
    try_symbol(name).unwrap_or_else(|| Icon::new(IconName::CircleQuestionMark))
}

/// The bundled icon for an SF Symbol name, if there is one yet.
pub fn try_symbol(name: &str) -> Option<Icon> {
    let lucide = match name {
        "star" => IconName::Star,
        "star.fill" => return Some(Icon::default().data(STAR_FILLED)),
        "star.slash" => IconName::StarOff,
        "tray" => IconName::Inbox,
        "tray.and.arrow.up" => IconName::ArchiveRestore,
        "bolt" => IconName::Zap,
        "moon" | "moon.zzz" => IconName::Moon,
        "archivebox" => IconName::Archive,
        "arrow.uturn.backward" => IconName::Undo2,
        "trash" => IconName::Trash,
        "sun.max" => IconName::Sun,
        "folder" => IconName::Folder,
        "square.and.pencil" => IconName::SquarePen,
        "magnifyingglass" => IconName::Search,
        "xmark.circle.fill" | "xmark.circle" => IconName::CircleX,
        "hand.raised" => IconName::Hand,
        "arrow.down" => IconName::ArrowDown,
        "arrow.up" => IconName::ArrowUp,
        "arrow.up.arrow.down" => IconName::ArrowUpDown,
        "ellipsis" => IconName::Ellipsis,
        "chevron.down" => IconName::ChevronDown,
        "chevron.right" => IconName::ChevronRight,
        "checkmark" => IconName::Check,
        "xmark" => IconName::X,
        "minus" => IconName::Minus,
        "clock" => IconName::Clock,
        "sunset" => IconName::Sunset,
        "sunrise" => IconName::Sunrise,
        "briefcase" => IconName::Briefcase,
        "calendar" => IconName::Calendar,
        "text.bubble" => IconName::MessageSquareText,
        "arrow.clockwise" => IconName::RotateCw,
        "pawprint" => IconName::PawPrint,
        "books.vertical" => IconName::Library,
        "bell" => IconName::Bell,
        "wrench.and.screwdriver" => IconName::Wrench,
        "hammer" => IconName::Hammer,
        "number" => IconName::Hash,
        "pencil" => IconName::Pencil,
        "rectangle.split.2x1" => IconName::Columns2,
        "square.stack" => IconName::Layers,
        "plus" => IconName::Plus,
        "desktopcomputer" => IconName::Monitor,
        "macmini" => IconName::Server,
        // The symbols a runner can name for itself (`RunnerInfo.icon`).
        "laptopcomputer" => IconName::Laptop,
        "cloud" => IconName::Cloud,
        "server.rack" => IconName::Server,
        "smartphone" => IconName::Smartphone,
        "arrow.right.square" => IconName::SquareArrowRight,
        "ladybug" | "ant" => IconName::Bug,
        "externaldrive" => IconName::HardDrive,
        // The symbols Settings › Pools and the librarian offer for a project
        // (`Pools.iconChoices`).
        "photo" => IconName::Image,
        "sparkle" => IconName::Sparkle,
        "chevron.left.forwardslash.chevron.right" => IconName::CodeXml,
        "house" => IconName::House,
        "paintbrush" => IconName::Paintbrush,
        "film" => IconName::Film,
        "book" => IconName::Book,
        "graduationcap" => IconName::GraduationCap,
        "leaf" => IconName::Leaf,
        "flame" => IconName::Flame,
        "heart" => IconName::Heart,
        "flag" => IconName::Flag,
        "bookmark" => IconName::Bookmark,
        "tag" => IconName::Tag,
        "globe" => IconName::Globe,
        "building.2" => IconName::Building2,
        "cart" => IconName::ShoppingCart,
        "gamecontroller" => IconName::Gamepad2,
        "music.note" => IconName::Music,
        "camera" => IconName::Camera,
        "airplane" => IconName::Plane,
        "cup.and.saucer" => IconName::Coffee,
        // The window shell's (WindowDock, ActivityBar, DetailView).
        "bubble.left.and.bubble.right" => IconName::MessagesSquare,
        "square.stack.3d.up" => IconName::Layers,
        "gearshape" => IconName::Settings,
        "gauge.with.dots.needle.33percent" | "gauge.with.dots.needle.100percent" => IconName::Gauge,
        "arrow.down.circle" => IconName::CircleArrowDown,
        "checklist" => IconName::ListChecks,
        "powerplug" => IconName::Plug,
        "terminal" => IconName::Terminal,
        "pin" => IconName::Pin,
        "square.dashed" => IconName::SquareDashed,
        "text.alignleft" => IconName::TextAlignStart,
        "sidebar.left" => IconName::PanelLeft,
        "sidebar.right" => IconName::PanelRight,
        "chevron.left" => IconName::ChevronLeft,
        "square.and.arrow.up" => IconName::Share,
        "doc.plaintext" => IconName::FileText,
        "safari" => IconName::Compass,
        "exclamationmark.triangle" => IconName::TriangleAlert,
        "bolt.horizontal" | "bolt.horizontal.circle" => IconName::Zap,
        "lock" => IconName::Lock,
        "person.crop.circle.badge.exclamationmark" => IconName::UserRoundX,
        "checkmark.circle.fill" => IconName::CircleCheck,
        "arrow.up.right" => IconName::ArrowUpRight,
        "arrow.triangle.2.circlepath" => IconName::RefreshCw,
        "arrow.right.circle" => IconName::CircleArrowRight,
        "minus.circle" => IconName::CircleMinus,
        "plus.square.on.square" => IconName::CopyPlus,
        // The pane beside the thread: Highlights, previews, Patches, folders.
        "doc.text" => IconName::FileText,
        "doc.richtext" => IconName::FileText,
        "doc" => IconName::File,
        "waveform" => IconName::AudioLines,
        "tablecells" => IconName::Table,
        "curlybraces.square" => IconName::FileBraces,
        "arrow.triangle.pull" => IconName::GitPullRequest,
        "smallcircle.filled.circle" => IconName::GitCommitHorizontal,
        "link" => IconName::Link,
        "circle.dashed" => IconName::CircleDashed,
        "pause.circle" => IconName::CirclePause,
        "person.2" => IconName::Users,
        "pin.slash" => IconName::PinOff,
        "arrow.triangle.branch" => IconName::GitBranch,
        "cylinder.split.1x2" => IconName::Database,
        "iphone" => IconName::Smartphone,
        "list.bullet" => IconName::List,
        "doc.on.doc" => IconName::Copy,
        "arrow.up.forward.app" => IconName::SquareArrowOutUpRight,
        "cursorarrow.click" => IconName::MousePointerClick,
        "chevron.up" => IconName::ChevronUp,
        "info.circle" => IconName::Info,
        "eye" => IconName::Eye,
        "eye.slash" => IconName::EyeOff,
        "textformat" => IconName::Type,
        "scalemass" => IconName::Scale,
        "square.grid.2x2" => IconName::LayoutGrid,
        "checkmark.square.fill" => IconName::SquareCheck,
        "square" => IconName::Square,
        "exclamationmark.circle" => IconName::CircleAlert,
        "paperplane" => IconName::Send,
        _ => return None,
    };
    Some(Icon::new(lucide))
}
