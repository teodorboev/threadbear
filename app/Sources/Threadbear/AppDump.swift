#if DEBUG
import AppKit
import ThreadbearCore

/// The automation hook's `dump`: what the window shows, as data. The
/// sidebar's rows by section, the open thread's rows (id, kind, height,
/// open or not, the start of their text, which are on screen), the
/// selection, the composer's text and the banner. `Tools/dev-app.sh dump`
/// prints `text`; `dump --json` the JSON.
struct AppDump: Encodable {
    struct Window: Encodable {
        var width: Int
        var height: Int
        var appearance: String
    }

    struct BannerInfo: Encodable {
        var style: String
        var text: String
        var detail: String?
    }

    struct Thread: Encodable {
        var id: String
        var title: String
        var status: String
        var needsYou: Bool
        var unread: Bool
        var hovered: Bool
        /// What the row says is running in the background, if anything.
        var background: String?
        /// "Waiting for room · 5:00 am", for a pool thread with no login
        /// that has room.
        var waiting: String? = nil
        /// The sub-agent rows under it, as the sidebar shows them.
        var subagents: [SubagentInfo] = []
        /// "On bruce" for another runner's thread ("On bruce, offline").
        var runner: String? = nil
        /// The sidebar row as it's drawn (ThreadRowView), for the specs: the
        /// line under the title, whether the title's bold, the mark in the
        /// gutter ("dot" or "tick"), the words on the right and the tile.
        var place: String = ""
        var bold: Bool = false
        var mark: String? = nil
        var trailing: String = ""
        var tile: String = ""
        /// Who started it, when it wasn't Mike: the mark before the place line
        /// (`ThreadByline`), as an SF Symbol name.
        var origin: String? = nil
        /// The hover buttons' tips, while the row's hovered.
        var hoverButtons: [String]? = nil
    }

    struct SubagentInfo: Encodable {
        var id: String
        var depth: Int
        var status: String
        var title: String
        var detail: String
        var selected: Bool
    }

    struct Section: Encodable {
        var name: String
        var threads: [Thread]
        /// Inbox and Scheduled sections, and Projects ones with the
        /// projectDecisions experiment: folded away under their heading.
        var collapsed: Bool? = nil
        /// The number drawn on the heading while it's folded.
        var count: Int? = nil
        /// How many rows it draws, for the Archived section (`ArchiveRows`).
        var shown: Int? = nil
    }

    struct HomePageInfo: Encodable {
        var title: String
        var status: String?
        var stats: [StatInfo]
    }

    struct SettingsWindowInfo: Encodable {
        var tab: String
        var appearance: String
        var defaultFolder: String?
        var keepAwake: Bool?
        var labReview: Bool?
        var privateFolders: [String]?
        var smallJobsPool: String?
        /// Runners, Phone & browser and Keyboard (Automation+SettingsSections.swift).
        var runners: SettingsRunnersInfo? = nil
        var devices: SettingsDevicesInfo? = nil
        var keyboard: SettingsKeyboardInfo? = nil
    }

    struct ScheduleEditorInfo: Encodable {
        var isNew: Bool
        var text: String
        var once: Bool
        var cron: String?
    }

    struct StatInfo: Encodable {
        var value: String
        var label: String
    }

    struct Sidebar: Encodable {
        var mode: String
        var sections: [Section]
        var snoozed: Int
        var archived: Int
        /// How the Inbox is sorted ("needsYou", "newest", "oldest").
        var sort: String = ""
        /// Before the runner's first list: the grey loading rows are showing.
        var loading: Bool = false
    }

    struct Composer: Encodable {
        /// The thread the draft belongs to, or "new".
        var draft: String
        var text: String
        var images: Int
        /// The seq of the message being edited, when the draft is an edit.
        var editing: Int? = nil
        /// Where that thread runs, or where a new one will start: what the
        /// composer's chips and menus show.
        var cwd: String
        var account: String
        /// The model, or "default (...)" saying whose default.
        var model: String
        var ask: Bool
        /// Its permission mode as the composer says it, past "asks".
        var mode: PermissionMode = .full
        /// The effort level, or "default" for the model's own.
        var effort: String = "default"
        /// The pool that picks its login; nil when it's pinned to
        /// `account`, or the runner has no pools.
        var pool: String? = nil
        /// The model chip as it reads ("Claude · Opus 5.5 · High").
        var chip: String = ""
        /// After its title: "Limit reached", or "· 12%" running low.
        var chipBadge: String? = nil
        /// The other runner it runs on ("bruce"); nil for this Mac.
        var runner: String? = nil
        /// The open card in the box, for the behaviour specs: "Done" with
        /// its follow-ups, or "Needs you" with each ask.
        var card: [String]? = nil
        /// What the card's Run buttons say, one per ask with a command
        /// ("▶ Run: npm test", "Running…: npm test", "✓ Ran: npm test").
        var cardRuns: [String]? = nil
        /// A new thread from this box is on its way: its words stay, greyed
        /// and locked, until the runner has made it (`composer.starting`).
        var creating: Bool = false
        /// The picks as chips over the box while the card's folded
        /// (AnswerChips): "1: done".
        var answers: [String]? = nil
    }

    /// The new-thread page, for the behaviour specs (`thread.newpage`),
    /// while it's what's showing: what the heading and note say, the project
    /// chip, and each login's room card.
    struct NewThreadInfo: Encodable {
        var heading: String
        var note: String
        var project: String?
        var rooms: [RoomInfo]
    }

    struct RoomInfo: Encodable {
        var label: String
        var line: String
        var selected: Bool
        var note: String?
    }

    /// The slash palette over the composer while it's open (`composer.slash`).
    struct PaletteInfo: Encodable {
        var query: String
        var category: String
        var categories: [PaletteCategory]
        var commands: [String]
        var picked: String?
        var detail: PaletteDetail?
        /// What the list says when it has nothing to show.
        var note: String?
    }

    struct PaletteCategory: Encodable {
        var title: String
        var count: Int
    }

    struct PaletteDetail: Encodable {
        var title: String
        var usage: String
        var description: String?
        var now: String?
        var also: String?
        var origin: String
        var path: String?
        var buttons: [String]
    }

    /// The Move sheet while it's up (MoveSheet), for the behaviour specs.
    struct MoveSheetInfo: Encodable {
        var title: String
        var subtitle: String
        var folder: String
        var login: String
        var problems: [String]
        var leftBehind: [String]
        var clone: String?
        var asking: Bool
        var canMove: Bool
    }

    /// Set by the sheet as it draws, so `dump` can say what it shows.
    @MainActor static var moveSheetNow: MoveSheetInfo?

    /// Set where the palette is drawn (ComposerView.paletteContent), so
    /// `dump` can say what it shows; nil while it's closed.
    @MainActor static var slashPalette: PaletteInfo?

    /// What a palette shows, in the words `dump` gives them.
    @MainActor
    static func paletteInfo(commands: [SlashCommand]?, query: String, provider: String, recent: [String], state: PaletteState, current: [String: String], canOpenFile: Bool) -> PaletteInfo {
        let all = commands ?? []
        let matches = SlashPalette.matches(all, query: query, category: state.category, recent: recent)
        let picked = matches.first { $0.name == state.selected } ?? matches.first
        let categories = SlashPalette.counts(all, query: query, recent: recent).map { PaletteCategory(title: $0.0.title(provider: provider), count: $0.1) }
        let note: String?
        if commands == nil {
            note = "Getting this thread's commands…"
        } else if matches.isEmpty {
            note = query.isEmpty ? "Nothing here yet." : "No command matches /\(query). Send it and it goes to the agent as words."
        } else {
            note = nil
        }
        let detail = picked.map { command in
            PaletteDetail(
                title: "/\(command.name)",
                usage: command.usage,
                description: command.description.flatMap { $0.isEmpty ? nil : $0 },
                now: current[command.name],
                also: (command.aliases ?? []).isEmpty ? nil : "Also " + (command.aliases ?? []).map { "/\($0)" }.joined(separator: ", "),
                origin: SlashPalette.origin(of: command, provider: provider),
                path: command.path.map { SlashPalette.shortPath($0) },
                buttons: ["Run", "Add details"] + (command.path != nil && canOpenFile ? ["Open file"] : [])
            )
        }
        return PaletteInfo(query: query, category: state.category.title(provider: provider), categories: categories, commands: matches.map { "/\($0.name)" }, picked: picked.map { "/\($0.name)" }, detail: detail, note: note)
    }

    struct Settings {
        var cwd: String
        var account: String
        var model: String
        var ask: Bool
        var mode: PermissionMode = .full
        var effort: String = "default"
        var pool: String? = nil
    }

    /// The runner's pools as the picker's rail shows them, when it has any.
    struct PoolInfo: Encodable {
        var id: String
        var title: String
        var logins: [String]
        var folders: [String]
        /// "3 · 61%".
        var rail: String
        var status: String?
    }

    /// A thread's permission mode as the dump says it.
    static func modeText(_ mode: PermissionMode) -> String {
        switch mode {
        case .ask: "asks before edits"
        case .auto: "auto"
        case .plan: "plan first"
        case .full: "full access"
        case .unknown: "unknown"
        }
    }

    /// What a new thread sent from the composer would get, as the
    /// composer's menus show it (`ThreadSettings` in ComposerControls).
    @MainActor
    static func newThreadSettings(_ app: AppModel) -> Settings {
        let account = app.newThreadAccountId
        return Settings(cwd: app.newThreadCwd ?? app.draftLogins.defaults.cwd, account: account,
                        model: app.newThreadModel ?? defaultModel(app, account: account), ask: app.newThreadPermission == .ask,
                        mode: app.newThreadPermission, effort: app.newThreadEffort ?? "default", pool: app.newThreadPoolId)
    }

    /// Whose default a new thread gets, and which model that is when known.
    @MainActor
    static func defaultModel(_ app: AppModel, account: String) -> String {
        if let model = app.defaults.model(for: app.provider(ofAccount: account)) { return "default (the runner's, \(model))" }
        if let model = app.account(account)?.defaultModel { return "default (the account's, \(model))" }
        return "default (the account's)"
    }

    struct Row: Encodable {
        var id: String
        var kind: String
        var height: Int
        /// Nil for a row that doesn't open (a message, a waiting card).
        var expanded: Bool?
        var inGroup: Bool
        var onScreen: Bool
        var seq: Int
        var turn: String?
        var text: String
        /// The reply buttons its text links, as each pill reads with its
        /// id ("✓ Fill in the form [b7k2abcd]"), or "unknown <id>".
        var buttons: [String]? = nil
        /// The ```slack, ```email and ```draft boxes it draws (DraftBlockCell).
        var drafts: [DraftInfo]? = nil
        /// Its ```mermaid and ```html boxes (RichBlockCell), as the GPUI
        /// app's dump has them.
        var rich: [RichInfo]? = nil
        /// Its closed ```bash blocks and their icons (`shellBlocks`).
        var shellBlocks: [String]? = nil
        /// The chips it draws among its words: Mike's bubble's pictures,
        /// cards and threads ("picture: Picture 1", "card: Fix the button",
        /// "thread: Morning brief").
        var chips: [String]? = nil
        /// A gap row's words, from the row's own top, once drawn: they must
        /// sit inside its fixed height, not over the rows either side.
        var label: LabelFrame? = nil
    }

    struct LabelFrame: Encodable {
        var top: Int
        var height: Int
    }

    /// A draft's box as it's drawn: its label, each line (a Slack quote's
    /// with its bar), each formatted piece ("bold: Hi", "link: there
    /// <https://x.dev>") and what its Copy says.
    struct DraftInfo: Encodable {
        var label: String
        var lines: [String]
        var styled: [String]
        var copy: String
    }

    /// A rich box as drawn: its language, whether the picture or the source
    /// shows, where its drawing has got to ("drawing", "drawn", "failed:
    /// <why>"), whether it's asking about a web runtime (never, here: WebKit
    /// comes with macOS), what its Copy says, and its picture's size.
    struct RichInfo: Encodable {
        var kind: String
        var shows: String
        var state: String
        var asks: Bool
        var copy: String
        var size: String?
    }

    struct LimitInfo: Encodable {
        var text: String
        var button: String?
        /// The box's Carry on by itself, "Yes" or "No", when it offers it.
        var carryOn: String? = nil
    }

    struct Transcript: Encodable {
        var threadId: String
        var state: String
        /// Rows above the first one shown (more pages in as you scroll up).
        var hasMoreAbove: Bool
        var atBottom: Bool
        var rows: [Row]
    }

