// Settings › Pools (SettingsView.swift's PoolSettings): the runner's pools,
// the one being edited (its name, icon, logins in order and folders), kept
// as a draft and saved whole (`pools.save`) whenever the runner would take
// it. What it would refuse is said under the form instead of sent.
use super::{plain_note, row_with, section, section_with_body, SettingsWindow};
use crate::{call, icons, style};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, Sizable as _, StyledExt as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::json;
use threadbear_core::logins::{provider_title, twin_label, Account, Pool};
use threadbear_core::pools;

/// The icons a pool can take (`Pools.iconChoices`), those this app has a
/// drawing of.
const ICON_CHOICES: [&str; 28] = [
    "sparkle", "chevron.left.forwardslash.chevron.right", "briefcase", "house", "hammer", "wrench.and.screwdriver", "paintbrush", "film", "book", "graduationcap", "leaf", "flame", "bolt", "moon", "sun.max", "star", "heart", "flag", "bookmark", "tag", "globe", "building.2", "cart", "gamecontroller", "music.note", "camera", "airplane", "cup.and.saucer",
];

/// What's shown, which can run ahead of the runner's list while a new pool
/// has no logins yet.
#[derive(Default)]
pub(crate) struct PoolsDraft {
    list: Vec<Pool>,
    /// The runner's list this was last reset to, to follow its changes.
    base: Vec<Pool>,
    selected: Option<String>,
    /// The name box, made once there's a window to make it in.
    name: Option<Entity<InputState>>,
    /// What went wrong with the last save, until the next one.
    error: Option<String>,
    saving: bool,
}

