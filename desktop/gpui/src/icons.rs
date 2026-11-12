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

/// `hand.raised.fill`, the asks card's mark: Lucide's hand is an outline, and
/// the Swift card's is the filled one, four fingers and the thumb out to the
/// right. Drawn to SF's proportions (about 11 by 15 at the card's 14 pt
/// title), so at 16.5 px it covers what the Swift one does.
const HAND_RAISED_FILLED: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="black"><rect x="4" y="5.2" width="3" height="10.4" rx="1.5"/><rect x="7.4" y="2.2" width="3" height="12" rx="1.5"/><rect x="10.8" y="1" width="3" height="13" rx="1.5"/><rect x="14.2" y="2.6" width="3" height="11.6" rx="1.5"/><path d="M4 12h13.2v1.4l1.2-2.6c.5-1 1.5-1.3 2.2-.8.6.4.7 1.2.4 1.9l-2.6 6.6c-.9 2.4-3 4.1-5.8 4.1h-2.2C6.8 22.6 4 19.9 4 16.4z"/></g></svg>"#;

/// `lock.open`, Full access's mark: SF's has the shackle swung up and out to
/// the right of the body, where Lucide's stands over it, so it read as a
/// closed lock beside the Swift one. An outline in the text colour, about as
/// wide as SF's at 12 pt when drawn at 13.
const LOCK_OPEN: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="11" width="13" height="11" rx="2.5"/><path d="M12.5 11V7.5a4.5 4.5 0 0 1 9 0V9.5"/></svg>"#;

/// SF's `checkmark` as the done card draws it at its title's size: a short
/// stroke and a long one rising well above it. Lucide's check is wide and
/// low, and beside the Swift card's it read as a different mark.
const CHECKMARK_TALL: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 13.5l5 5.5L20 3.5"/></svg>"#;

/// The done card's tick (`checkmark` at 14 pt): about 12 by 12 at 16.
pub fn tall_checkmark() -> Icon {
    Icon::default().data(CHECKMARK_TALL)
}

/// The providers' marks (`ProviderLogo`), as single-colour SVGs that take the
/// text colour, as the Swift app's template images do. Claude's is Anthropic's
/// "A\" rather than Claude's starburst: Mike asked for the Anthropic logo, and
/// it stays legible at 12 pt.
const CLAUDE_LOGO: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill-rule="evenodd"><path fill="black" d="M13.827 3.52h3.603L24 20h-3.603l-6.57-16.48zm-7.258 0h3.767L16.906 20h-3.674l-1.343-3.461H5.017l-1.344 3.46H0L6.57 3.522zm4.132 9.959L8.453 7.687 6.205 13.48H10.7z"/></svg>"#;
const CODEX_LOGO: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill-rule="evenodd"><path fill="black" d="M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z"/></svg>"#;

/// A provider's logo ("claude", "codex"), or its symbol where there's no logo
/// (`ProviderMark`, with `ModelPicker.providerSymbol`'s fallbacks).
pub fn provider_mark(provider: &str) -> Icon {
    match if provider.is_empty() { "claude" } else { provider } {
        "claude" => Icon::default().data(CLAUDE_LOGO),
        "codex" => Icon::default().data(CODEX_LOGO),
        _ => symbol("cpu"),
    }
}

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
        "hand.raised.fill" => return Some(Icon::default().data(HAND_RAISED_FILLED)),
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
        // The project menu's Make Its Own Project and Merge into, and a task
        // card's thread chip (projects.rs, project_page.rs).
        "square.on.square" => IconName::Copy,
        "arrow.triangle.merge" => IconName::Merge,
        "bubble.left" => IconName::MessageSquare,
        // A Notion card's chip in Mike's message (CardChip).
        "square.text.square" => IconName::SquareMenu,
        // A message from another thread's agent (agentMessageText).
        "arrow.turn.down.right" => IconName::CornerDownRight,
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
        // The Runners, Updates and Connectors panels'.
        "line.3.horizontal" => IconName::Menu,
        "arrow.up.forward.square" => IconName::SquareArrowOutUpRight,
        "arrow.up.forward" => IconName::ArrowUpRight,
        "nosign" => IconName::Ban,
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
        "lock.open" => return Some(Icon::default().data(LOCK_OPEN)),
        "cpu" => IconName::Cpu,
        // The trust chip over the composer, and the origin badges' marks.
        "lock.shield" | "lock.shield.fill" => IconName::ShieldLock,
        "shippingbox" | "shippingbox.fill" => IconName::Box,
        "house.fill" => IconName::House,
        "folder.fill" => IconName::Folder,
        "wrench.and.screwdriver.fill" => IconName::Wrench,
        "person.crop.circle.badge.exclamationmark" => IconName::UserRoundX,
        "checkmark.circle.fill" => IconName::CircleCheck,
        "arrow.up.right" => IconName::ArrowUpRight,
        "arrow.up.left.and.arrow.down.right" => IconName::Maximize2,
        "key" => IconName::Key,
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
        "bell.slash" => IconName::BellOff,
        "questionmark.circle" => IconName::CircleQuestionMark,
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
        // The Scheduled list's row buttons and menu, and the editor's Repeat menu.
        "pause" => IconName::Pause,
        "play" | "playpause" => IconName::Play,
        "calendar.badge.clock" => IconName::CalendarClock,
        "slider.horizontal.3" => IconName::SlidersHorizontal,
        "folder.badge.plus" => IconName::FolderPlus,
        // Settings' sidebar.
        "sparkles" => IconName::Sparkles,
        "keyboard" => IconName::Keyboard,
        // The Librarian view's headings.
        "hourglass" => IconName::Hourglass,
        "clock.arrow.circlepath" => IconName::RotateCcwClock,
        // A reply's links' menus (reply_links.rs), the selection bar and the
        // Explain pane (selection_bar.rs, explain_pane.rs).
        "wand.and.stars" => IconName::WandSparkles,
        "questionmark.folder" => IconName::FileQuestionMark,
        // Lucide has no magnifier with a sparkle in it: the plain one is nearest.
        "sparkle.magnifyingglass" => IconName::Search,
        "text.quote" => IconName::TextQuote,
        "arrow.up.circle.fill" => IconName::CircleArrowUp,
        _ => return None,
    };
    Some(Icon::new(lucide))
}