    var instance: String?
    var connection: String
    var runnerVersion: String
    var banner: BannerInfo?
    /// The line under the bar when the open thread can't go on where it is
    /// (LimitBanner): what it says and its button.
    var limit: LimitInfo?
    /// The Archived note with Undo at the top of the thread panel: its thread.
    var archivedNote: String?
    /// The last archive or hibernate flight (`ArchiveFlight.Record`).
    var flight: ArchiveFlight.Record?
    /// The milestone note at the top of the thread panel: its first line.
    var milestoneNote: String?
    var error: String?
    var window: Window?
    var sidebar: Sidebar
    var selection: Thread?
    /// The sub-agent whose transcript is showing, if one is.
    var subagent: String?
    /// The selected thread's panels and the pane (PanelsView).
    var panels: Panels?
    /// Its action chips (ActionChips).
    var actions: [ActionInfo] = []
    var composer: Composer
    /// The new-thread page while it's showing: nil with a thread open.
    var newThread: NewThreadInfo? = nil
    /// The slash palette over the composer while it's open.
    var palette: PaletteInfo? = nil
    /// The Move sheet while it's up.
    var moveSheet: MoveSheetInfo? = nil
    /// The open card of asks and what's picked on it, as lines; nil when
    /// there's none.
    var decisions: [String]? = nil
    var transcript: Transcript?
    /// The usage popover's content as lines, and whether it's open.
    var usage: Usage? = nil
    /// The project page showing in place of the thread, if one is.
    var project: ProjectInfo? = nil
    /// The open project page's decisions (the projectDecisions experiment),
    /// one line a thread: "Title: first ask | second ask", "(sending)" at
    /// the end while an answer's on its way. Nil with the experiment off or
    /// no project page open.
    var projectDecisions: [String]? = nil
    /// The Projects page's cards, while it's showing: "Title: 1 needs
    /// you · 2 working" (or "Title: quiet").
    var projectCards: [String]? = nil
    /// The page in the detail area in place of a thread (`showsHomePage`):
    /// its title, and for Scheduled its status line and tiles.
    var homePage: HomePageInfo? = nil
    /// The schedule editor while it's up, as it was opened: the sheet keeps
    /// its own copy after that, so what's typed in it isn't here.
    var scheduleEditor: ScheduleEditorInfo? = nil
    /// The Settings window while it's open: its section, and the runner's
    /// settings it shows.
    var settingsWindow: SettingsWindowInfo? = nil
    /// The librarian's state in words ("Looking at ..."), when the runner
    /// has projects.
    var librarian: String? = nil
    /// Empty from a runner older than pools.
    var pools: [PoolInfo] = []
    /// The window's panels and where they are (WindowDock), in one line.
    var dock: String = ""
    /// The last time the panels moved with their glide (a panel opened or
    /// closed): where each panel's group was and where it is now, by panel.
    var panelMotion: PanelMotion?
    struct PanelMotion: Encodable {
        struct Rect: Encodable { var x, y, width, height: Double }
        var from: [String: Rect]
        var to: [String: Rect]
    }
    /// The Threads panel is on screen: in the layout and not folded away
    /// with ⌘/ (`threadsShowing`).
    var threadsShowing: Bool = true
    /// Threads open in panels of their own (ThreadPanels.swift).
    var threadPanels: [ThreadPanelInfo] = []
    /// The thread the main panel has selected but says is open in its own
    /// panel (`OpenElsewhereNote`).
    var openElsewhere: String? = nil
    /// The keys shown beside the buttons while ⌘ is held (ShortcutHints),
    /// as "Threads ⌘1", in the buttons' order.
    var keyHints: [String] = []
    /// The card in the middle of the window while a page swipe is under way
    /// (`SwipeNavigationArrow`).
    var pageSwipe: PageSwipeInfo? = nil

    struct PageSwipeInfo: Encodable {
        /// Its lines: "Back to", the thread or page, and where it is.
        var lines: [String]
        /// Letting go goes there.
        var armed: Bool
        var progress: Double
    }

    struct ThreadPanelInfo: Encodable {
        var id: String
        var title: String
        /// Its tab's the one showing in its group, or it's in a window of its own.
        var showing: Bool
        var transcript: Transcript?
    }
    /// The window's zoom, in percent (TextZoom).
    var zoom: Int = 100
    /// "end" or "empty" while the end-of-inbox note is up (InboxEndToast).
    var inboxEndNote: String? = nil
    /// The activity bar and the right rail, as drawn (ActivityBar, PanelRail).
    var rails: Rails = Rails()
    /// The panel bar's title line and its buttons' tips (ThreadHeader).
    var header: Header? = nil
    /// What the activity bar's Runners, Updates and Connectors panels draw,
    /// a line each, and whether each is on show (as `usage` has it). The
    /// GPUI app's dump says the same lines.
    var runners: PanelLines? = nil
    var updates: PanelLines? = nil
    var connectors: PanelLines? = nil
    /// The Terminal panel: whether it's up, the open thread's shell and what
    /// its screen says, and every shell (TerminalPanel.swift).
    var terminal: TerminalInfo? = nil
    /// Diagrams and pages open full size, by block id (RichBlockWindow).
    var richWindows: [String]? = nil

    struct TerminalInfo: Encodable {
        var shown: Bool
        var cwd: String
        /// Said over the terminal when its folder isn't the thread's.
        var note: String?
        /// "running", "exited" or "none" (not started).
        var shell: String
        var waitingForReturn: Bool
        /// The program has asked for pastes to be marked (bracketed paste):
        /// a shell at its prompt.
        var pastesMarked: Bool
        var focused: Bool
        var cols: Int?
        var rows: Int?
        /// The visible lines, trailing spaces gone.
        var screen: [String]?
        var shells: [String]
    }

    struct RailButton: Encodable {
        var title: String
        var badge: Int? = nil
        /// The accent dot: something only Mike can do.
        var dot: Bool = false
        /// The grey one: something going on that sorts itself out.
        var quietDot: Bool = false
        var active: Bool
    }

    struct PanelLines: Encodable {
        var shown: Bool
        var lines: [String]
    }

    struct Rails: Encodable {
        var activity: [RailButton] = []
        var right: [RailButton] = []
    }

    struct Header: Encodable {
        var title: String
        var renaming: Bool
        var backTip: String
        var forwardTip: String
        var shared: Bool
        /// The bar over the thread with reviews waiting (PendingReviewsButton).
        var pendingReviews: String?
    }

    /// The activity bar's and right rail's buttons, as those views draw them.
    @MainActor
    static func rails(_ app: AppModel) -> Rails {
        var activity = [RailButton(title: "Threads", badge: app.sections.inbox.count, active: app.sidebarMode == .inbox)]
        if app.schedulesSupported {
            activity.append(RailButton(title: "Scheduled", badge: app.activeScheduleCount, active: app.sidebarMode == .scheduled))
        }
        if app.projectsSupported {
            activity.append(RailButton(title: "Projects", active: app.sidebarMode == .projects))
            activity.append(RailButton(title: "Librarian", badge: app.librarian?.pendingCount, active: app.sidebarMode == .librarian))
        }
        // The foot's panels, with their dots (ConnectorsActivityButton,
        // RunnersActivityButton, UpdatesActivityButton).
        if let connectors = app.connectors, connectors.state != "off" {
            activity.append(RailButton(title: "Connectors", dot: ConnectorsText.needsYou(connectors), quietDot: ConnectorsText.quiet(connectors), active: app.connectorsShown))
        }
        if app.runners.count > 1 {
            activity.append(RailButton(title: "Runners", quietDot: app.runners.contains { RunnersText.state($0) != .online }, active: app.runnersShown))
        }
        let side = UpdatesPanel.appSide(AppUpdater.shared)
        activity.append(RailButton(title: "Updates", dot: UpdatesText.needsYou(app: side, runner: app.runnerUpdates),
                                   quietDot: UpdatesText.shown(app: side, runner: app.runnerUpdates) || app.clis.contains(where: UpdatesText.cliPending),
                                   active: app.updatesShown))
        var right: [RailButton] = []
        if let thread = app.paneThread {
            if app.openProject == nil {
                right.append(RailButton(title: "Highlights", dot: thread.firstOpenAsk != nil, active: app.dock.isShowing(PaneSection.highlightsId)))
            }
            for panel in thread.panels {
                right.append(RailButton(title: panel.title, badge: panel.badge, active: app.dock.isShowing(panel.id)))
            }
            if app.openProject == nil {
                right.append(RailButton(title: "Files", active: app.dock.isShowing(BuiltinPanels.files)))
            }
            if !thread.patches.isEmpty {
                right.append(RailButton(title: "Patches", badge: thread.patches.count, active: app.dock.isShowing(PaneSection.patchesId)))
            }
        }
        return Rails(activity: activity, right: right)
    }

    @MainActor
    static func header(_ app: AppModel) -> Header? {
        guard let thread = app.selectedThread, !app.showsHomePage, app.openProject == nil else { return nil }
        return Header(title: thread.title.isEmpty ? "Untitled" : thread.title, renaming: app.renamingThread == thread.id,
                      backTip: HistoryText.tip(back: true, to: app.canGoBack ? app.destination(back: true) : nil, shortcut: AppShortcuts.shared.display(.back)),
                      forwardTip: HistoryText.tip(back: false, to: app.canGoForward ? app.destination(back: false) : nil, shortcut: AppShortcuts.shared.display(.forward)),
                      shared: thread.share?.isOn == true,
                      pendingReviews: thread.hasPendingReviews ? "Review design options (\(thread.pendingReviews ?? 0))" : nil)
    }

    struct ProjectInfo: Encodable {
        var id: String
        var title: String
        /// The project it's a part of, by title, and its own parts' titles.
        var parent: String? = nil
        var parts: [String] = []
        /// The tab showing ("decisions" or "project") with the
        /// projectDecisions experiment on; nil without tabs.
        var tab: String? = nil
        var folders: [String]
        var threads: [String]
        var patches: [String]
        var archived: Bool
    }

    struct Usage: Encodable {
        var shown: Bool
        var lines: [String]
        /// The activity bar's honey pot: how full, for which login, and its
        /// tooltip.
        var icon: UsageIcon
        /// What the panel draws, part by part (`UsagePanel`), the same as
        /// the GPUI app's dump: the tiles and the chart once the stats have
        /// come, a card per pool, the notes over them and the line under.
        var tiles: [UsageTile] = []
        var chart: UsageChart? = nil
        var cards: [UsageCard] = []
        var notes: [String] = []
        /// The Settings card's switch, while some login has a model's own
        /// week for it to switch.
        var settings: UsageSettingsInfo? = nil
        var footer: String? = nil
    }

    struct UsageSettingsInfo: Encodable {
        /// What it says: "Show Weekly Fable".
        var `switch`: String
        var on: Bool
    }

    struct UsageIcon: Encodable {
        var percent: Int
        var login: String?
        var tip: String
    }

    struct UsageTile: Encodable {
        var title: String
        var value: String
        var detail: String?
    }

    struct UsageChart: Encodable {
        var title: String
        var range: String
        /// The key under it: the logins that spent something here.
        var key: [String]
        var empty: String?
    }

    struct UsageCard: Encodable {
        var title: String
        var today: String?
        var logins: [UsageLogin]
    }

    struct UsageLogin: Encodable {
        var name: String
        /// The provider over it, in a card that mixes them.
        var provider: String?
        var note: String?
        var today: String?
        var status: String?
        var meters: [UsageMeter]
        var more: [String]
    }

    struct UsageMeter: Encodable {
        var title: String
        var left: String
        var verdict: String?
    }

    struct ActionInfo: Codable {
        var id: String
        var title: String
        var kind: String
        var running: Bool
    }

    struct Panels: Codable {
        struct Item: Codable {
            var id: String
            var title: String
            var kind: String
            var url: String?
            var note: String?
            var active: Bool
        }
        var shown: Bool
        var width: Int
        var items: [Item]
        /// A file or page open over the section: "file /path" or "page <url>".
        var preview: String?
        /// What the Highlights section has: "3 pinned, 2 files, ..." or why not.
        var highlights: String?
        /// What the pane shows, as drawn: scripts and the specs read this.
        var pane: Pane?
    }