impl SettingsWindow {
    pub(super) fn render_pools(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(app) = self.app.upgrade() else { return vec![plain_note("Not connected to the runner.", cx)] };
        let (runner_pools, accounts, connected) = {
            let t = app.read(cx);
            (t.logins.pools.clone(), t.logins.accounts.clone(), matches!(t.status, threadbear_client::runner::Status::Connected { .. }))
        };
        if runner_pools.is_empty() {
            return vec![plain_note(if connected { "This runner is too old for pools. It gets them when it next updates." } else { "Not connected to the runner." }, cx)];
        }
        // The runner's list wins, unless Mike is part way through a change
        // it wouldn't take yet.
        if self.pools.base != runner_pools && pools::problem(&self.pools.list, &accounts).is_none() || self.pools.list.is_empty() {
            self.pools.list = runner_pools.clone();
            self.pools.base = runner_pools;
            if !self.pools.selected.as_ref().is_some_and(|id| self.pools.list.iter().any(|p| &p.id == id)) {
                self.pools.selected = self.pools.list.first().map(|p| p.id.clone());
            }
            self.fill_name(window, cx);
        }
        let name = self.name_box(window, cx);
        let current = self.current_pool().cloned();
        let mut sections = Vec::new();

        // The list, with + and −.
        let mut rows: Vec<AnyElement> = self.pools.list.iter().map(|pool| self.pool_row(pool, current.as_ref().is_some_and(|c| c.id == pool.id), &accounts, cx)).collect();
        let can_delete = self.pools.list.len() > 1 && current.is_some();
        let delete_tip = if self.pools.list.len() < 2 { "There has to be at least one pool".to_owned() } else { format!("Delete {}", current.as_ref().map_or("this pool", |p| p.title.as_str())) };
        let add = self.icon_button("settings-pool-add".into(), "plus", "Add a pool", cx, |this, window, cx| this.add_pool(window, cx));
        let delete = if can_delete {
            self.icon_button_owned("settings-pool-delete".into(), "minus", delete_tip, cx, |this, window, cx| this.confirm_delete_pool(window, cx))
        } else {
            crate::highlights::tipped(div().id("settings-pool-delete").flex_none().size(px(22.)).flex().items_center().justify_center().opacity(0.4), delete_tip).child(icons::symbol("minus").with_size(px(13.))).into_any_element()
        };
        rows.push(row_with(h_flex().gap(px(2.)).child(add).child(delete).into_any_element(), div().into_any_element(), cx));
        sections.push(section(
            "settings-pools",
            "Pools",
            "New threads start on their folder's pool, or on the one you picked last. Each turn runs on a login with room, and the thread moves on when that login runs out.",
            "set up pools for you, such as one for work and one for everything else, or to add another login.",
            rows,
            cx,
        ));

        if let Some(pool) = current {
            // Its name, icon and logins.
            let mut rows = vec![row_with(div().child("Name").into_any_element(), div().w(px(220.)).child(Input::new(&name).small()).into_any_element(), cx)];
            rows.push(row_with(div().child("Icon").into_any_element(), self.icon_picker(&pool, &accounts, cx), cx));
            for (account, included) in pools::login_list(&pool, &accounts) {
                rows.push(self.login_row(&pool, account, included, &accounts, cx));
            }
            sections.push(section_with_body(
                "settings-pool-logins",
                &pool.title,
                "Tick the logins this pool can use. A mixed pool tries providers in this order; within one provider, the login with the most room goes first.",
                "change its logins or their order, or add a login that isn't listed.",
                group(rows, cx),
                cx,
            ));
            // Its folders.
            let secondary = style::secondary_text(cx);
            let mut rows: Vec<AnyElement> = Vec::new();
            if pool.folders.is_empty() {
                rows.push(row_with(div().text_color(secondary).child("None yet").into_any_element(), div().into_any_element(), cx));
            }
            for folder in &pool.folders {
                let (id, folder_owned) = (pool.id.clone(), folder.clone());
                let remove = self.icon_button_owned(format!("settings-pool-folder-remove-{folder}"), "minus.circle", format!("Stop starting threads in this folder on {}", pool.title), cx, move |this, _, cx| {
                    let folder = folder_owned.clone();
                    this.change_pool(&id, cx, move |p| p.folders.retain(|f| f != &folder));
                });
                let label = h_flex().gap(px(8.)).items_center().min_w_0().child(icons::symbol("folder").with_size(px(13.)).text_color(secondary)).child(div().min_w_0().text_ellipsis_middle().whitespace_nowrap().overflow_hidden().child(folder.clone()));
                rows.push(row_with(label.into_any_element(), remove, cx));
            }
            let id = pool.id.clone();
            let add = self.button_owned("settings-pool-folder-add", "Add Folder…", format!("Pick folders whose new threads start on {}", pool.title), cx, move |this, window, cx| this.add_pool_folders(id.clone(), window, cx));
            rows.push(row_with(h_flex().child(add).into_any_element(), div().into_any_element(), cx));
            sections.push(section("settings-pool-folders", "Folders", &format!("New threads in these folders, or a folder inside one, start on {}.", pool.title), "add folders for you, or move a project's folders to another pool.", rows, cx));
        }

        // What the runner would refuse, else what it did.
        let note = pools::problem(&self.pools.list, &accounts).map(|p| (p, false)).or_else(|| self.pools.error.clone().map(|e| (e, true)));
        if let Some((text, failed)) = note {
            let color = if failed { style::failed(cx) } else { style::secondary_text(cx) };
            sections.push(h_flex().px(px(4.)).gap(px(6.)).items_center().text_size(px(13.)).text_color(color).child(icons::symbol(if failed { "exclamationmark.triangle" } else { "info.circle" }).with_size(px(13.))).child(div().flex_1().child(text)).into_any_element());
        }
        sections
    }

    fn current_pool(&self) -> Option<&Pool> {
        self.pools.selected.as_ref().and_then(|id| self.pools.list.iter().find(|p| &p.id == id)).or_else(|| self.pools.list.first())
    }