    /// The pane as drawn (`panels.pane`): Highlights' headings and rows, a
    /// patches list, a list panel, or the file open in it.
    struct Pane: Codable {
        struct Row: Codable {
            var title: String
            /// A commit's short sha, before the title.
            var lead: String?
            var trailing: String?
            var note: String?
            var detail: String?
            var symbol: String?
            /// Needs you's tick box.
            var ticked: Bool?
        }
        struct Section: Codable {
            var key: String
            var title: String
            /// What the heading counts, shown when it's folded.
            var count: Int
            var folded: Bool
            /// Left out while folded.
            var rows: [Row]
        }
        struct Patch: Codable {
            var id: String
            var source: String
            /// The orange line and its Trust button, while it's waiting.
            var waiting: String?
            var trusting: Bool?
            var gives: String?
            var error: String?
            var file: String?
        }
        /// Which section shows (a panel's id, or threadbear.highlights).
        var section: String
        /// The one line it has in place of rows ("Nothing here yet...").
        var empty: String?
        var brief: [String]?
        /// Tool patches' cards at the top of Highlights, by title.
        var cards: [String]?
        var sections: [Section]?
        /// The Patches section's rows.
        var patches: [Patch]?
        /// A list panel's rows under their headings, in order.
        var list: [Section]?
        /// An image panel: its size, or the note it shows.
        var image: String?
        /// What's open over the section (a file, folder or page): its kind
        /// ("markdown", "code", "text", "picture", "video", "quickLook", "web",
        /// "folder", "page", "fetching", "note") and what it shows, from the
        /// start (a picture's size, the words).
        var previewKind: String?
        var previewText: String?
        /// A text file's find bar while it's up: its words and where the
        /// selection is among the matches ("the: 2 of 5", "the: not found").
        var previewFind: String?
        /// The line over a text file: the conflict bar's words, or the grey
        /// note (cut off, can't be edited, gone).
        var previewNote: String?
        /// The Markdown file's Preview or Markdown, in the bar's menu, when
        /// the bar has one.
        var previewMode: String?
        /// The folder browser's summary (as `dump`'s preview line), when one
        /// is showing, the Files panel's too.
        var folder: String?
    }

    static func connectionName(_ state: ConnectionState) -> String {
        switch state {
        case .connecting: return "connecting"
        case .connected: return "connected"
        case .runnerDown: return "down"
        case .lost: return "lost"
        case .unauthorized: return "unauthorized"
        }
    }

    /// A transcript as it's drawn: the main panel's, or a thread's in a panel
    /// of its own.
    @MainActor
    static func transcriptInfo(_ controller: TranscriptController) -> Transcript? {
        guard let session = controller.session else { return nil }
        let table = controller.tableView
        let visible = table.rows(in: table.visibleRect)
        let rows = (0..<session.displayCount).map { index -> Row in
            let row = session.displayRow(index)
            return Row(id: row.id, kind: row.kind.rawValue, height: Int(table.rect(ofRow: index).height.rounded()),
                       expanded: opens(row) ? controller.isExpanded(row) : nil, inGroup: row.inGroup,
                       onScreen: NSLocationInRange(index, visible), seq: row.seq, turn: row.turnId, text: RowDebug.excerpt(row),
                       buttons: Self.buttons(row), drafts: Self.drafts(row), rich: Self.rich(row), shellBlocks: Self.shellBlocks(row, threadId: session.threadId),
                       chips: Self.chips(row), label: Self.gapLabel(table, index))
        }
        let state: String
        switch session.state {
        case .idle: state = "idle"
        case .loading: state = "loading"
        case .loaded: state = "loaded"
        case .failed(let message): state = "failed: \(message)"
        }
        return Transcript(threadId: session.threadId, state: state, hasMoreAbove: session.hasMoreAbove,
                          atBottom: controller.isAtBottom, rows: rows)
    }

    /// Where a gap row's label is in its cell (flipped, so from the top),
    /// when the row's on screen.
    @MainActor
    static func gapLabel(_ table: NSTableView, _ index: Int) -> LabelFrame? {
        guard let cell = table.view(atColumn: 0, row: index, makeIfNecessary: false) as? GapRowCell else { return nil }
        return LabelFrame(top: Int(cell.label.frame.minY.rounded()), height: Int(cell.label.frame.height.rounded()))
    }

    /// The keys the buttons show while ⌘ is held (`shortcutHint`): the
    /// activity bar's views and Settings, New thread beside the Inbox's
    /// search, and the composer's Stop while its thread runs with nothing
    /// typed. The GPUI app's dump says the same.
    @MainActor
    static func keyHints(_ app: AppModel) -> [String] {
        guard ShortcutHints.shared.showing else { return [] }
        var out: [String] = []
        func add(_ title: String, _ command: ShortcutCommand) {
            if let key = AppShortcuts.shared.display(command) { out.append("\(title) \(key)") }
        }
        add("Threads", .showInbox)
        if app.schedulesSupported { add("Scheduled", .showScheduled) }
        if app.projectsSupported {
            add("Projects", .showProjects)
            add("Librarian", .showLibrarian)
        }
        add("Settings", .settings)
        if app.sidebarMode == .inbox && app.threadsShowing { add("New thread", .newThread) }
        if let thread = app.selectedThread, thread.status.isBusy {
            let draft = app.drafts[thread.id]
            if (draft?.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, draft?.images.isEmpty ?? true { add("Stop", .stop) }
        }
        return out
    }

    /// What the page swipe's card says (`SwipeNavigationArrow`), as the
    /// GPUI app's dump does.
    @MainActor
    static func pageSwipe(_ app: AppModel) -> PageSwipeInfo? {
        let swipe = SwipeNavigation.shared.swipe
        guard let direction = swipe.direction else { return nil }
        let back = direction == .back
        let destination = app.destination(back: back)
        let empty = swipe.isEmpty || destination == nil
        let place = destination ?? (back ? "Nothing to go back to" : "Nothing to go forward to", nil)
        var lines: [String] = []
        if !empty { lines.append(back ? "Back to" : "Forward to") }
        lines.append(place.title)
        if let detail = place.detail { lines.append(detail) }
        return PageSwipeInfo(lines: lines, armed: swipe.armed, progress: swipe.progress)
    }

    /// Each thread in a panel of its own (ThreadPanels.swift), in the
    /// layout's order: whether its tab's the one showing, and its transcript.
    @MainActor
    static func threadPanels(_ app: AppModel) -> [ThreadPanelInfo] {
        app.dock.layout.panels.compactMap { panel in
            guard let id = WindowDock.pinnedThread(panel) else { return nil }
            return ThreadPanelInfo(id: id, title: WindowDock.title(of: panel, app: app), showing: app.dock.isShowing(panel),
                                   transcript: app.panelTranscripts[id].flatMap(transcriptInfo))
        }
    }

    @MainActor
    static func make(app: AppModel, window: NSWindow?, instance: String?) -> AppDump {
        let now = Date()
        func thread(_ summary: ThreadSummary, section: InboxSection? = nil) -> Thread {
            let look = ThreadRowLook(summary, section: section)
            let selectedSub = app.subagentSelection?.threadId == summary.id ? app.subagentSelection?.subagentId : nil
            // Put-away rows list no sub-agents, as in the sidebar.
            let subagents = section == nil || section?.isPutAway == true ? [] : SubagentRows.visible(summary, threadSelected: app.selection == summary.id, selectedSubagent: selectedSub, now: app.now, finishedShown: section != .inbox).map {
                SubagentInfo(id: $0.id, depth: $0.depth, status: $0.subagent.status.rawValue, title: SubagentRows.title($0.subagent),
                             detail: SubagentRows.detail($0.subagent), selected: $0.id == selectedSub)
            }
            let shown = rowShows(summary, section: section, app: app)
            return Thread(id: summary.id, title: summary.title, status: summary.status.rawValue, needsYou: summary.needsYou,
                          unread: look.unread, hovered: app.hoverOverride == summary.id,
                          background: look.status == .background ? BackgroundWork.short(summary.background) : nil,
                          waiting: summary.waitingForRoom.map { Pools.waitingLine($0, now: app.now) } ?? (summary.waitingForNetwork != nil ? "Waiting for connection" : nil),
                          subagents: subagents, runner: app.runnerPlace(of: summary), place: shown.place, bold: shown.bold, mark: shown.mark,
                          trailing: shown.trailing, tile: shown.tile, origin: ThreadByline.line(summary)?.symbol, hoverButtons: shown.buttons)
        }
        let sections: [Section]
        switch app.sidebarMode {
        case .inbox:
            // The search box applies, as it does to what's on screen.
            let built = app.sections.matching(app.searchQuery)
            sections = InboxSection.allCases.map { section in
                let collapsed = app.collapsedSections.contains(section)
                let searching = !app.searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                let shown = section == .archived ? ArchiveRows.shown(built[section], limit: app.archiveLimit, searching: searching, selection: app.selection).rows.count : nil
                return Section(name: section.title, threads: built[section].map { thread($0, section: section) }, collapsed: collapsed,
                               count: collapsed && !built[section].isEmpty ? built[section].count : nil, shown: shown)
            }
        case .projects:
            // A section per project, then the threads in none.
            // Folded to a row a project with the projectDecisions experiment
            // on: a folded one lists no threads, with its count, as drawn.
            let built = app.projectSections
            let folding = app.projectDecisionsOn
            let section = { (id: String, name: String, threads: [ThreadSummary]) -> Section in
                let folded = folding && !app.openProjectRows.contains(id)
                return Section(name: name, threads: folded ? [] : threads.map { thread($0) },
                               collapsed: folding ? folded : nil, count: folding ? threads.count : nil)
            }
            sections = built.sections.map { section($0.id, $0.project.title, $0.threads) }
                + [section(ProjectFolds.unfiledId, "Not in a project", built.unfiled)]
        case .librarian:
            // Looks in the thread shape: the queue's reason, or what a look
            // did, is the status.
            let status = app.librarian
            sections = [
                Section(name: "Next", threads: (status?.queue ?? []).map { job in
                    Thread(id: job.threadId, title: job.threadTitle, status: job.reason.rawValue, needsYou: false, unread: false, hovered: false)
                }),
                Section(name: "Recent", threads: (status?.recent ?? []).map { entry in
                    Thread(id: entry.threadId, title: entry.text, status: entry.outcome.rawValue, needsYou: false, unread: false, hovered: false)
                }),
            ]
        case .scheduled:
            // Schedules in the thread shape: their state is the status, and
            // the row's lines as ScheduleRowView draws them.
            let groups = ScheduleSection.group(app.schedules)
            sections = ScheduleSection.allCases.map { section in
                Section(name: section.title, threads: (groups[section] ?? []).map { scheduleRow($0, app: app) },
                        collapsed: app.collapsedScheduleSections.contains(section))
            }
        }
        let key = app.selection ?? "new"
        let transcript = transcriptInfo(app.transcript)
        let appearance = window?.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? "dark" : "light"
        return AppDump(
            instance: instance,
            connection: connectionName(app.connection),
            runnerVersion: app.runnerVersion,
            banner: app.banner.map { BannerInfo(style: $0.style.rawValue, text: $0.text, detail: $0.detail) },
            limit: app.selectedThread.flatMap { limitInfo(app, thread: $0) },
            archivedNote: app.archivedNote?.threadId,
            flight: ArchiveFlight.lastFlight,
            milestoneNote: app.milestoneNote.map { ThreadMilestones.title($0.tier) },
            error: app.errorMessage,
            window: window.map { Window(width: Int(contentSize($0).width), height: Int(contentSize($0).height), appearance: appearance) },
            sidebar: Sidebar(mode: String(describing: app.sidebarMode), sections: sections,
                             snoozed: ThreadSections.snoozed(app.threads.values, now: now).count, archived: app.archivedCount,
                             sort: app.inboxOrder.rawValue, loading: !app.threadsLoaded),
            selection: app.selectedThread.map { thread($0) },
            subagent: app.subagentSelection?.subagentId,
            panels: app.selectedThread.flatMap { panels($0, app: app) },
            actions: app.selectedThread.map { thread in
                thread.actions.map { ActionInfo(id: $0.id, title: $0.title, kind: $0.kind.rawValue, running: app.isRunning(action: $0, in: thread.id)) }
            } ?? [],
            composer: composer(app, key: key),
            newThread: newThreadPageDump(app),
            palette: slashPalette,
            moveSheet: app.moveRequest == nil ? nil : moveSheetNow,
            decisions: app.selectedThread.flatMap { decisions($0, app: app) },
            transcript: transcript,
            usage: usage(app),
            project: app.openProject.map { project in
                let threads = ProjectSections.threads(in: project.id, from: app.threads.values)
                return ProjectInfo(id: project.id, title: project.title,
                                   parent: project.parentId.flatMap { app.project($0)?.title },
                                   parts: (app.projectSections.parts[project.id] ?? []).map(\.project.title),
                                   tab: app.projectDecisionsOn ? app.projectTab(project.id).rawValue : nil,
                                   folders: project.folders, threads: threads.map(\.id),
                                   patches: PatchGroups.patches(of: project, in: threads).map(\.patch.id), archived: project.archived)
            },
            projectDecisions: app.openProject.flatMap { projectDecisionLines($0, app: app) },
            projectCards: app.openProject == nil && app.showsProjectsHome ? app.projectCards.map { card in
                card.project.title + ": " + (card.status.isQuiet ? "quiet" : card.status.parts.map(\.text).joined(separator: " · "))
                    + (card.parts.isEmpty ? "" : " (parts: " + card.parts.map(\.title).joined(separator: ", ") + ")")
            } : nil,
            homePage: homePageDump(app),
            scheduleEditor: app.scheduleEditor.map { ScheduleEditorInfo(isNew: $0.isNew, text: $0.text, once: $0.once, cron: $0.once ? nil : $0.repeatChoice.cron) },
            settingsWindow: SettingsWindow.current?.isVisible == true ? SettingsWindowInfo(
                tab: app.settingsTab.rawValue, appearance: UserDefaults.standard.string(forKey: AppAppearance.key) ?? "system",
                defaultFolder: app.runnerSettings?.defaultCwd, keepAwake: app.runnerSettings?.keepAwake, labReview: app.runnerSettings?.labReview,
                privateFolders: app.runnerSettings?.models.privateFolders, smallJobsPool: app.runnerSettings?.models.pool,
                runners: runnersInfo(app), devices: devicesInfo(app), keyboard: keyboardInfo()) : nil,
            librarian: app.librarian.map(LibrarianText.state),
            pools: app.usesPools ? app.pools.map { pool in
                PoolInfo(id: pool.id, title: pool.title, logins: pool.accountIds, folders: pool.folders,
                         rail: Pools.railSubtitle(pool, accounts: app.accounts), status: Pools.statusLine(pool, accounts: app.accounts, now: app.now))
            } : [],
            dock: app.dock.layout.summary.replacingOccurrences(of: "threadbear.", with: ""),
            panelMotion: app.dock.motionFrom.map { from in
                func rects(_ frames: [String: CGRect]) -> [String: PanelMotion.Rect] {
                    Dictionary(frames.map { ($0.key.replacingOccurrences(of: "threadbear.", with: ""), PanelMotion.Rect(x: $0.value.minX.rounded(), y: $0.value.minY.rounded(), width: $0.value.width.rounded(), height: $0.value.height.rounded())) }, uniquingKeysWith: { a, _ in a })
                }
                return PanelMotion(from: rects(from), to: rects(app.dock.panelFrames))
            },
            threadsShowing: app.threadsShowing,
            threadPanels: threadPanels(app),
            openElsewhere: app.selection.flatMap { id in
                !app.showsHomePage && app.openProject == nil && app.dock.layout.contains(WindowDock.threadPanel(id)) ? id : nil
            },
            keyHints: keyHints(app),
            pageSwipe: pageSwipe(app),
            zoom: TextZoom.shared.percent,
            inboxEndNote: app.inboxEndNote.map { $0.empty ? "empty" : "end" },
            rails: rails(app),
            header: header(app),
            runners: PanelLines(shown: app.runnersShown, lines: runnersLines(app)),
            updates: PanelLines(shown: app.updatesShown, lines: updatesLines(app)),
            connectors: PanelLines(shown: app.connectorsShown, lines: connectorsLines(app)),
            terminal: terminalInfo(app, window: window),
            richWindows: RichBlockWindowController.openIds
        )
    }

    /// The Terminal as the GPUI app's dump has it too.
    @MainActor
    static func terminalInfo(_ app: AppModel, window: NSWindow?) -> TerminalInfo {
        let target = TerminalTarget.for(app.selectedThread) { app.runnerName(of: $0) }
        let session = Terminals.shared.existing(for: target)
        let snapshot = session?.view.terminalStateSnapshot()
        let shell = session.map { $0.exited ? "exited" : "running" } ?? "none"
        return TerminalInfo(shown: app.terminalShown, cwd: target.cwd, note: target.note, shell: shell,
                            waitingForReturn: session?.waitingForReturn ?? false,
                            pastesMarked: session.map { !$0.exited && snapshot?.bracketedPasteMode == true } ?? false,
                            focused: session.map { window?.firstResponder === $0.view } ?? false,
                            cols: snapshot?.dimensions.cols, rows: snapshot?.dimensions.rows,
                            screen: snapshot?.visibleRows.map { row in
                                var text = row.text
                                while text.last?.isWhitespace == true { text.removeLast() }
                                return text
                            },
                            shells: Terminals.shared.debugSummary)
    }

    /// What the Runners panel draws (RunnersPanel), a line each: how many are
    /// up, then each runner in Mike's order ("bruce *" for where new threads
    /// start), its figures when its details are open, and what's running
    /// there.
    @MainActor
    static func runnersLines(_ app: AppModel) -> [String] {
        let open = Set((UserDefaults.standard.string(forKey: "runnersPanelOpen") ?? "").split(separator: ",").map(String.init))
        let running = app.sections.running
        let up = app.runners.filter { RunnersText.state($0) != .offline }.count
        var lines = [up == app.runners.count ? "All online" : "\(up) of \(app.runners.count) online"]
        for r in app.orderedRunners {
            let isDefault = r.isSelf ? app.defaultRunnerId == nil : app.defaultRunnerId == r.id
            lines.append("\(r.name)\(isDefault ? " *" : ""): \(RunnersText.stateLine(r, now: app.now))\(RunnersText.build(r).map { ", " + $0 } ?? "")")
            if open.contains(r.id) {
                if let s = app.runnerStats[r.id] {
                    lines += RunnersText.gauges(s).map { "  \($0.title) \($0.text)\($0.high ? " (high)" : "")" }
                    lines += (RunnersText.machineLines(s) + [RunnersText.staleLine(s, now: app.now)].compactMap { $0 }).map { "  " + $0 }
                } else if RunnersText.state(r) == .offline {
                    lines.append("  Offline, so it isn't sending figures.")
                } else {
                    lines.append(app.runnerStatsAsked ? "  Waiting for its figures. An older runner doesn't send them." : "  Asking for its figures…")
                }
            }
            let on = RunnersText.running(on: r, in: running)
            lines += on.prefix(6).map { "  > " + ($0.title.isEmpty ? "Untitled" : $0.title) }
            if on.count > 6 { lines.append("  and \(on.count - 6) more") }
        }
        return lines
    }

    /// What the Updates panel draws (UpdatesPanel): its sections as "# App"
    /// and so on, buttons in brackets, and each CLI as `usageLines` has it.
    @MainActor
    static func updatesLines(_ app: AppModel) -> [String] {
        let side = UpdatesPanel.appSide(AppUpdater.shared)
        var lines = ["# App", UpdatesText.appLine(side), "# Runner"]
        if let runner = app.runnerUpdates {
            let build = UpdatesText.buildLine(runner)
            let old = UpdatesText.oldRunnersLine(runner)
            let launcher = UpdatesText.launcherLine(runner)
            let notFollowing = UpdatesText.followLines(runner)
            if build == nil && old == nil && launcher == nil && notFollowing.isEmpty { lines.append("The runner is up to date.") }
            lines += notFollowing
            if let build { lines.append(build) }
            if let old {
                lines.append(old)
                lines += runner.oldRunners.flatMap(\.threads).map { id in
                    let title = app.threads[id]?.title ?? ""
                    return "  > " + (title.isEmpty ? String(id.prefix(8)) : title)
                }
            }
            if let launcher {
                lines.append(launcher)
                lines.append("  $ \(UpdatesText.installCommand(checkout: AppUpdater.shared.running?.checkout)) [Copy]")
            }
        } else {
            lines += UpdatesText.runnerLines(nil)
        }
        if let upstream = app.runnerUpdates?.upstream, let text = UpdatesText.upstreamLine(upstream) {
            lines += ["# From Mike", text]
            if upstream.available, let release = upstream.release {
                for group in UpdatesText.releaseLines(release) {
                    if let heading = group.heading { lines.append("## " + heading) }
                    lines += group.lines.map { "  " + $0 }
                }
                lines.append("[Bring Them In] [Let's Discuss]")
            }
        }
        lines.append("# CLIs")
        if app.clisSupported == false || (app.clisSupported == nil && app.clis.isEmpty) {
            lines.append("The CLI versions need a newer runner.")
        } else {
            for cli in app.clis {
                var line = CliText.statusLine(cli) + " (" + CliText.policyTitle(cli.policy).lowercased() + ")"
                if let button = CliText.updateTitle(cli) { line += " [\(button)]" }
                if let back = CliText.goBackTitle(cli) { line += " [\(back)]" }
                if CliText.isUpdating(cli), let text = CliText.bannerText(cli) { line += ": " + text }
                lines.append(line)
                if let missing = cli.hold?.missing, !missing.isEmpty { lines.append("  Missing: " + missing.joined(separator: ", ")) }
                if let managed = CliText.managedLine(cli) { lines.append("  " + managed) }
            }
            lines.append(CliText.checkedLine(app.clis, now: app.now).map { $0 + " [Check now]" } ?? "[Check now]")
        }
        return lines
    }

    /// What the Connectors panel draws (ConnectorsPanel): where the gateway
    /// is, why there's nothing to list, or each service with its button.
    @MainActor
    static func connectorsLines(_ app: AppModel) -> [String] {
        let status = app.connectors
        var lines = [ConnectorsText.host(status) ?? "Gateway"]
        if status?.services.isEmpty ?? true, let line = ConnectorsText.emptyLine(status) { lines.append(line) }
        for s in status?.services ?? [] {
            lines.append("\(s.title): \(ConnectorsText.stateLine(s))\(ConnectorsText.buttonTitle(s).map { " [\($0)]" } ?? "")")
            if let message = s.message, s.state != "connected" { lines.append("  " + message) }
        }
        return lines
    }

    /// The page in the detail area, if one's in place of the thread.
    @MainActor
    private static func homePageDump(_ app: AppModel) -> HomePageInfo? {
        guard app.openProject == nil else { return nil }
        if app.showsScheduledHome {
            let next = ScheduledStats.next(app.schedules)
            let status = next.map { "Next: \(ScheduledStats.title($0)), \(ScheduledStats.when($0, now: app.now))" } ?? "Nothing coming up"
            let tiles = app.schedulesSupported ? ScheduledStats.tiles(app.schedules, now: app.now) : []
            return HomePageInfo(title: "Scheduled", status: status, stats: tiles.map { StatInfo(value: $0.value, label: $0.label) })
        }
        if app.showsProjectsHome { return HomePageInfo(title: "Projects", status: nil, stats: []) }
        if app.showsLibrarianHome {
            let tiles = app.librarian.map { LibrarianStats.tiles($0, projects: app.projects, threads: Array(app.threads.values)) } ?? []
            return HomePageInfo(title: "Librarian", status: app.librarian.map(LibrarianText.state), stats: tiles.map { StatInfo(value: $0.value, label: $0.label) })
        }
        return nil
    }

    /// A schedule's row as ScheduleRowView draws it, in the thread shape:
    /// when it next runs (or Paused, Done), the line under the title, a run
    /// that didn't send or another Mac's Run now still waiting, and the
    /// hover buttons' tips.
    @MainActor
    private static func scheduleRow(_ schedule: Schedule, app: AppModel) -> Thread {
        let hovered = app.hoverOverride == schedule.id
        let away = app.awayRunnerName(schedule)
        let trailing: String
        if schedule.paused {
            trailing = "Paused"
        } else if schedule.isFinished {
            trailing = "Done"
        } else if let next = schedule.nextRunAt {
            trailing = ScheduleTime.relative(next, now: app.now)
        } else {
            trailing = ""
        }
        let place = ([schedule.whenText] + (away.map { [$0] } ?? []) + [app.scheduleTarget(schedule)]).joined(separator: " · ")
        var waiting: String?
        if let away, schedule.runRequestedAt != nil {
            waiting = "Waiting for \(away) to run it"
        } else if let last = schedule.lastRun {
            switch last.outcome {
            case .sent: waiting = nil
            case .failed: waiting = "Last run failed" + (last.note.map { ": \($0)" } ?? "")
            case .skipped: waiting = "Skipped last run" + (last.note.map { ": \($0)" } ?? "")
            }
        }
        var buttons = [away.map { "Ask \($0) to run it now" } ?? "Run now"]
        if !schedule.isFinished { buttons.append(schedule.paused ? "Resume" : "Pause") }
        buttons.append("Delete")
        return Thread(id: schedule.id, title: schedule.title, status: schedule.paused ? "paused" : schedule.isFinished ? "finished" : "active",
                      needsYou: false, unread: false, hovered: hovered, waiting: waiting, place: place, trailing: trailing,
                      hoverButtons: hovered ? buttons : nil)
    }

    /// What ThreadRowView draws for a row, as data: the same rules as its
    /// body and `trailing(_:)`, for the specs (features/sidebar-rows.feature).
    @MainActor
    private static func rowShows(_ summary: ThreadSummary, section: InboxSection?, app: AppModel) -> (place: String, bold: Bool, mark: String?, trailing: String, tile: String, buttons: [String]?) {
        let mode = section.map { RowMode($0, thread: summary, now: app.now) } ?? .inbox
        let look = mode == .inbox || mode == .projects ? ThreadRowLook(summary, section: section) : ThreadRowLook(status: .time, unread: false, emphasised: false)
        let mark: String? = look.done ? "tick" : look.unread ? "dot" : nil
        let tile: String
        if let first = ProjectMark.shown(summary).first {
            tile = ProjectMark.letter(first.title)
        } else {
            tile = summary.isRemote ? "runner" : "folder"
        }
        var buttons: [String]? = nil
        if app.hoverOverride == summary.id {
            buttons = mode.rowActions.map { action in
                switch action {
                case .star: return summary.isStarred ? "Remove star" : "Star: keep at the top"
                case .snooze: return "Hibernate"
                case .archive: return AppShortcuts.shared.tip("Archive", .archive)
                case .unsnooze: return "Wake"
                case .delete: return "Delete chat…"
                case .unarchive: return "Unarchive"
                }
            }
        }
        return (ProjectMark.place(summary), look.emphasised, mark, rowTrailing(summary, look: look, mode: mode, section: section, app: app), tile, buttons)
    }

    /// The words at the right of a row, as `ThreadRowView.trailing` has them.
    @MainActor
    private static func rowTrailing(_ thread: ThreadSummary, look: ThreadRowLook, mode: RowMode, section: InboxSection?, app: AppModel) -> String {
        if let moving = thread.moving {
            if moving.ended { return moving.state == "cancelled" ? "Move cancelled" : "Couldn't move" }
            return "Moving to \(moving.toName)"
        }
        if mode != .hibernating, let waiting = thread.waitingForRoom, !thread.isArchived { return Pools.waitingLine(waiting, now: app.now) }
        if mode != .hibernating, thread.waitingForNetwork != nil, !thread.isArchived { return "Waiting for connection" }
        let time = RelativeTime.short(mode == .archive ? thread.archivedAt ?? thread.lastActivityAt : thread.lastActivityAt, now: app.now)
        switch look.status {
        case .time:
            if mode == .hibernating {
                if let words = WakeWords.row(thread, now: app.now) { return words.text }
                if thread.snoozedUntilRelease { return "Back on update" }
                if thread.snoozedUntilRestart { return "Back on restart" }
                if let until = thread.snoozedUntil { return "Back \(RelativeTime.until(until, now: app.now))" }
                return ""
            }
            if look.woke, thread.woke != nil { return "Woke up" }
            if section == .hibernating, let next = app.wakes[thread.id]?.nextRunAt { return RelativeTime.until(next, now: app.now) }
            return time
        case .working:
            let elapsed = ThreadRowLook.workingLabel(since: thread.lastActivityAt, now: Date())
            return look.labelled ? "Working " + elapsed : elapsed
        case .background: return BackgroundWork.short(thread.background)
        case .needsYou: return time
        case .failed: return "Failed"
        case .stopped: return "Stopped"
        }
    }

    @MainActor
    private static func panels(_ thread: ThreadSummary, app: AppModel) -> Panels? {
        let section = app.paneSection(for: thread)
        var items = [Panels.Item(id: PaneSection.highlightsId, title: "Highlights", kind: "highlights", url: nil, note: nil, active: section == .highlights)]
        items += thread.panels.map {
            Panels.Item(id: $0.id, title: $0.title, kind: $0.kind.rawValue, url: $0.url, note: $0.note, active: section == .panel($0.id))
        }
        if !thread.patches.isEmpty {
            let summary = thread.patches.map { $0.id + ($0.error != nil ? " (error)" : "") }.joined(separator: ", ")
            items.append(Panels.Item(id: PaneSection.patchesId, title: "Patches", kind: "patches", url: nil, note: summary, active: section == .patches))
        }
        let preview: String? = app.panePreview[thread.id].map { preview in
            switch preview.target {
            case .file(let path): "file \(path)" + (preview.editing ? " (editing)" : "") + (preview.away.map { " (from \($0))" } ?? "")
            case .folder(let path): "folder \(path)" + (preview.away.map { " (from \($0))" } ?? "") + (FolderPaneView.current.map { " " + $0.summary } ?? "")
            case .page(let url): "page \(url.absoluteString)"
            case .remote(let path, let failed): "fetching \(path)" + (failed.map { " (failed: \($0))" } ?? "")
            case .explain:
                // What was asked and where each answer is up to, for a script.
                "explain " + (app.explainers[thread.id].map { session in
                    session.title + " | " + session.turns.map { $0.answer ?? $0.error.map { "error: \($0)" } ?? "pending" }.joined(separator: " | ")
                } ?? "(none)")
            }
        }
        return Panels(shown: app.showsPanels(for: thread), width: Int(app.dock.frame(of: section.id)?.width ?? 0), items: items, preview: preview,
                      highlights: highlightsLine(app, thread.id), pane: pane(thread, section: section, app: app))
    }

    /// The pane as drawn: mirrors DockPane, PreviewBody and HighlightsView's
    /// content so a script can read what a person would.
    @MainActor
    private static func pane(_ thread: ThreadSummary, section chosen: PaneSection, app: AppModel) -> Pane {
        // The rail's Files panel isn't one of the sections `paneSection`
        // chooses between, but it's what's showing once it was picked.
        let section = app.panelChoice[thread.id] == BuiltinPanels.files && app.dock.isShowing(BuiltinPanels.files) ? PaneSection(id: BuiltinPanels.files) : chosen
        var pane = Pane(section: section.id)
        pane.folder = FolderPaneView.current?.summary
        if let preview = app.panePreview[thread.id], preview.from == section {
            fillPreview(&pane, preview, thread: thread, app: app)
            return pane
        }
        switch section {
        case .highlights:
            fillHighlights(&pane, thread, app: app)
        case .patches:
            pane.patches = thread.patches.map { patch in
                Pane.Patch(id: patch.id, source: PatchesList.sourceName(patch.source),
                           waiting: patch.untrusted.map { PatchesList.waitingText(patch, untrusted: $0) },
                           trusting: patch.untrusted != nil ? app.isTrusting(patch) : nil,
                           gives: patch.untrusted == nil && patch.error == nil ? PatchesList.gives(patch.id, in: thread) : nil,
                           error: patch.error ?? patch.liveError, file: patch.file)
            }
        case .panel(let id):
            guard id != BuiltinPanels.files, let panel = thread.panels.first(where: { $0.id == id }) else { break }
            switch panel.kind {
            case .list:
                guard let render = PanelDebug.renders[panel.id] else { pane.empty = "Loading…"; break }
                guard let items = render.items, !items.isEmpty else { pane.empty = render.note ?? "Nothing in this list."; break }
                var sections: [Pane.Section] = []
                let headings = PanelItem.headings(items)
                for (index, item) in items.enumerated() {
                    if let heading = headings[index] ?? (sections.isEmpty ? "" : nil) {
                        sections.append(Pane.Section(key: heading, title: heading, count: 0, folded: false, rows: []))
                    }
                    let symbol = item.icon.flatMap { $0.isEmpty ? nil : $0 } ?? item.tone.map(\.rawValue)
                    sections[sections.count - 1].rows.append(Pane.Row(title: item.title, trailing: item.trailing, detail: item.detail, symbol: symbol))
                    sections[sections.count - 1].count += 1
                }
                pane.list = sections
            case .image:
                let image = PanelDebug.images[panel.id]
                pane.image = image.map { "picture \(Int($0.size.width))x\(Int($0.size.height))" } ?? PanelDebug.renders[panel.id]?.note ?? "Taking a screenshot…"
            case .url:
                break
            }
        }
        return pane
    }

    @MainActor
    private static func fillPreview(_ pane: inout Pane, _ preview: PanePreview, thread: ThreadSummary, app: AppModel) {
        switch preview.target {
        case .page(let url):
            pane.previewKind = url.isFileURL ? "web" : "page"
            pane.previewText = url.absoluteString
        case .remote(_, let failed):
            pane.previewKind = failed == nil ? "fetching" : "note"
            pane.previewText = failed.map { _ in "Couldn't get it from \(preview.away ?? "that Mac")" }
        case .explain:
            pane.previewKind = "note"
        case .folder(let path):
            pane.previewKind = "folder"
            pane.previewText = path
        case .file(let path):
            if !FileManager.default.fileExists(atPath: path) {
                pane.previewKind = "note"
                pane.previewText = "This file isn't there any more."
            } else if PreviewRules.viewer(forPath: path) == .web {
                pane.previewKind = "web"
                pane.previewText = path
            } else if TextFileView.shows(path) {
                // The rendered text, once its controller is up for this file.
                pane.previewKind = PreviewRules.isMarkdown(path) ? (preview.editing ? "text" : "markdown") : (PreviewRules.codeLanguage(forPath: path) != nil ? "code" : "text")
                if let controller = TextFileController.current, controller.url.path == path {
                    pane.previewText = String(controller.textView.string.prefix(4000))
                    pane.previewFind = controller.debugFindLine
                    pane.previewNote = controller.conflict != nil ? "Changed on disk while you were editing." : controller.note
                }
                // What the bar offers (PreviewButtons): the Markdown menu, or
                // the pencil and Done, for a file here that can be edited.
                if preview.away == nil, TextFileView.canEdit(path) {
                    pane.previewMode = PreviewRules.isMarkdown(path) ? (preview.editing ? "markdown" : "preview") : (preview.editing ? "editing" : "reading")
                }
            } else {
                let shows = FilePreviewView.current?.debugShows ?? "quickLook"
                if shows.hasPrefix("picture") {
                    pane.previewKind = "picture"
                    pane.previewText = String(shows.dropFirst("picture ".count))
                } else if shows.hasPrefix("video") {
                    // The film's size once its track has said it, as the
                    // GPUI app's dump has it.
                    pane.previewKind = "video"
                    pane.previewText = shows == "video" ? nil : String(shows.dropFirst("video ".count))
                } else {
                    pane.previewKind = shows
                }
            }
        }
    }

    /// HighlightsView's content, as rows.
    @MainActor
    private static func fillHighlights(_ pane: inout Pane, _ thread: ThreadSummary, app: AppModel) {
        if app.highlightsUnsupported { pane.empty = "This runner doesn't keep highlights yet."; return }
        guard let fetched = app.highlights[thread.id] else { return }
        let sections = HighlightSections.build(fetched)
        let projects = HighlightProjects.build(thread: thread, project: app.project)
        let tools = Set(thread.patches.filter { !($0.matched ?? []).isEmpty }.map(\.id))
        let toolPanels = thread.panels.filter { tools.contains($0.patch) }
        let scheduled = ScheduleRows.rows(for: thread, in: app.schedules, now: app.now, target: app.scheduleTarget)
        let started = StartedThreadRows.rows(for: thread, in: app.threads, now: app.now)
        let asks = thread.asks
        if sections.isEmpty && asks.isEmpty && projects.isEmpty && toolPanels.isEmpty && scheduled.isEmpty && thread.subagents.isEmpty && started.isEmpty {
            pane.empty = "Nothing here yet. Files, links and commits from this thread show up here as it works."
            return
        }
        let flags = HighlightsFlags.current
        var out: [Pane.Section] = []
        func add(_ key: String, _ title: String, count: Int, rows: @autoclosure () -> [Pane.Row]) {
            let folded = app.collapsedHighlights.contains(key)
            out.append(Pane.Section(key: key, title: title, count: count, folded: folded, rows: folded ? [] : rows()))
        }
        if !toolPanels.isEmpty { pane.cards = toolPanels.map { [$0.title, $0.summary ?? $0.note].compactMap { $0 }.joined(separator: ": ") } }
        if let brief = sections.brief { pane.brief = HighlightSections.points(brief) }
        if !asks.isEmpty {
            add(HighlightFold.needsYou, "Needs you", count: asks.filter { !$0.ticked }.count,
                rows: asks.map { Pane.Row(title: $0.plainAction, detail: $0.plainWhere, symbol: $0.ticked ? "checkmark.square.fill" : "square", ticked: $0.ticked) })
        }
        for row in projects {
            let tasks = row.project.tasks.flatMap { $0.loading ? nil : $0 }
            let key = HighlightFold.project(row.project.id)
            let count = row.project.links.count + (tasks?.openCount ?? 0)
            add(key, row.project.title, count: count, rows: row.project.links.map {
                let address = HighlightText.address($0.url)
                return Pane.Row(title: $0.title, detail: address == $0.title ? nil : address, symbol: "globe")
            })
        }
        if !scheduled.isEmpty {
            add(HighlightFold.scheduled, "Scheduled", count: scheduled.count,
                rows: scheduled.map { Pane.Row(title: $0.title, trailing: $0.when, detail: $0.detail, symbol: $0.paused ? "pause.circle" : "clock") })
        }
        if !thread.subagents.isEmpty {
            let all = SubagentRows.highlights(thread)
            let shown = HighlightSections.capped(all, limit: HighlightSections.subagentLimit, showAll: flags?.allSubagents ?? false)
            add(HighlightFold.subagents, "Sub-agents", count: all.count, rows: shown.shown.map { subagent in
                let icon: String = switch subagent.status {
                case .running: "circle.dashed"
                case .completed: "checkmark"
                case .failed: "xmark"
                case .stopped: "minus"
                }
                return Pane.Row(title: SubagentRows.title(subagent), trailing: SubagentRows.duration(subagent, now: app.now), detail: SubagentRows.detail(subagent), symbol: icon)
            } + (shown.more > 0 ? [Pane.Row(title: "Show all \(all.count)", symbol: "ellipsis")] : []))
        }
        if !started.isEmpty {
            let shown = HighlightSections.capped(started, limit: HighlightSections.subagentLimit, showAll: flags?.allStarted ?? false)
            add(HighlightFold.startedThreads, "Threads it started", count: started.count, rows: shown.shown.map {
                Pane.Row(title: $0.thread.title, detail: StartedThreadRows.detail($0, now: app.now), symbol: StartedThreadRows.icon($0.state))
            } + (shown.more > 0 ? [Pane.Row(title: "Show all \(started.count)", symbol: "ellipsis")] : []))
        }
        func item(_ h: Highlight) -> Pane.Row {
            let isCommit = h.kind == .commit
            let detail: String? = if let path = h.path {
                HighlightText.folder(of: path, cwd: thread.cwd)
            } else if isCommit {
                HighlightText.commitLine(h)
            } else {
                h.url.map(HighlightText.address).flatMap { $0 == h.title ? nil : $0 }
            }
            let how = h.isPinned || isCommit ? nil : h.how.word
            return Pane.Row(title: h.title, lead: isCommit ? h.sha : nil, trailing: how.flatMap { $0.isEmpty ? nil : $0 }, note: h.note, detail: detail, symbol: FileIcon.symbol(for: h))
        }
        if !sections.pinned.isEmpty { add(HighlightFold.pinned, "Pinned", count: sections.pinned.count, rows: sections.pinned.map(item)) }
        if !sections.files.isEmpty { add(HighlightFold.files, "Files", count: sections.files.count, rows: sections.files.map(item)) }
        if !sections.links.isEmpty {
            let links = HighlightSections.capped(sections.links, limit: HighlightSections.linkLimit, showAll: flags?.allLinks ?? false)
            add(HighlightFold.links, "Links", count: sections.links.count, rows: links.shown.map(item) + (links.more > 0 ? [Pane.Row(title: "Show all \(sections.links.count)", symbol: "ellipsis")] : []))
        }
        if !sections.commits.isEmpty || !sections.edited.isEmpty {
            let commits = HighlightSections.capped(sections.commits, limit: HighlightSections.commitLimit, showAll: flags?.allCommits ?? false)
            var rows = commits.shown.map(item)
            if commits.more > 0 { rows.append(Pane.Row(title: "Show all \(sections.commits.count)", symbol: "ellipsis")) }
            if !sections.edited.isEmpty {
                let open = flags?.editedOpen ?? false
                rows.append(Pane.Row(title: HighlightText.editedCount(sections.edited.count), symbol: open ? "chevron.down" : "chevron.right"))
                if open {
                    rows += sections.edited.map { Pane.Row(title: ($0 as NSString).lastPathComponent, detail: HighlightText.folder(of: $0, cwd: thread.cwd), symbol: FileIcon.symbol(forPath: $0)) }
                }
            }
            add(HighlightFold.changes, "Changes", count: sections.commits.count + sections.edited.count, rows: rows)
        }
        pane.sections = out
    }

    @MainActor
    private static func highlightsLine(_ app: AppModel, _ threadId: String) -> String {
        if app.highlightsUnsupported { return "not kept by this runner" }
        guard let fetched = app.highlights[threadId] else { return "not fetched" }
        let sections = HighlightSections.build(fetched)
        if sections.isEmpty { return "empty" }
        let parts = [("pinned", sections.pinned.count), ("files", sections.files.count), ("links", sections.links.count),
                     ("commits", sections.commits.count), ("edited", sections.edited.count)]
        let counts = parts.filter { $0.1 > 0 }.map { "\($0.1) \($0.0)" }
        let folded = app.collapsedHighlights.isEmpty ? [] : ["folded: \(app.collapsedHighlights.sorted().joined(separator: " "))"]
        return ((sections.brief != nil ? ["a brief"] : []) + counts + folded).joined(separator: ", ")
    }

    /// The Usage panel and its icon, part by part, as `UsagePanel` and
    /// `UsageActivityButton` work them out, for the specs (features/usage.feature)
    /// and the GPUI app's dump of the same shape.
    @MainActor
    static func usage(_ app: AppModel) -> Usage {
        let now = app.now
        let accounts = app.accounts
        let groups = UsageGroups.build(pools: app.poolsSupported ? app.pools : [], accounts: accounts)
        // Filled to the room on the login the next new thread would use.
        let login = app.newThreadAccountId
        let level = HoneyLevel.level(app.draftLogins.account(login)?.usage)
        var out = Usage(shown: app.usageShown, lines: usageLines(app),
                        icon: UsageIcon(percent: Int((level * 100).rounded()), login: login, tip: UsageGroups.tooltip(groups, now: now)))
        let ids = accounts.map(\.id)
        if let stats = app.usageStats {
            let calendar = Calendar.current
            let today = UsageStatsMath.today(stats, accounts: ids, now: now)
            let usual = UsageStatsMath.usualByNow(stats, accounts: ids, now: now)
            let week = UsageStatsMath.total(stats, accounts: ids, from: calendar.date(byAdding: .day, value: -6, to: calendar.startOfDay(for: now)) ?? now, to: now.addingTimeInterval(1))
            let hoursToday = max(1, now.timeIntervalSince(calendar.startOfDay(for: now)) / 3600)
            out.tiles = [
                UsageTile(title: "Today", value: UsageStatsMath.tokens(today.total), detail: usual.map { "usually " + UsageStatsMath.tokens($0) }),
                UsageTile(title: "Last hour", value: UsageStatsMath.tokens(UsageStatsMath.lastHour(stats, accounts: ids, now: now)),
                          detail: UsageStatsMath.tokens(Int(saturating: Double(today.total) / hoursToday)) + "/h today"),
                UsageTile(title: "7 days", value: UsageStatsMath.tokens(week.total), detail: UsageStatsMath.tokens(week.total / 7) + " a day"),
            ]
            // The chart's range is the panel's @AppStorage.
            let byDay = UserDefaults.standard.string(forKey: "usage.chartRange") == "week"
            let bars = byDay ? UsageStatsMath.byDay(stats, accounts: ids, days: 7, now: now) : UsageStatsMath.todayByHour(stats, accounts: ids, now: now)
            let spending = ids.filter { id in bars.contains { $0.accountId == id } }
            out.chart = UsageChart(title: byDay ? "Tokens by day" : "Tokens by hour", range: byDay ? "week" : "today",
                                   key: spending.map { AccountName.login($0, accounts: accounts) },
                                   empty: bars.isEmpty ? (stats.ready ? "Nothing spent yet" : "Reading the transcripts…") : nil)
        }
        let notices = AccountUsageText.notices(accounts)
        // The Settings card's switch is the panel's @AppStorage, on until
        // it's turned off.
        let showModelWeeks = UserDefaults.standard.object(forKey: UsagePanel.showModelWeeksKey) as? Bool ?? true
        out.cards = groups.map { group in
            let today = app.usageStats.map { UsageStatsMath.today($0, accounts: group.accounts.map(\.id), now: now) }
            let logins = group.providers.flatMap { section in
                section.accounts.map { account in
                    usageLogin(account, provider: group.providers.count > 1 ? ModelPicker.providerTitle(section.provider) : nil,
                               notice: notices.own[account.id], stats: app.usageStats, showModelWeeks: showModelWeeks, now: now)
                }
            }
            return UsageCard(title: group.title, today: today.flatMap { $0.total > 0 ? UsageStatsMath.tokens($0.total) + " today" : nil }, logins: logins)
        }
        let modelWeeks = UsagePanel.modelWeekTitles(accounts)
        if !modelWeeks.isEmpty { out.settings = UsageSettingsInfo(switch: "Show " + modelWeeks.joined(separator: ", "), on: showModelWeeks) }
        if !accounts.contains(where: { $0.usage != nil }) { out.notes.append("The usage numbers need a newer runner.") }
        if let stats = app.usageStats, !stats.ready { out.notes.append("Still reading the transcripts, so these are partial.") }
        let source = [AccountUsageText.sourceLine(accounts, now: now), notices.shared].compactMap { $0 }
        out.footer = source.isEmpty ? nil : source.joined(separator: ". ")
        return out
    }

    /// One login's row as `LoginRow` draws it: its meters (the main windows,
    /// then a model's own week unless the switch is off) with what each says
    /// to do, then the quiet lines under them.
    private static func usageLogin(_ account: Account, provider: String?, notice: String?, stats: UsageStats?, showModelWeeks: Bool, now: Date) -> UsageLogin {
        let usage = account.usage
        let today = stats.map { UsageStatsMath.today($0, accounts: [account.id], now: now) }
        let stale = usage?.stale == true
        let meters = (AccountUsageText.mainWindows(usage) + (showModelWeeks ? AccountUsageText.modelWeeks(usage) : [])).map { window in
            let samples = stats?.samples(account.id, kind: window.kind, title: window.title) ?? []
            return UsageMeter(title: window.title, left: AccountUsageText.percent(window.remainingPercent) + " left",
                              verdict: LimitPace.verdict(window, pace: LimitPace.of(window, samples: samples, now: now), stale: stale, now: now)?.text)
        }
        var more: [String] = []
        if stale, let readAt = usage?.readAt { more.append("Numbers from " + AccountUsageText.when(readAt, now: now)) }
        more += AccountUsageText.extraLines(usage, notice: notice, now: now)
        let status = usage == nil ? "Nothing known yet" : (usage?.blocked == true ? "Limit reached" : nil)
        return UsageLogin(name: AccountName.title(account), provider: provider, note: account.note.flatMap { $0.isEmpty ? nil : $0 },
                          today: today.flatMap { $0.total > 0 ? UsageStatsMath.tokens($0.total) + " today" : nil },
                          status: status, meters: meters, more: more)
    }

    /// What the Usage panel says: each login's room by pool and provider
    /// (`UsagePanel`), then the CLIs from the Updates panel, as the
    /// automation's `usage` presses their buttons.
    @MainActor
    static func usageLines(_ app: AppModel) -> [String] {
        var lines: [String] = []
        // The spend at the top, as the tiles say it.
        if let stats = app.usageStats {
            let ids = app.accounts.map(\.id)
            var spend = "Today " + UsageStatsMath.tokens(UsageStatsMath.today(stats, accounts: ids, now: app.now).total)
            spend += ", last hour " + UsageStatsMath.tokens(UsageStatsMath.lastHour(stats, accounts: ids, now: app.now))
            if !stats.ready { spend += " (still reading)" }
            lines.append(spend)
        }
        let groups = UsageGroups.build(pools: app.poolsSupported ? app.pools : [], accounts: app.accounts)
        let pooled = groups.contains { $0.pool != nil }
        for group in groups {
            if pooled { lines.append("# " + group.title) }
            for section in group.providers {
            lines.append((pooled ? "## " : "# ") + ModelPicker.providerTitle(section.provider))
            for account in section.accounts {
                var line = AccountName.title(account)
                if let plan = account.plan { line += " (\(plan))" }
                if let note = account.note { line += ", " + note }
                if let usage = account.usage {
                    if usage.blocked { line += ", limit reached" }
                    if usage.stale { line += ", stale" }
                    let windows = usage.windows.map { "\($0.title) \(AccountUsageText.percent($0.remainingPercent))" }
                    if !windows.isEmpty { line += ": " + windows.joined(separator: ", ") }
                    if let notice = usage.notice { line += " [\(notice)]" }
                } else {
                    line += ": nothing known"
                }
                lines.append(line)
            }
            }
        }
        if let source = AccountUsageText.sourceLine(app.accounts, now: app.now) { lines.append(source) }
        lines.append("# CLIs")
        if app.clisSupported == false || (app.clisSupported == nil && app.clis.isEmpty) {
            lines.append("needs a newer runner")
        } else {
            for cli in app.clis {
                var line = CliText.statusLine(cli) + " (" + CliText.policyTitle(cli.policy).lowercased() + ")"
                if let button = CliText.updateTitle(cli) { line += " [\(button)]" }
                if let back = CliText.goBackTitle(cli) { line += " [\(back)]" }
                if CliText.isUpdating(cli), let text = CliText.bannerText(cli) { line += ": " + text }
                lines.append(line)
            }
        }
        return lines
    }

    /// What's in an ask's Something else box, quoted, with its pictures.
    private static func ownAnswer(_ note: AskNote?) -> String {
        let note = note ?? AskNote()
        let pictures = note.images.isEmpty ? "" : " +\(note.images.count) picture\(note.images.count == 1 ? "" : "s")"
        return "\"\(note.text)\"" + pictures
    }

    /// The threads with decisions on a project's page, as it lists them.
    @MainActor
    static func projectDecisionThreads(_ project: Project, app: AppModel) -> [ThreadSummary] {
        let built = app.projectSections
        return ProjectDecisions.threads(built.sections.first { $0.id == project.id }.map(built.threadsWithParts) ?? [])
    }

    @MainActor
    static func projectDecisionLines(_ project: Project, app: AppModel) -> [String]? {
        guard app.projectDecisionsOn else { return nil }
        return projectDecisionThreads(project, app: app).map { thread in
            let asks = thread.asks.filter { !$0.ticked }.map(\.plainAction).joined(separator: " | ")
            let sending = ProjectDecisions.isSending(thread, sentFor: app.decisionSends[thread.id]?.asksSeq)
            return "\(thread.title): \(asks)" + (sending ? " (sending)" : "")
        }
    }

    /// "2 picked", then each ask with its choices, "*" on the agent's
    /// pick, its links and the answer picked.
    @MainActor
    static func decisions(_ thread: ThreadSummary, app: AppModel) -> [String]? {
        guard Decisions.isOpen(thread, card: app.sessions[thread.id]?.openCard) else { return nil }
        let picks = app.picks(for: thread)
        let notes = app.notes(for: thread)
        var lines = ["\(picks.count) picked"]
        for (i, ask) in thread.asks.enumerated() {
            let options = Decisions.choices(for: ask)
            // Each with what it does in brackets, when the agent said (optionNotes).
            let choices = options.enumerated().map { i, label in
                let note = ask.optionNotes.indices.contains(i) && ask.optionNotes.count == ask.options.count ? ask.optionNotes[i].trimmingCharacters(in: .whitespacesAndNewlines) : ""
                return (i == ask.suggested ? "*" : "") + label + (note.isEmpty ? "" : " (\(note))")
            }
            let links = ask.links.isEmpty ? "" : " links: " + ask.links.map(\.title).joined(separator: ", ")
            let answer = picks[i] == Decisions.ownAnswer ? "something else: " + ownAnswer(notes[i]) : picks[i].flatMap { options.indices.contains($0) ? options[$0] : nil } ?? "no answer"
            lines.append("  \(i + 1). \(ask.action) [\(choices.joined(separator: " | "))]\(links) -> \(answer)")
        }
        return lines
    }

    @MainActor
    private static func composer(_ app: AppModel, key: String) -> Composer {
        let settings: Settings
        if let thread = app.selectedThread {
            // A thread's model is its own, or what its last turn ran on,
            // or the account's default.
            let model = thread.model ?? thread.modelInUse.map { "default (the account's, \($0))" } ?? defaultModel(app, account: thread.accountId)
            settings = Settings(cwd: thread.cwd, account: thread.accountId, model: model, ask: thread.permissionMode == .ask,
                                mode: thread.permissionMode, effort: thread.effort ?? thread.effortInUse.map { "default (\($0))" } ?? "default",
                                pool: app.usesPools ? thread.poolId : nil)
        } else {
            settings = newThreadSettings(app)
        }
        // Chips as the [Picture N] markers they'll go as, not invisible marks.
        return Composer(draft: key, text: InlinePictures.messageText(app.drafts[key]?.normalized.text ?? ""), images: app.drafts[key]?.images.count ?? 0,
                        editing: app.editing[key]?.seq,
                        cwd: settings.cwd, account: settings.account, model: settings.model, ask: settings.ask, mode: settings.mode, effort: settings.effort,
                        pool: settings.pool, chip: ThreadSettings(app: app, thread: app.selectedThread).modelChipTitle,
                        chipBadge: badgeWords(ThreadSettings(app: app, thread: app.selectedThread).modelChipBadge),
                        runner: app.selectedThread.flatMap(app.runnerName(of:)) ?? (app.selectedThread == nil ? app.newThreadRunner?.name : nil),
                        card: cardLines(app, key: key), cardRuns: cardRunLines(app, key: key), creating: key == "new" && app.creatingThread,
                        answers: answerLines(app, key: key))
    }

    /// LimitBanner's words and button, as it draws them.
    @MainActor private static func limitInfo(_ app: AppModel, thread: ThreadSummary) -> LimitInfo? {
        guard let advice = app.limitAdvice(for: thread) else { return nil }
        let waiting = thread.waitingForRoom != nil && app.usesPools
        let button = advice.target.map { target -> String in
            let title = AccountUsageText.switchTitle(target, accounts: app.accounts)
            return waiting ? title.replacingOccurrences(of: "Switch to", with: "Pin to") : title
        }
        return LimitInfo(text: advice.text, button: button, carryOn: advice.stopped ? (thread.carryOnAtReset ? "Yes" : "No") : nil)
    }

    /// A chip's badge as it reads after the title.
    private static func badgeWords(_ badge: AccountUsageText.ChipBadge?) -> String? {
        switch badge {
        case .low(let percent)?: return "· " + percent
        case .blocked?: return "Limit reached"
        case nil: return nil
        }
    }

    /// The open card as lines (`composer.card`): "Done" with a "[x]" or
    /// "[ ]" line per follow-up, "(?)" on one asked about; or "Needs you"
    /// ("(collapsed)" when folded) with a line per ask, and under an ask
    /// with a command to run, "    $ <command>" (a line of its own, so a
    /// spec matching an ask's line by its end still finds it).
    @MainActor
    private static func cardLines(_ app: AppModel, key: String) -> [String]? {
        guard let row = app.sessions[key]?.openCard, case .asks(let card) = row.content else { return nil }
        return cardLines(card, collapsed: app.collapsedCards[key] == row.seq)
    }

    /// What RowRenderer.appendRun's button says for each of the open card's
    /// commands (`cardRuns`); the GPUI app's dump says the same.
    @MainActor
    private static func cardRunLines(_ app: AppModel, key: String) -> [String]? {
        guard let row = app.sessions[key]?.openCard, case .asks(let card) = row.content, !card.isDone else { return nil }
        let lines = card.asks.compactMap { ask -> String? in
            guard let run = ask.run, let id = run.buttonId else { return nil }
            let button = ReplyButtons.shared.known(id)?.button
            let failed = button.map { $0.failed?.error } ?? run.failed
            let title: String
            if button?.pressed != nil {
                title = "✓ Ran"
            } else if ReplyButtons.shared.isPressing(id) || button?.running != nil || (button == nil && run.running == true) {
                title = "Running…"
            } else {
                title = failed == nil ? "▶ Run" : "▶ Run again"
            }
            return "\(title): \(run.command)"
        }
        return lines.isEmpty ? nil : lines
    }

    static func cardLines(_ card: AsksCard, collapsed: Bool) -> [String] {
        var lines: [String]
        if card.isDone {
            lines = ["Done"] + card.next.enumerated().map { i, text in
                (card.pickedNext.contains(i) ? "[x] " : "[ ] ") + text + (card.explainNext.contains(i) ? " (?)" : "")
            }
        } else {
            lines = [collapsed ? "Needs you (collapsed)" : "Needs you"]
            for (i, ask) in card.asks.enumerated() {
                lines.append((card.picks[i] != nil ? "[x] " : "[ ] ") + "\(i + 1). \(ask.action)")
                if let run = ask.run { lines.append("    $ " + run.command) }
            }
        }
        // Another lab's review (RowRenderer.reviewText): its heading, then
        // "  Fixed: <claim>" a finding, as the GPUI app's dump says them.
        if let review = card.review {
            lines.append(review.heading)
            lines += review.findings.map { finding in
                let word = switch finding.outcome {
                case "fixed": "Fixed"
                case "disputed": "Disputed"
                case "left": "Left for you"
                default: "No answer"
                }
                return "  \(word): \(finding.claim)"
            }
        }
        return lines
    }

    /// The picks shown as chips over the box while the card's folded
    /// (AnswerChips), as `composer.answers`: "1: done", "2: Just us". Nil
    /// while the card's open above (its buttons show them) or a message is
    /// being edited.
    @MainActor
    private static func answerLines(_ app: AppModel, key: String) -> [String]? {
        guard let thread = app.threads[key], app.editing[key] == nil else { return nil }
        let picks = app.picks(for: thread)
        guard !picks.isEmpty, let row = app.sessions[key]?.openCard, case .asks(let card) = row.content, !card.isDone,
              app.collapsedCards[key] == row.seq else { return nil }
        let notes = app.notes(for: thread)
        let lines = picks.keys.sorted().compactMap { index -> String? in
            guard thread.asks.indices.contains(index), let pick = picks[index] else { return nil }
            let ask = thread.asks[index]
            if pick == Decisions.ownAnswer {
                let note = notes[index] ?? AskNote()
                let text = InlinePictures.words(note.text).trimmingCharacters(in: .whitespacesAndNewlines)
                let shown = !text.isEmpty ? text : note.images.isEmpty ? "Your answer above" : note.images.count == 1 ? "1 picture" : "\(note.images.count) pictures"
                return "\(index + 1): \(shown)"
            }
            let options = Decisions.choices(for: ask)
            return options.indices.contains(pick) ? "\(index + 1): \(options[pick])" : nil
        }
        return lines.isEmpty ? nil : lines
    }

    /// Rows a click opens: work lines, groups and settled approvals that
    /// have something to show.
    static func opens(_ row: TranscriptRow) -> Bool {
        switch row.content {
        case .group, .tool, .reasoning: return true
        case .approval(let item): return item.resolution != nil && item.opens
        default: return false
        }
    }

    /// The window's content size, as `--window-size` and `resize` set it
    /// (the toolbar included).
    static func contentSize(_ window: NSWindow) -> NSSize {
        window.contentRect(forFrameRect: window.frame).size
    }

    func json(pretty: Bool) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = pretty ? [.sortedKeys, .prettyPrinted, .withoutEscapingSlashes] : [.sortedKeys, .withoutEscapingSlashes]
        let data = (try? encoder.encode(self)) ?? Data()
        return String(decoding: data, as: UTF8.self) + (pretty ? "\n" : "")
    }