    /// The name box, made the first time it's drawn; Return or leaving it
    /// saves what's typed (`commitTitle`).
    fn name_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(name) = &self.pools.name {
            return name.clone();
        }
        let title = self.current_pool().map(|p| p.title.clone()).unwrap_or_default();
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Pool name").default_value(title));
        // A new name typed and not yet taken: an update's restart waits for
        // it. What it's filled with from the pool is saved (`fill_name`).
        crate::unsent::watch(&name, cx);
        let sub = cx.subscribe_in(&name, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.commit_pool_name(window, cx);
            }
        });
        self._subs.push(sub);
        self.pools.name = Some(name.clone());
        name
    }

    /// Puts the selected pool's name in the box.
    fn fill_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.current_pool().map(|p| p.title.clone()).unwrap_or_default();
        if let Some(name) = self.pools.name.clone() {
            name.update(cx, |s, cx| s.set_value(title, window, cx));
            crate::unsent::saved(&name, cx);
        }
    }

    fn commit_pool_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(pool), Some(name)) = (self.current_pool().cloned(), self.pools.name.clone()) else { return };
        let typed = name.read(cx).value().to_string();
        match pools::renaming(&pool, &typed) {
            Some(renamed) => {
                let title = renamed.title.clone();
                self.change_pool(&pool.id, cx, move |p| p.title = title.clone());
            }
            // Blank or unchanged: back to the name it has.
            None => name.update(cx, |s, cx| s.set_value(pool.title.clone(), window, cx)),
        }
        crate::unsent::saved(&name, cx);
    }

    fn choose_pool(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_pool_name(window, cx);
        self.pools.selected = Some(id);
        self.fill_name(window, cx);
        cx.notify();
    }

    fn add_pool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_pool_name(window, cx);
        let taken: Vec<String> = self.pools.list.iter().map(|p| p.id.clone()).collect();
        let pool = Pool { id: pools::new_id("New pool", &taken), title: "New pool".into(), ..Default::default() };
        self.pools.selected = Some(pool.id.clone());
        self.pools.list.push(pool);
        self.fill_name(window, cx);
        cx.notify();
    }

    fn confirm_delete_pool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pool) = self.current_pool().cloned() else { return };
        let me = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (me, id) = (me.clone(), pool.id.clone());
            alert
                .title(format!("Delete {}?", pool.title))
                .description("Threads on it carry on with the login they're on, pinned to it.")
                .button_props(DialogButtonProps::default().ok_text("Delete").show_cancel(true))
                .on_ok(move |_, window, cx| {
                    let id = id.clone();
                    me.update(cx, |this, cx| {
                        this.pools.list.retain(|p| p.id != id);
                        this.pools.selected = this.pools.list.first().map(|p| p.id.clone());
                        this.fill_name(window, cx);
                        this.save_pools(cx);
                    })
                    .ok();
                    true
                })
        });
    }

    fn add_pool_folders(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Add".into()) });
        let home = std::env::var("HOME").unwrap_or_default();
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let folders: Vec<String> = paths.iter().map(|p| threadbear_core::new_thread::abbreviate_home(&p.to_string_lossy(), &home)).collect();
                this.update(cx, |this, cx| {
                    this.change_pool(&id, cx, move |p| {
                        for folder in &folders {
                            if !p.folders.contains(folder) {
                                p.folders.push(folder.clone());
                            }
                        }
                    })
                })
                .ok();
            }
        })
        .detach();
    }

    /// Changes one pool in the draft, and saves the draft if the runner
    /// would take it.
    fn change_pool(&mut self, id: &str, cx: &mut Context<Self>, change: impl Fn(&mut Pool)) {
        if let Some(pool) = self.pools.list.iter_mut().find(|p| p.id == id) {
            change(pool);
        }
        self.save_pools(cx);
        cx.notify();
    }

    fn save_pools(&mut self, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else { return };
        let accounts = app.read(cx).logins.accounts.clone();
        if pools::problem(&self.pools.list, &accounts).is_some() {
            return;
        }
        let list: Vec<serde_json::Value> = self.pools.list.iter().map(pools::to_json).collect();
        let client = self.client.clone();
        self.pools.saving = true;
        cx.spawn(async move |this, cx| {
            let res = call(client, "pools.save", json!({ "pools": list })).await;
            this.update(cx, |this, cx| {
                this.pools.saving = false;
                match res {
                    Ok(v) => {
                        this.pools.error = None;
                        if let Some(app) = this.app.upgrade() {
                            app.update(cx, |app, cx| {
                                app.logins.set_pools(&v["pools"]);
                                cx.notify();
                            });
                        }
                    }
                    Err(e) => this.pools.error = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `PoolListRow`: its icon, its name (semibold while it's the one being
    /// edited, the row tinted), and its logins and folders in grey.
    fn pool_row(&self, pool: &Pool, selected: bool, accounts: &[Account], cx: &mut Context<Self>) -> AnyElement {
        let (accent, secondary) = (style::control_accent(cx), style::secondary_text(cx));
        let id = pool.id.clone();
        let tip = if selected { format!("Editing {} below", pool.title) } else { format!("Edit {}", pool.title) };
        let symbol = pools::symbol(pool, accounts);
        let mark = icons::try_symbol(&symbol).unwrap_or_else(|| icons::symbol("square.stack"));
        crate::highlights::tipped(
            h_flex()
                .id(SharedString::from(format!("settings-pool-{}", pool.id)))
                .min_h(px(36.))
                .px(px(super::ROW_INSET))
                .py(px(6.))
                .gap(px(8.))
                .items_center()
                .text_size(px(13.))
                .line_height(px(17.))
                .cursor_pointer()
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(8.))
                        .items_center()
                        .px(px(8.))
                        .py(px(5.))
                        .mx(px(-8.))
                        .my(px(-5.))
                        .rounded(px(6.))
                        .when(selected, |s| s.bg(accent.opacity(0.2)))
                        .child(div().w(px(20.)).flex().justify_center().child(mark.with_size(px(14.)).text_color(if selected { accent } else { secondary })))
                        .child(div().flex_none().when(selected, |s| s.font_semibold()).child(pool.title.clone()))
                        .child(div().flex_1().min_w(px(12.)))
                        .child(div().min_w_0().truncate().text_color(secondary).child(pool_detail(pool, accounts))),
                ),
            tip,
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.choose_pool(id.clone(), window, cx)))
        .into_any_element()
    }

    /// The Icon row's button, which opens a grid of the choices.
    fn icon_picker(&self, pool: &Pool, accounts: &[Account], cx: &mut Context<Self>) -> AnyElement {
        let symbol = pools::symbol(pool, accounts);
        let secondary = style::secondary_text(cx);
        let me = cx.entity().downgrade();
        let id = pool.id.clone();
        let face = h_flex()
            .id("settings-pool-icon")
            .gap(px(4.))
            .items_center()
            .px(px(6.))
            .py(px(3.))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(style::primary_opacity(cx, 0.08)))
            .child(icons::try_symbol(&symbol).unwrap_or_else(|| icons::symbol("square.stack")).with_size(px(14.)))
            .child(icons::symbol("chevron.down").with_size(px(8.)).text_color(secondary));
        let _ = face;
        let shown = symbol.clone();
        Popover::new("settings-pool-icon-popover")
            .anchor(Anchor::TopRight)
            .trigger(IconFace { symbol: symbol.clone(), secondary, hover: style::primary_opacity(cx, 0.08) })
            .content(move |_, window, cx| {
                let popover = cx.entity();
                let accent = style::control_accent(cx);
                let _ = window;
                let mut grid = div().flex().flex_wrap().w(px(7. * 34.)).gap(px(4.)).p(px(6.));
                for choice in ICON_CHOICES.iter().filter(|c| icons::try_symbol(c).is_some()) {
                    let (me, id, choice) = (me.clone(), id.clone(), *choice);
                    grid = grid.child(
                        crate::highlights::tipped(
                            div().id(SharedString::from(format!("pool-icon-{choice}"))).size(px(30.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer().when(choice == shown, |s| s.bg(accent.opacity(0.25))).hover(|s| s.bg(style::primary_opacity(cx, 0.08))),
                            choice,
                        )
                        .on_click({
                            let popover = popover.clone();
                            move |_, window, cx| {
                                let id = id.clone();
                                me.update(cx, |this, cx| this.change_pool(&id, cx, move |p| p.icon = Some(choice.to_owned()))).ok();
                                popover.update(cx, |s, cx| s.dismiss(window, cx));
                            }
                        })
                        .child(icons::symbol(choice).with_size(px(14.))),
                    );
                }
                grid
            })
            .into_any_element()
    }

    /// `LoginToggleRow`: ticked if it's in the pool, with arrows to put the
    /// ticked ones in order.
    fn login_row(&self, pool: &Pool, account: &Account, included: bool, accounts: &[Account], cx: &mut Context<Self>) -> AnyElement {
        let secondary = style::secondary_text(cx);
        // Two logins with one email (issue 18) read the same, so the title
        // adds the note, or the id with none, and the note isn't said twice.
        let twin = twin_label(account, accounts);
        let note = account.note.clone().filter(|_| twin.is_none());
        let detail: Vec<String> = [Some(provider_title(account.provider_id())), note].into_iter().flatten().filter(|s| !s.is_empty()).collect();
        let (pool_id, login) = (pool.id.clone(), account.id.clone());
        let me = cx.entity().downgrade();
        let title = match twin {
            Some(label) => format!("{} ({label})", account.name()),
            None => account.name(),
        };
        let label = h_flex()
            .gap(px(6.))
            .min_w_0()
            .child(div().flex_none().child(title.clone()))
            .child(div().min_w_0().truncate().text_color(secondary).child(detail.join(" · ")));
        let toggle = {
            let (pool_id, login) = (pool_id.clone(), login.clone());
            Checkbox::new(SharedString::from(format!("settings-pool-login-{}", account.id))).checked(included).small().on_click(move |on: &bool, _, cx| {
                let (pool_id, login, on) = (pool_id.clone(), login.clone(), *on);
                me.update(cx, |this, cx| this.change_pool(&pool_id, cx, move |p| *p = pools::setting(p, &login, on))).ok();
            })
        };
        let mut controls = h_flex().gap(px(2.));
        if included {
            let first = pool.account_ids.first() == Some(&account.id);
            let last = pool.account_ids.last() == Some(&account.id);
            for (step, symbol, tip, disabled) in [(-1, "chevron.up", "Earlier in the order", first), (1, "chevron.down", "Later in the order", last)] {
                let (pool_id, login) = (pool_id.clone(), login.clone());
                controls = controls.child(if disabled {
                    crate::highlights::tipped(div().id(SharedString::from(format!("settings-pool-move-{step}-{}", account.id))).size(px(22.)).flex().items_center().justify_center().opacity(0.35), tip).child(icons::symbol(symbol).with_size(px(11.))).into_any_element()
                } else {
                    self.icon_button(format!("settings-pool-move-{step}-{}", account.id), symbol, tip, cx, move |this, _, cx| this.change_pool(&pool_id, cx, |p| *p = pools::moving_by(p, &login, step)))
                });
            }
        }
        row_with(
            crate::highlights::tipped(h_flex().id(SharedString::from(format!("settings-pool-login-row-{}", account.id))).gap(px(8.)).min_w_0().child(toggle).child(label), if included { format!("{title} is in this pool") } else { format!("Add {title} to this pool") }).into_any_element(),
            controls.into_any_element(),
            cx,
        )
    }
}

/// "acme, gmail · ~/dev/convex": a pool's logins, then its folders.
fn pool_detail(pool: &Pool, accounts: &[Account]) -> String {
    let logins: Vec<String> = pool.account_ids.iter().map(|id| threadbear_core::logins::short_account_name(id, accounts)).collect();
    let mut text = if logins.is_empty() { "No logins yet".to_owned() } else { logins.join(", ") };
    if !pool.folders.is_empty() {
        text.push_str(" · ");
        text.push_str(&pool.folders.join(", "));
    }
    text
}

/// Rows in a group, with a line between them, as `section` lays them.
fn group(rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    super::rows_with_dividers(rows, cx)
}

/// The Icon row's face: the pool's icon and a small chevron, opening the grid.
#[derive(IntoElement)]
struct IconFace {
    symbol: String,
    secondary: Hsla,
    hover: Hsla,
}

impl gpui_kit::component::Selectable for IconFace {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for IconFace {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let hover = self.hover;
        crate::highlights::tipped(
            h_flex().id("settings-pool-icon").gap(px(4.)).items_center().px(px(6.)).py(px(3.)).rounded(px(6.)).cursor_pointer().hover(move |s| s.bg(hover)),
            "Pick this pool's icon. It shows in front of the model chip under the composer.",
        )
        .child(icons::try_symbol(&self.symbol).unwrap_or_else(|| icons::symbol("square.stack")).with_size(px(14.)))
        .child(icons::symbol("chevron.down").with_size(px(8.)).text_color(self.secondary))
    }
}