    /// For reading: one thing per line, rows in a table, `>` marking the
    /// rows on screen.
    var text: String {
        var lines: [String] = []
        var head = "\(instance.map { "dev app \($0)" } ?? "app"): \(connection)"
        if !runnerVersion.isEmpty { head += ", runner \(runnerVersion)" }
        if let window { head += ", window \(window.width)x\(window.height) \(window.appearance)" }
        lines.append(head)
        if let banner { lines.append("banner (\(banner.style)): \(banner.text)" + (banner.detail.map { "  [\($0)]" } ?? "")) }
        if let archivedNote { lines.append("archived note: \(archivedNote), with Undo") }
        if let milestoneNote { lines.append("milestone note: \(milestoneNote)") }
        if let error { lines.append("error banner: \(error)") }
        lines.append("")
        lines.append("sidebar, \(sidebar.mode) (\(sidebar.snoozed) snoozed, \(sidebar.archived) archived):")
        // Columns as wide as the longest id: a real runner's ids are 36
        // character UUIDs, and fixed widths pushed every later column out
        // of line.
        let threadWidth = max(12, sidebar.sections.flatMap(\.threads).map(\.id.count).max() ?? 0)
        for section in sidebar.sections where !section.threads.isEmpty {
            lines.append("  \(section.name)" + (section.collapsed == true ? " (collapsed)" : ""))
            for thread in section.threads {
                var flags = [thread.status]
                if thread.unread { flags.append("unread") }
                if thread.hovered { flags.append("hovered") }
                if let background = thread.background { flags.append("bg: " + background) }
                if let waiting = thread.waiting { flags.append(waiting) }
                if let runner = thread.runner { flags.append(runner) }
                let mark = thread.id == selection?.id ? ">" : " "
                lines.append("  \(mark) \(pad(thread.id, threadWidth)) \(pad(flags.joined(separator: " "), 16)) \(thread.title)")
                for sub in thread.subagents {
                    let indent = String(repeating: "  ", count: sub.depth + 1)
                    lines.append("  \(sub.selected ? ">" : " ") \(indent)sub \(sub.id) [\(sub.status)] \(sub.title): \(sub.detail)")
                }
            }
        }
        lines.append("")
        if let selection {
            lines.append("selection: \(selection.id) \"\(selection.title)\" (\(selection.status))" + (subagent.map { ", showing sub-agent \($0)" } ?? ""))
        } else {
            lines.append("selection: none (new thread)")
        }
        if let panels {
            let items = panels.items.map { item in
                var text = item.title + (item.active ? "*" : "") + " (\(item.kind)"
                // A data: URL (the mock's page) is thousands of characters.
                if let url = item.url { text += " " + (url.count > 80 ? String(url.prefix(77)) + "…" : url) }
                text += ")"
                if let note = item.note { text += " \"\(note)\"" }
                return text
            }
            lines.append("panels (\(panels.shown ? "shown, \(panels.width) wide" : "hidden")): " + items.joined(separator: ", "))
            if let preview = panels.preview { lines.append("pane preview: \(preview)") }
            if let highlights = panels.highlights { lines.append("highlights: \(highlights)") }
        }
        if !actions.isEmpty {
            lines.append("actions: " + actions.map { "\($0.id) \"\($0.title)\" (\($0.kind)\($0.running ? ", running" : ""))" }.joined(separator: ", "))
        }
        if let usage, usage.shown {
            lines.append("usage popover:")
            lines += usage.lines.map { "  " + $0 }
        }
        if let decisions { lines.append("decisions: " + decisions[0]); lines += decisions.dropFirst().map { "  " + $0 } }
        // The open card as it reads in the box, the done card's follow-ups too.
        if let card = composer.card, let first = card.first { lines.append("card: " + first); lines += card.dropFirst().map { "  " + $0 } }
        lines.append("composer (\(composer.draft)): \"\(composer.text)\"" + (composer.images > 0 ? " and \(composer.images) image(s)" : "")
            + (composer.editing.map { ", editing the message at seq \($0)" } ?? ""))
        let on = composer.pool.map { "pool \($0) (now \(composer.account))" } ?? (pools.isEmpty || composer.runner != nil ? composer.account : "\(composer.account), pinned")
        lines.append("  \(composer.draft == "new" ? "starts" : "runs") in \(composer.cwd)\(composer.runner.map { " on \($0)" } ?? "") on \(on), model \(composer.model), "
            + (composer.effort == "default" ? "" : "effort \(composer.effort), ") + Self.modeText(composer.mode))
        lines.append("  model chip: \(composer.chip)")
        lines.append("dock: \(dock)")
        if !pools.isEmpty {
            lines.append("pools:")
            for pool in pools {
                var line = "  \(pool.id) \"\(pool.title)\" \(pool.rail): " + pool.logins.joined(separator: ", ")
                if !pool.folders.isEmpty { line += " [folders: " + pool.folders.joined(separator: ", ") + "]" }
                if let status = pool.status { line += " (\(status))" }
                lines.append(line)
            }
        }
        if let transcript {
            var about = "\(transcript.rows.count) rows, \(transcript.state)"
            if transcript.hasMoreAbove { about += ", more above" }
            about += transcript.atBottom ? ", at the bottom" : ", scrolled up"
            lines.append("")
            lines.append("transcript \(transcript.threadId): \(about)")
            let ids = transcript.rows.map { ($0.inGroup ? "  " : "") + $0.id }
            let rowWidth = max(12, ids.map(\.count).max() ?? 0)
            for (row, id) in zip(transcript.rows, ids) {
                let open = row.expanded.map { $0 ? "open" : "closed" } ?? ""
                lines.append("\(row.onScreen ? ">" : " ") \(pad(id, rowWidth)) \(pad(row.kind, 10)) \(pad(String(row.height), 5)) \(pad(open, 7)) \(row.text)")
                for button in row.buttons ?? [] { lines.append("  \(pad("", rowWidth)) button     \(button)") }
            }
        }
        return lines.joined(separator: "\n") + "\n"
    }

    /// The pills a reply's buttons show as, for `dump`.
    static func buttons(_ row: TranscriptRow) -> [String]? {
        guard case .assistant(let item) = row.content else { return nil }
        let ids = ReplyButtons.ids(in: item.text)
        guard !ids.isEmpty else { return nil }
        return ids.map { id in
            guard let known = ReplyButtons.shared.known(id) else { return "unknown \(id)" }
            let look = ReplyButtons.look(known.button, pressing: ReplyButtons.shared.isPressing(id))
            return "\(ReplyButtons.label(known.button, look: look)) [\(id), \(look.rawValue)]"
        }
    }

    /// The chips a row draws among its words, read off its text as drawn:
    /// Mike's bubble's pictures, cards and threads (RowRenderer.userText),
    /// and the thread chips of an agent's message or a reply (ThreadChips).
    static func chips(_ row: TranscriptRow) -> [String]? {
        if case .assistant(let item) = row.content { return threadChips(MarkdownRenderer.render(item.text).string) }
        guard case .user(let message) = row.content, !message.showsAsNote else { return nil }
        if let header = message.agentHeader { return threadChips(RowRenderer.agentMessageText(header)) }
        let text = RowRenderer.userText(message)
        var found: [String] = []
        text.enumerateAttributes(in: NSRange(location: 0, length: text.length)) { attributes, _, _ in
            if let index = attributes[.tbPictureChip] as? NSNumber {
                found.append("picture: \(PictureChip.label(index.intValue + 1))")
            } else if let title = attributes[.tbCardChip] as? String {
                found.append("\((attributes[.link] as? URL)?.scheme == "thread" ? "thread" : "card"): \(title)")
            }
        }
        return found.isEmpty ? nil : found
    }

    /// The thread chips in drawn text, each as its words ("thread: 85735a8f").
    private static func threadChips(_ text: NSAttributedString) -> [String]? {
        let found = ThreadChips.chips(in: text).map { "thread: \((text.string as NSString).substring(with: $0.range))" }
        return found.isEmpty ? nil : found
    }

    /// The rich boxes a reply draws (RichBlockCell), read off its rendered
    /// text, in order.
    static func rich(_ row: TranscriptRow) -> [RichInfo]? {
        guard case .assistant(let item) = row.content, RichBlocks.mayHoldRich(item.text) else { return nil }
        let text = MarkdownRenderer.render(item.text).string
        var found: [RichInfo] = []
        text.enumerateAttribute(.attachment, in: NSRange(location: 0, length: text.length)) { value, _, _ in
            guard let cell = (value as? NSTextAttachment)?.attachmentCell as? RichBlockCell else { return }
            var state = "drawing", size: String?
            switch cell.blockState {
            case .drawing: state = "drawing"
            case .drawn(let drawn):
                state = "drawn"
                size = "\(Int(drawn.size.width.rounded()))x\(Int(drawn.size.height.rounded()))"
            case .failed(let why): state = "failed: \(why)"
            }
            found.append(RichInfo(kind: cell.block.kind.rawValue, shows: cell.showsPicture ? "drawn" : "source", state: state, asks: false,
                                  copy: "Copy", size: size))
        }
        return found.isEmpty ? nil : found
    }

    /// The drafts a reply draws as boxes, read off its rendered text: the
    /// header's cell, then the box's lines up to where its fill ends.
    static func drafts(_ row: TranscriptRow) -> [DraftInfo]? {
        guard case .assistant(let item) = row.content, DraftBlock.mayHold(item.text) else { return nil }
        let text = MarkdownRenderer.render(item.text).string
        let string = text.string as NSString
        let whole = NSRange(location: 0, length: text.length)
        var found: [DraftInfo] = []
        text.enumerateAttribute(.attachment, in: whole) { value, range, _ in
            guard let cell = (value as? NSTextAttachment)?.attachmentCell as? DraftBlockCell else { return }
            var box = NSRange()
            _ = text.attribute(.tbBlockBackground, at: range.location, longestEffectiveRange: &box, in: whole)
            // After the header's own line break; each line ends in one, but
            // the reply's last.
            let end = NSMaxRange(box)
            var at = NSMaxRange(range) + 1
            var lines: [String] = []
            var styled: [String] = []
            while at < end {
                let next = string.range(of: "\n", options: [], range: NSRange(location: at, length: end - at))
                let line = NSRange(location: at, length: (next.location == NSNotFound ? end : next.location) - at)
                lines.append(string.substring(with: line))
                // Runs that look the same are one piece: the line's first
                // character carries the rule under the header as well.
                var pieces: [(looks: String, words: String, link: URL?)] = []
                text.enumerateAttributes(in: line) { attributes, run, _ in
                    let traits = (attributes[.font] as? NSFont)?.fontDescriptor.symbolicTraits ?? []
                    let link = attributes[.link] as? URL
                    var looks: [String] = []
                    if traits.contains(.bold) { looks.append("bold") }
                    if traits.contains(.italic) { looks.append("italic") }
                    if attributes[.strikethroughStyle] != nil { looks.append("strike") }
                    if attributes[.tbInlineBackground] != nil { looks.append("code") }
                    if link != nil { looks.append("link") }
                    let joined = looks.joined(separator: "+")
                    if let last = pieces.last, last.looks == joined, last.link == link {
                        pieces[pieces.count - 1].words += string.substring(with: run)
                    } else {
                        pieces.append((joined, string.substring(with: run), link))
                    }
                }
                styled += pieces.filter { !$0.looks.isEmpty }.map { "\($0.looks): \($0.words)" + ($0.link.map { " <\($0.absoluteString)>" } ?? "") }
                at = NSMaxRange(line) + 1
            }
            found.append(DraftInfo(label: cell.draft.label, lines: lines, styled: styled, copy: cell.wasJustCopied() ? "Copied" : "Copy"))
        }
        return found.isEmpty ? nil : found
    }

    /// The closed ```bash blocks a reply holds and the icons each gets, as
    /// TextKit draws them (ShellBlockIcons.shown): "[put, copy] npm test",
    /// or "[copy] npm test" where the Terminal wouldn't be the thread's own.
    /// The GPUI app's dump says the same.
    @MainActor
    static func shellBlocks(_ row: TranscriptRow, threadId: String) -> [String]? {
        guard case .assistant(let item) = row.content, ShellBlock.mayHold(item.text) else { return nil }
        var commands: [String] = []
        var open: (mark: Character, length: Int, quotes: Int, language: String, code: [String])?
        for line in item.text.components(separatedBy: "\n") {
            var rest = Substring(line).drop { $0 == " " || $0 == "\t" }
            var quotes = 0
            while rest.first == ">" {
                quotes += 1
                rest = rest.dropFirst().drop { $0 == " " || $0 == "\t" }
            }
            if var block = open {
                let closing = rest.trimmingCharacters(in: .whitespaces)
                if quotes == block.quotes, closing.count >= block.length, closing.allSatisfy({ $0 == block.mark }) {
                    if let shell = ShellBlock.from(language: block.language, code: block.code.joined(separator: "\n")) { commands.append(shell.command) }
                    open = nil
                } else {
                    block.code.append(block.quotes > 0 ? String(rest) : line)
                    open = block
                }
            } else if let mark = rest.first, mark == "`" || mark == "~" {
                let length = rest.prefix { $0 == mark }.count
                if length >= 3 { open = (mark, length, quotes, rest.dropFirst(length).trimmingCharacters(in: .whitespaces), []) }
            }
        }
        guard !commands.isEmpty else { return nil }
        let icons = ShellBlockIcons.shown(in: threadId).contains(.put) ? "[put, copy]" : "[copy]"
        return commands.map { "\(icons) \($0)" }
    }

    private func pad(_ text: String, _ width: Int) -> String {
        text.count >= width ? text : text + String(repeating: " ", count: width - text.count)
    }
}
#endif
