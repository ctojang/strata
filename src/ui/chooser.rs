// SPDX-License-Identifier: MIT

mod placement;
mod request;

#[cfg(test)]
mod tests;

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::{gdk::ModifierType, glib, prelude::*};

use crate::{
    adapters::{LocalFileSource, LocalOperationProvider, LocalPreviewProvider},
    model::{FileEntry, Location},
    services::{
        DirectoryChange, DirectoryEvent, DirectoryRequest, FileSource, LoadHandle,
        LocationIdentity, LocationValidationError, MetadataRequest,
    },
};

use super::{
    blur::BlurBin,
    browser::BrowserView,
    browser_modes::{BrowserMode, ThumbnailResize},
    marquee::OriginClick,
    preferences::PreferenceManager,
    preview::{PreviewDrawer, preview_target},
    shortcut_footer::ShortcutFooter,
    top_bar_navigation::TopBarNavigation,
    window::{
        ChooserKeys, ChooserPolicy, MIN_SIDEBAR_WIDTH, SIDEBAR_WIDTH, SidebarView,
        build_appearance_menu, build_sidebar, edit_focused_text, home_directory,
        install_modal_focus_trap, visible_modal_layer,
    },
};
use super::{
    focus_navigation::{
        FOCUS_MODIFIERS, arrow_direction, plain_tab_direction, vim_focus_direction,
    },
    keys::{self, KeyPress, SHORTCUT_MODIFIERS, bindings::*},
};
use request::{CHOOSER_APPLICATION_ID, open_selection};

type Completion = Box<dyn FnOnce(Option<PathBuf>)>;
type DestinationValidator = Rc<dyn Fn(&Path) -> Result<PathBuf, String>>;

thread_local! {
    static CHOOSERS: RefCell<Vec<glib::WeakRef<gtk::Window>>> = const { RefCell::new(Vec::new()) };
    static DESTINATION_PARENTS: RefCell<Vec<glib::WeakRef<gtk::Window>>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
thread_local! {
    static DESTINATION_STATES: RefCell<Vec<std::rc::Weak<ChooserState>>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn destination_chooser_for(parent: &gtk::Window) -> Option<(gtk::Window, BrowserView)> {
    DESTINATION_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .filter_map(std::rc::Weak::upgrade)
            .find(|state| {
                state.window.is_visible() && state.window.transient_for().as_ref() == Some(parent)
            })
            .map(|state| (state.window.clone(), state.view.clone()))
    })
}

struct ChooserFileSource {
    source: Rc<dyn FileSource>,
    root_limit: Option<PathBuf>,
}

impl FileSource for ChooserFileSource {
    fn allows_navigation(&self, location: &Location) -> bool {
        chooser_location_allowed(self.root_limit.as_deref(), location)
    }

    fn allows_entry(&self, entry: &FileEntry) -> bool {
        chooser_entry_allowed(self.root_limit.as_deref(), entry)
    }

    fn validate_location(&self, location: &Location) -> Result<(), LocationValidationError> {
        if !self.allows_navigation(location) {
            return Err(LocationValidationError::Refused(
                "Choose an existing folder inside this removable device.",
            ));
        }
        if location.native_path().is_none() && !location.is_recent_root() {
            return Err(LocationValidationError::Refused(
                "The system file chooser supports local files and folders only.",
            ));
        }
        self.source.validate_location(location)
    }

    fn validate_location_async(
        &self,
        location: Location,
        emit: Rc<dyn Fn(Result<(), LocationValidationError>)>,
    ) -> LoadHandle {
        if self.root_limit.is_some()
            || (location.native_path().is_none() && !location.is_recent_root())
        {
            emit(self.validate_location(&location));
            return LoadHandle::new(|| {});
        }
        self.source.validate_location_async(location, emit)
    }

    fn supports_metadata_fill(&self, location: &Location) -> bool {
        (location.native_path().is_some() || location.is_recent_root())
            && self.source.supports_metadata_fill(location)
    }

    fn fill_metadata(
        &self,
        request: MetadataRequest,
        emit: Rc<dyn Fn(DirectoryEvent)>,
    ) -> LoadHandle {
        self.source.fill_metadata(request, emit)
    }

    fn enumerate(&self, request: DirectoryRequest, emit: Rc<dyn Fn(DirectoryEvent)>) -> LoadHandle {
        let root_limit = self.root_limit.clone();
        self.source.enumerate(
            request,
            Rc::new(move |event| {
                let event = match event {
                    DirectoryEvent::Batch {
                        request_id,
                        mut entries,
                    } => {
                        entries.retain(|entry| chooser_entry_allowed(root_limit.as_deref(), entry));
                        DirectoryEvent::Batch {
                            request_id,
                            entries,
                        }
                    }
                    event => event,
                };
                emit(event);
            }),
        )
    }

    fn watch(
        &self,
        location: Location,
        include_hidden: bool,
        notify: Rc<dyn Fn(DirectoryChange)>,
    ) -> Option<LoadHandle> {
        let root_limit = self.root_limit.clone();
        self.source.watch(
            location,
            include_hidden,
            Rc::new(move |change| {
                notify(filter_directory_change(root_limit.as_deref(), change));
            }),
        )
    }

    fn query_location_identity(
        &self,
        location: Location,
        emit: Rc<dyn Fn(Option<LocationIdentity>)>,
    ) -> LoadHandle {
        self.source.query_location_identity(location, emit)
    }

    fn find_by_identity(
        &self,
        parent: Location,
        identity: LocationIdentity,
        emit: Rc<dyn Fn(Option<Location>)>,
    ) -> LoadHandle {
        self.source.find_by_identity(parent, identity, emit)
    }
}

fn chooser_location_allowed(root: Option<&Path>, location: &Location) -> bool {
    root.is_none_or(|root| {
        location.native_path().is_some_and(|path| {
            path.canonicalize()
                .is_ok_and(|path| path.is_dir() && path.starts_with(root))
        })
    })
}

fn chooser_entry_allowed(root_limit: Option<&Path>, entry: &FileEntry) -> bool {
    chooser_location_allowed(root_limit, &entry.location)
        && entry.location.native_path().is_some()
        && entry.is_directory()
}

fn filter_directory_change(root_limit: Option<&Path>, change: DirectoryChange) -> DirectoryChange {
    match change {
        DirectoryChange::Upsert(entry) if !chooser_entry_allowed(root_limit, &entry) => {
            DirectoryChange::Remove(entry.location)
        }
        DirectoryChange::Move { from, entry } if !chooser_entry_allowed(root_limit, &entry) => {
            DirectoryChange::Remove(from)
        }
        change => change,
    }
}

struct ChooserState {
    destination: DestinationRequest,
    window: gtk::Window,
    view: BrowserView,
    error: gtk::Label,
    action_status: gtk::Stack,
    completion: RefCell<Option<Completion>>,
}

impl ChooserState {
    fn cancel(&self) {
        self.finish(None);
    }

    fn finish(&self, result: Option<PathBuf>) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        CHOOSERS.with(|choosers| {
            choosers
                .borrow_mut()
                .retain(|window| window.upgrade().is_some_and(|window| window != self.window));
        });
        self.window.close();
        completion(result);
    }

    fn show_error(&self, message: &str) {
        self.error.set_label(message);
        self.error.set_visible(true);
        self.action_status.set_visible_child_name("error");
    }

    fn clear_error(&self) {
        self.error.set_visible(false);
        self.action_status.set_visible_child_name("normal");
    }

    fn chosen_entries(&self) -> Vec<FileEntry> {
        let browser = self.view.browser();
        let entries = match self.view.selected_search_results() {
            Some(results) => results,
            None if browser.selection_is_load_cursor() => return Vec::new(),
            None => browser.selected_entries(),
        };
        if entries.is_empty() && PreferenceManager::shared().tenxer_mode() {
            return self.view.focused_target().into_iter().collect();
        }
        entries
    }

    fn accept(&self) {
        if !self.destination.parent.is_visible() {
            self.cancel();
            return;
        }
        if self.completion.borrow().is_none() || visible_modal_layer(&self.window).is_some() {
            return;
        }
        self.clear_error();
        let browser = self.view.browser();
        let Some(current) = browser.active_location() else {
            self.show_error(&crate::i18n::tr("Choose an accessible local folder"));
            return;
        };
        let entries = self
            .chosen_entries()
            .into_iter()
            .filter(|entry| entry.is_directory() && browser.allows_entry(entry))
            .collect::<Vec<_>>();
        match open_selection(&entries, &current)
            .map_err(crate::i18n::tr)
            .and_then(|path| (self.destination.validate)(&path))
        {
            Ok(path) => self.finish(Some(path)),
            Err(message) => self.show_error(&message),
        }
    }

    fn confirm_file(&self, entry: &FileEntry) {
        if self.completion.borrow().is_some() && self.view.browser().allows_entry(entry) {
            self.show_error(&crate::i18n::tr("Choose folders only"));
        }
    }
}

const MIN_CHOOSER_WIDTH: i32 = 640;
const MIN_CHOOSER_HEIGHT: i32 = 460;
const MAX_CHOOSER_WIDTH: i32 = 1000;
const MAX_CHOOSER_HEIGHT: i32 = 680;
const FALLBACK_CHOOSER_WIDTH: i32 = 920;
const FALLBACK_CHOOSER_HEIGHT: i32 = 580;

fn chooser_default_dimensions_for_monitor(monitor_width: i32, monitor_height: i32) -> (i32, i32) {
    if monitor_width <= 0 || monitor_height <= 0 {
        return (FALLBACK_CHOOSER_WIDTH, FALLBACK_CHOOSER_HEIGHT);
    }
    let target_width = (monitor_width.saturating_mul(80) / 100)
        .min(monitor_width.saturating_sub(120))
        .clamp(MIN_CHOOSER_WIDTH.min(monitor_width), MAX_CHOOSER_WIDTH);
    let target_height = (monitor_height.saturating_mul(78) / 100)
        .min(monitor_height.saturating_sub(100))
        .clamp(MIN_CHOOSER_HEIGHT.min(monitor_height), MAX_CHOOSER_HEIGHT);

    (target_width, target_height)
}

fn chooser_initial_dimensions(
    monitor: Option<(i32, i32)>,
    parent_size_hint: Option<(i32, i32)>,
) -> (i32, i32) {
    let monitor = monitor.filter(|(width, height)| *width > 0 && *height > 0);
    let parent = parent_size_hint.filter(|(width, height)| *width > 0 && *height > 0);
    let bounds = match (monitor, parent) {
        (Some((mw, mh)), Some((pw, ph))) => Some((mw.min(pw), mh.min(ph))),
        (monitor, parent) => parent.or(monitor),
    };
    bounds.map_or(
        (FALLBACK_CHOOSER_WIDTH, FALLBACK_CHOOSER_HEIGHT),
        |(width, height)| chooser_default_dimensions_for_monitor(width, height),
    )
}

fn detect_monitor_geometry(
    display: Option<&gtk::gdk::Display>,
    window: Option<&gtk::Window>,
) -> Option<(i32, i32)> {
    let window_display = window.map(gtk::prelude::WidgetExt::display);
    let default_display = gtk::gdk::Display::default();
    let display = display
        .or(window_display.as_ref())
        .or(default_display.as_ref())?;

    if let Some(geom) = window
        .and_then(|w| w.surface())
        .and_then(|surface| display.monitor_at_surface(&surface))
        .map(|monitor| monitor.geometry())
        .filter(|geom| geom.width() > 0 && geom.height() > 0)
    {
        return Some((geom.width(), geom.height()));
    }

    let monitors = display.monitors();
    for index in 0..monitors.n_items() {
        if let Some(monitor) = monitors
            .item(index)
            .and_then(|item| item.downcast::<gtk::gdk::Monitor>().ok())
        {
            let geom = monitor.geometry();
            if geom.width() > 0 && geom.height() > 0 {
                return Some((geom.width(), geom.height()));
            }
        }
    }

    None
}

struct DestinationLease(glib::WeakRef<gtk::Window>);

impl DestinationLease {
    fn acquire(parent: &gtk::Window) -> Option<Self> {
        let acquired = DESTINATION_PARENTS.with(|parents| {
            let mut parents = parents.borrow_mut();
            parents.retain(|parent| parent.upgrade().is_some());
            if parents
                .iter()
                .any(|candidate| candidate.upgrade().as_ref() == Some(parent))
            {
                return false;
            }
            parents.push(parent.downgrade());
            true
        });
        if acquired {
            Some(Self(parent.downgrade()))
        } else {
            let active = CHOOSERS.with(|choosers| {
                choosers
                    .borrow()
                    .iter()
                    .filter_map(glib::WeakRef::upgrade)
                    .find(|window| window.transient_for().as_ref() == Some(parent))
            });
            if let Some(window) = active {
                window.present();
            }
            None
        }
    }
}

impl Drop for DestinationLease {
    fn drop(&mut self) {
        DESTINATION_PARENTS.with(|parents| {
            parents.borrow_mut().retain(|parent| {
                parent.upgrade() != self.0.upgrade() && parent.upgrade().is_some()
            });
        });
    }
}

pub(crate) struct DestinationRequest {
    pub parent: gtk::Window,
    pub title: String,
    pub accept_label: String,
    pub initial_directory: PathBuf,
    pub root_limit: Option<PathBuf>,
    pub allow_create: bool,
    pub validate: DestinationValidator,
}

pub(crate) fn present_destination_chooser(
    destination: DestinationRequest,
    completion: impl FnOnce(PathBuf) + 'static,
) {
    let Some(lease) = DestinationLease::acquire(&destination.parent) else {
        return;
    };
    glib::MainContext::default().spawn_local(async move {
        placement::prepare_chooser_placement().await;
        if !destination.parent.is_visible() {
            return;
        }
        build_chooser(destination, move |path| {
            drop(lease);
            if let Some(path) = path {
                completion(path);
            }
        });
    });
}

fn build_chooser(
    destination: DestinationRequest,
    completion: impl FnOnce(Option<PathBuf>) + 'static,
) -> Rc<ChooserState> {
    let source = Rc::new(ChooserFileSource {
        source: Rc::new(LocalFileSource),
        root_limit: destination.root_limit.clone(),
    });
    let view = BrowserView::new_chooser(source);
    let theme = PreferenceManager::shared();
    view.set_operation_provider(Rc::new(LocalOperationProvider));
    view.set_chooser_allows_create(destination.allow_create);
    let browser = view.browser();
    let preview_preferences = theme.clone();
    let preview = PreviewDrawer::new(
        Rc::new(LocalPreviewProvider::new(Rc::new(move || {
            preview_preferences.media_preview_backend()
        }))),
        false,
    );

    let parent_size = Some((destination.parent.width(), destination.parent.height()));
    let (initial_width, initial_height) =
        chooser_initial_dimensions(detect_monitor_geometry(None, None), parent_size);

    let window = gtk::Window::builder()
        .title(&destination.title)
        .default_width(initial_width)
        .default_height(initial_height)
        .modal(true)
        .build();
    crate::ui::decoration::claim_titlebar(&window);
    window.set_transient_for(Some(&destination.parent));
    window.set_destroy_with_parent(true);
    window.set_application(destination.parent.application().as_ref());
    let child = window.downgrade();
    let handler = destination.parent.connect_unrealize(move |_| {
        if let Some(window) = child.upgrade() {
            window.close();
        }
    });
    let parent = destination.parent.downgrade();
    let handler = RefCell::new(Some(handler));
    window.connect_unrealize(move |_| {
        if let Some(handler) = handler.take()
            && let Some(parent) = parent.upgrade()
        {
            parent.disconnect(handler);
        }
    });
    let header = gtk::HeaderBar::new();
    header.add_css_class("browser-header");
    header.set_show_title_buttons(false);
    let sidebar_toggle = super::sidebar_toggle::SidebarToggle::new();
    let location = view.location_widget();
    location.set_hexpand(true);
    let appearance = build_appearance_menu(&view, &browser, theme.clone(), &preview);
    let header_content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header_content.set_hexpand(true);
    header_content.set_valign(gtk::Align::Center);
    header_content.append(&sidebar_toggle);
    header_content.append(&location);
    let close = gtk::Button::builder()
        .tooltip_text(crate::ui::shortcut_reference::with_keys(
            "Cancel file selection (%{keys})",
            crate::ui::keys::Keys::One(crate::ui::keys::bindings::ESCAPE),
        ))
        .build();
    close.set_child(Some(&crate::assets::chrome_icon(crate::assets::icons::X)));
    close.add_css_class("header-action");
    let closing_window = window.downgrade();
    close.connect_clicked(move |_| {
        if let Some(window) = closing_window.upgrade() {
            window.close();
        }
    });
    let header_actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header_actions.add_css_class("header-actions");
    header_actions.append(&appearance);
    header_actions.append(&close);
    header_content.append(&header_actions);
    header.set_title_widget(Some(&header_content));

    let confined = destination.root_limit.is_some();
    let sidebar = build_sidebar(view.clone(), theme.clone(), true);
    sidebar.schedule_after_first_paint(&window);
    bind_chooser_sidebar_modes(&sidebar, &sidebar_toggle, &theme);
    let content = gtk::Paned::new(gtk::Orientation::Horizontal);
    content.set_wide_handle(false);
    content.set_position(SIDEBAR_WIDTH);
    sidebar.widget.set_size_request(MIN_SIDEBAR_WIDTH, -1);
    super::window::bind_sidebar_text_size(&content, &sidebar);
    content.set_shrink_start_child(false);
    content.set_resize_start_child(false);
    content.set_start_child(Some(&sidebar.widget));
    if confined {
        sidebar.widget.set_visible(false);
        sidebar_toggle.set_visible(false);
    }
    content.set_end_child(Some(&view.widget()));
    content.set_vexpand(true);
    let toggled_sidebar = sidebar.widget.clone();
    sidebar_toggle.connect_toggled(move |toggle| {
        toggled_sidebar.set_visible(toggle.is_active());
    });

    let preview_split = gtk::Paned::new(gtk::Orientation::Horizontal);
    preview_split.add_css_class("preview-split");
    // The column beside the divider keeps its resize edge; the preview grip moves it.
    preview_split.set_wide_handle(true);
    preview_split.set_resize_start_child(true);
    preview_split.set_resize_end_child(false);
    preview_split.set_shrink_start_child(false);
    preview_split.set_shrink_end_child(true);
    preview_split.set_start_child(Some(&content));
    preview_split.set_end_child(Some(&preview.widget()));
    preview_split.set_position(i32::MAX);
    preview_split.set_vexpand(true);
    preview.attach_split(&preview_split, &content, &view, Some(&sidebar));
    view.add_marquee_origin(
        &sidebar.widget,
        gtk::PackType::Start,
        OriginClick::ClearsSelection,
    );
    view.add_marquee_origin(
        &preview.widget(),
        gtk::PackType::End,
        OriginClick::keeps_selection_within(&preview.pane_widget()),
    );
    let (footer, footer_holder) = chooser_footer(&view, &theme);
    footer.set_chooser(crate::ui::shortcut_reference::ChooserScope);

    let error = gtk::Label::builder()
        .accessible_role(gtk::AccessibleRole::Alert)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    error.add_css_class("form-field-error");
    error.set_visible(false);

    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    cancel.add_css_class("action-dialog-cancel");
    let accept = gtk::Button::with_mnemonic(&destination.accept_label);
    accept.add_css_class("action-dialog-confirm");
    crate::ui::accessibility::set_description(
        &accept,
        Some(&crate::ui::shortcut_reference::with_keys(
            "Select folder (%{keys})",
            crate::ui::keys::Keys::One(crate::ui::keys::bindings::CHOOSE_FOLDER),
        )),
    );
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("chooser-actions");
    let normal_status = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let action_status = gtk::Stack::builder()
        .hexpand(true)
        .hhomogeneous(false)
        .vhomogeneous(true)
        .build();
    action_status.add_named(&normal_status, Some("normal"));
    action_status.add_named(&error, Some("error"));
    action_status.set_visible_child_name("normal");
    actions.append(&action_status);
    actions.append(&cancel);
    actions.append(&accept);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&header);
    root.append(&preview_split);
    root.append(&footer_holder);
    root.append(&actions);
    let blurred_root = BlurBin::new(&root);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&blurred_root));
    window.set_child(Some(&overlay));
    view.install_inline_edit_dismissal(&window);
    install_modal_focus_trap(&window);
    view.set_as_modal_focus_fallback(&window);
    window.set_default_widget(Some(&accept));

    let state = Rc::new(ChooserState {
        destination,
        window: window.clone(),
        view: view.clone(),
        error,
        action_status,
        completion: RefCell::new(Some(Box::new(completion))),
    });

    let weak = Rc::downgrade(&state);
    accept.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            state.accept();
        }
    });
    let weak = Rc::downgrade(&state);
    cancel.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            state.cancel();
        }
    });
    let preview_for_browser = preview.clone();
    let state_kept_until_unrealize = state.clone();
    browser.observe(move |event| {
        preview_for_browser.handle_browser_event(&state_kept_until_unrealize.view.browser(), event);
    });

    #[cfg(test)]
    DESTINATION_STATES.with(|states| {
        let mut states = states.borrow_mut();
        states.retain(|state| state.strong_count() > 0);
        states.push(Rc::downgrade(&state));
    });
    let weak = Rc::downgrade(&state);
    window.connect_close_request(move |_| {
        if let Some(state) = weak.upgrade() {
            state.cancel();
        }
        glib::Propagation::Proceed
    });
    let tenxer = tenxer_keys(
        &state,
        &sidebar,
        sidebar_toggle.upcast_ref(),
        &header_content,
        &preview,
        &footer,
    );
    install_shortcuts(
        &window,
        &state,
        &sidebar,
        sidebar_toggle.upcast_ref(),
        header_content.upcast_ref(),
        &preview,
        tenxer,
    );
    // Destroy can be delayed by the chooser's own closures; unrealize breaks their bindings.
    let browser_for_close = browser.clone();
    let closing_state = Rc::downgrade(&state);
    window.connect_unrealize(move |window| {
        if let Some(state) = closing_state.upgrade() {
            state.cancel();
        }
        browser_for_close.clear_observer();
        sidebar.disconnect();
        PreferenceManager::shared().release_bindings_within(window);
    });

    CHOOSERS.with(|choosers| choosers.borrow_mut().push(window.downgrade()));

    gtk::prelude::WidgetExt::realize(&window);
    if let Some(surface) = window
        .surface()
        .and_downcast::<gdk4_wayland::WaylandToplevel>()
    {
        surface.set_application_id(CHOOSER_APPLICATION_ID);
    }
    let dimensions =
        chooser_initial_dimensions(detect_monitor_geometry(None, Some(&window)), parent_size);
    window.set_default_size(dimensions.0, dimensions.1);
    browser.navigate(Location::local(&state.destination.initial_directory));
    window.present();
    if PreferenceManager::shared().tenxer_mode() {
        window.set_focus_visible(true);
        return state;
    }
    sidebar_toggle.grab_focus();
    window.set_focus_visible(true);
    let initial_focus = gtk::prelude::RootExt::focus(&window).map(|widget| widget.downgrade());
    // View initialization also queues focus work; restore the chooser's target afterward.
    glib::idle_add_local_once(move || {
        if let Some(widget) = initial_focus.and_then(|widget| widget.upgrade()) {
            widget.grab_focus();
        }
    });
    state
}

fn chooser_footer(
    view: &BrowserView,
    preferences: &Rc<PreferenceManager>,
) -> (ShortcutFooter, gtk::Box) {
    let footer = ShortcutFooter::new(view.view_mode());
    footer.bind_preferences(preferences);
    footer.observe_browser(&view.browser());
    let updated = footer.clone();
    view.connect_view_mode_changed(move |mode| updated.set_mode(mode));
    let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
    holder.add_css_class("chooser-footer");
    holder.append(footer.widget());
    preferences.bind_preference(
        &holder,
        PreferenceManager::tenxer_mode,
        |holder, enabled| holder.set_visible(enabled),
    );
    (footer, holder)
}

/// The chooser keeps its sidebar modes for the dialog only.
fn bind_chooser_sidebar_modes(
    sidebar: &SidebarView,
    toggle: &super::sidebar_toggle::SidebarToggle,
    preferences: &Rc<PreferenceManager>,
) {
    use super::sidebar_toggle::SidebarMode;
    let state = Rc::downgrade(&sidebar.state);
    let stepping_toggle = toggle.clone();
    toggle.set_step(move || {
        let Some(state) = state.upgrade() else {
            return;
        };
        match SidebarMode::of(stepping_toggle.is_active(), state.rail.get()).next() {
            SidebarMode::Expanded => {
                state.set_compact(false);
                stepping_toggle.set_active(true);
            }
            SidebarMode::Compact => state.set_compact(true),
            SidebarMode::Hidden => stepping_toggle.set_active(false),
        }
    });
    let describe = {
        let toggle = toggle.clone();
        let state = Rc::downgrade(&sidebar.state);
        let preferences = preferences.clone();
        Rc::new(move || {
            if let Some(state) = state.upgrade() {
                toggle.describe(
                    SidebarMode::of(toggle.is_active(), state.rail.get()),
                    preferences.tenxer_mode(),
                );
            }
        })
    };
    let on_toggled = describe.clone();
    toggle.connect_toggled(move |_| on_toggled());
    let on_rail = describe.clone();
    sidebar.state.connect_rail_changed(move |_| on_rail());
    preferences.bind_preference(toggle, PreferenceManager::tenxer_mode, move |_, _| {
        describe();
    });
}

fn tenxer_keys(
    state: &Rc<ChooserState>,
    sidebar: &SidebarView,
    sidebar_toggle: &gtk::ToggleButton,
    header: &gtk::Box,
    preview: &PreviewDrawer,
    footer: &ShortcutFooter,
) -> ChooserKeys {
    let confirming = Rc::downgrade(state);
    let cancelling = Rc::downgrade(state);
    super::window::chooser_keys(
        &state.window,
        &state.view,
        sidebar,
        TopBarNavigation::new(header, &sidebar.widget, sidebar_toggle),
        preview,
        footer,
        ChooserPolicy {
            confirm: Rc::new(move |entry| {
                if let Some(state) = confirming.upgrade() {
                    state.confirm_file(&entry);
                }
            }),
            cancel: Rc::new(move || {
                if let Some(state) = cancelling.upgrade() {
                    state.cancel();
                }
            }),
        },
    )
}

fn install_shortcuts(
    window: &gtk::Window,
    state: &Rc<ChooserState>,
    sidebar: &SidebarView,
    sidebar_toggle: &gtk::ToggleButton,
    header: &gtk::Widget,
    preview: &PreviewDrawer,
    tenxer: ChooserKeys,
) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(state);
    let sidebar_state = sidebar.state.clone();
    let sidebar_widget = sidebar.widget.clone();
    let sidebar_toggle = sidebar_toggle.clone();
    let header = header.clone();
    let preview = preview.clone();
    let focus_before_sidebar = Rc::new(RefCell::new(None::<gtk::Widget>));
    let handle = move |state: &Rc<ChooserState>, press: &KeyPress| -> glib::Propagation {
        let preferences = PreferenceManager::shared();
        if let Some(layer) = visible_modal_layer(&state.window) {
            let focused = gtk::prelude::RootExt::focus(&state.window);
            if !focused.is_some_and(|focus| focus == layer || focus.is_ancestor(&layer)) {
                layer.grab_focus();
            }
            return glib::Propagation::Proceed;
        }
        let focused = gtk::prelude::RootExt::focus(&state.window);
        if preferences.tenxer_mode()
            && state.view.location_has_focus()
            && !press.is_any(&[
                REFERENCE.ignoring(SHORTCUT_MODIFIERS),
                ESCAPE_ANY,
                TENXER_MODE,
            ])
        {
            return glib::Propagation::Proceed;
        }
        if preview.reset_pdf_zoom(press) {
            return glib::Propagation::Stop;
        }
        if let Some(result) = tenxer.handle(press) {
            return result;
        }
        if let Some(size) = preferences.text_size().for_shortcut(press) {
            state.view.set_text_size(size);
            return glib::Propagation::Stop;
        }
        if let Some(change) = ThumbnailResize::for_shortcut(press)
            && state.view.resize_icons_thumbnails(change)
        {
            return glib::Propagation::Stop;
        }
        if tenxer.reference_is_open() {
            return glib::Propagation::Proceed;
        }
        let browser = state.view.browser();
        let held = press.modifiers();
        let control = held.contains(ModifierType::CONTROL_MASK);
        let alt = held.contains(ModifierType::ALT_MASK);
        if focused
            .as_ref()
            .and_then(|focused| focused.ancestor(gtk::Popover::static_type()))
            .is_some_and(|popover| popover.has_css_class("folder-context-popover"))
        {
            return glib::Propagation::Proceed;
        }
        if press.is_any(&[CONTEXT_MENU, CONTEXT_MENU_F10])
            && !focused.as_ref().is_some_and(|widget| {
                super::focus_navigation::editable(widget)
                    || super::focus_navigation::in_popover(widget)
            })
            && state.view.open_focused_context_menu()
        {
            return glib::Propagation::Stop;
        }
        if state.view.item_view_has_focus()
            && !state.view.new_entry_is_active()
            && !state.view.rename_is_active()
            && !focused.as_ref().is_some_and(|widget| {
                super::focus_navigation::editable(widget)
                    || super::focus_navigation::in_popover(widget)
            })
            && preview.handle_video_key(press)
        {
            return glib::Propagation::Stop;
        }
        // Filtered rows own navigation, not the hidden directory selection.
        if press.is_any(&[
            UP.ignoring(SHORTCUT_MODIFIERS),
            DOWN.ignoring(SHORTCUT_MODIFIERS),
        ]) && state.view.selected_search_results().is_some()
            && !focused.as_ref().is_some_and(|widget| {
                super::focus_navigation::editable(widget)
                    || super::focus_navigation::in_popover(widget)
            })
        {
            state.window.set_focus_visible(true);
            return glib::Propagation::Proceed;
        }
        let original = *press;
        let press = &super::focus_navigation::navigation_key(
            press,
            PreferenceManager::shared().type_to_search(),
            focused.as_ref(),
        );
        let vim_navigation = *press != original;
        if !focused
            .as_ref()
            .is_some_and(super::focus_navigation::in_popover)
            && (press.is(SIDEBAR_FOCUS)
                || (!focused
                    .as_ref()
                    .is_some_and(super::focus_navigation::editable)
                    && super::window::is_browser_navigation_key(press)))
        {
            state.view.keyboard_navigation();
        }
        if arrow_direction(press, ModifierType::empty()).is_some() {
            state.window.set_focus_visible(true);
            if focused
                .as_ref()
                .is_none_or(|widget| !widget.is_mapped() || !widget.is_sensitive())
            {
                if preferences.tenxer_mode() {
                    browser.focus_active();
                } else {
                    sidebar_toggle.grab_focus();
                }
                return glib::Propagation::Stop;
            }
        }
        let sidebar_has_focus = focused.as_ref().is_some_and(|focused| {
            focused == &sidebar_widget || focused.is_ancestor(&sidebar_widget)
        });
        if press.is(ESCAPE_ANY) {
            if let Some(popover) = focused
                .as_ref()
                .and_then(|widget| widget.ancestor(gtk::Popover::static_type()))
                .and_downcast::<gtk::Popover>()
            {
                popover.popdown();
                return glib::Propagation::Stop;
            }
            if state.view.cancel_new_entry() || state.view.cancel_rename() {
                return glib::Propagation::Stop;
            }
            if state.view.dismiss_focused_filter() {
                return glib::Propagation::Stop;
            }
            if state.view.location_has_focus() {
                state.view.cancel_location_edit();
                return glib::Propagation::Stop;
            }
            if preview.is_enabled() {
                preview.close();
                return glib::Propagation::Stop;
            }
            state.cancel();
            return glib::Propagation::Stop;
        }
        if press.is_any(&[RENAME, RENAME_R])
            && browser.selected_entries().len() == 1
            && !focused.as_ref().is_some_and(|widget| {
                super::focus_navigation::editable(widget)
                    || super::focus_navigation::in_popover(widget)
            })
            && state.view.begin_rename()
        {
            return glib::Propagation::Stop;
        }
        if press.is(PROPERTIES.ignoring(ModifierType::SUPER_MASK))
            && state.view.item_view_has_focus()
            && !state.view.new_entry_is_active()
            && !state.view.rename_is_active()
            && state.view.show_focused_properties()
        {
            return glib::Propagation::Stop;
        }
        if state.view.new_entry_is_active() || state.view.rename_is_active() {
            return glib::Propagation::Proceed;
        }
        if press.is(SPACE)
            && let Some(entry) = state.view.search_result_for_space()
        {
            if state.view.activate_directory_on_space() {
                return glib::Propagation::Stop;
            }
            preview.toggle(
                preview_target(Some(entry)),
                state.view.browser().active_depth(),
            );
            return glib::Propagation::Stop;
        }
        if press.is(FILTER) && state.view.show_filter() {
            return glib::Propagation::Stop;
        }
        if press.is(LOCATION) {
            state.view.begin_location_edit();
            return glib::Propagation::Stop;
        }
        if press.is(SIDEBAR_FOCUS) {
            if preferences.tenxer_mode() {
                if !sidebar_toggle.is_active() {
                    return glib::Propagation::Stop;
                }
                if sidebar_has_focus {
                    let restored = focus_before_sidebar
                        .borrow_mut()
                        .take()
                        .is_some_and(|widget| widget.is_mapped() && widget.grab_focus());
                    if !restored {
                        browser.focus_active();
                    }
                } else {
                    focus_before_sidebar.replace(focused.clone());
                    sidebar_state.focus_active_place();
                    state.window.set_focus_visible(true);
                }
                return glib::Propagation::Stop;
            }
            if sidebar_has_focus {
                let restored = focus_before_sidebar
                    .borrow_mut()
                    .take()
                    .is_some_and(|widget| widget.grab_focus());
                if !restored {
                    browser.focus_active();
                }
            } else {
                focus_before_sidebar.replace(focused.clone());
                if !sidebar_toggle.is_active() {
                    sidebar_toggle.set_active(true);
                }
                let sidebar = sidebar_state.clone();
                glib::idle_add_local_once(move || {
                    sidebar.focus_active_place();
                });
            }
            return glib::Propagation::Stop;
        }
        if press.is(if preferences.tenxer_mode() {
            TENXER_SIDEBAR
        } else {
            SIDEBAR
        }) {
            if state.destination.root_limit.is_none() {
                sidebar_toggle.emit_clicked();
            }
            return glib::Propagation::Stop;
        }
        if state.view.location_has_focus() {
            return glib::Propagation::Proceed;
        }
        if press.is(CHOOSE_FOLDER.ignoring(ModifierType::SUPER_MASK))
            && state.view.item_view_has_focus()
        {
            state.accept();
            return glib::Propagation::Stop;
        }
        if press.is(NEW_FOLDER) {
            if state.destination.allow_create {
                state.view.create_new_folder();
            }
            return glib::Propagation::Stop;
        }
        if press.is(REFRESH.ignoring(SHORTCUT_MODIFIERS)) {
            if let Some(depth) = browser.active_depth() {
                crate::adapters::trash_support::forget_all();
                browser.retry_column(depth);
            }
            return glib::Propagation::Stop;
        }
        if press.is_any(&[HIDDEN_FILES, HIDDEN_FILES_PERIOD]) {
            browser.toggle_hidden();
            return glib::Propagation::Stop;
        }
        let permanent = if press.is(DELETE.ignoring(ModifierType::SUPER_MASK)) {
            Some(false)
        } else if press.is(DELETE_PERMANENTLY.ignoring(ModifierType::SUPER_MASK)) {
            Some(true)
        } else {
            None
        };
        if let Some(permanent) = permanent
            && state.view.item_view_has_focus()
            && !state.view.filter_has_focus()
            && state.view.confirm_delete(permanent)
        {
            return glib::Propagation::Stop;
        }
        if let Some(mode) = super::window::browser_mode_for_digit(press) {
            super::window::apply_browser_mode(&state.view, &PreferenceManager::shared(), mode);
            return glib::Propagation::Stop;
        }
        if control {
            return glib::Propagation::Proceed;
        }
        let column_popover = focused
            .as_ref()
            .and_then(|focused| focused.ancestor(gtk::Popover::static_type()))
            .and_downcast::<gtk::Popover>()
            .filter(|popover| popover.has_css_class("column-popover"));
        if let Some(popover) = column_popover
            && let Some(direction) = vim_focus_direction(press)
        {
            popover.child_focus(direction);
            return glib::Propagation::Stop;
        }
        let tab = plain_tab_direction(press);
        if !preferences.tenxer_mode()
            && let Some(direction) = tab
            && !focused
                .as_ref()
                .is_some_and(super::focus_navigation::in_popover)
            && state.view.default_tab(direction)
        {
            return glib::Propagation::Stop;
        }
        if !alt && let Some(focused) = focused.as_ref() {
            if super::focus_navigation::in_popover(focused) {
                return glib::Propagation::Proceed;
            }
            if press.is(ENTER.ignoring(ModifierType::SUPER_MASK))
                && super::focus_navigation::activate(state.window.upcast_ref())
            {
                return glib::Propagation::Stop;
            }
            if super::focus_navigation::editable(focused) {
                return glib::Propagation::Proceed;
            }
        }
        if preferences.tenxer_mode()
            && state.view.item_view_has_focus()
            && tab == Some(gtk::DirectionType::TabForward)
        {
            if header.child_focus(gtk::DirectionType::TabForward) {
                state.window.set_focus_visible(true);
            }
            return glib::Propagation::Stop;
        }
        if preferences.tenxer_mode() && tab.is_some() {
            if !super::focus_navigation::contains_widget(&state.view.widget(), focused.as_ref()) {
                browser.focus_active();
            }
            return glib::Propagation::Stop;
        }
        let mut header_left_boundary = false;
        if state.view.header_actions_have_focus() {
            if press.is_any(&[VIM_LEFT, LEFT.ignoring(FOCUS_MODIFIERS)]) {
                if state.view.move_header_focus(gtk::DirectionType::Left)
                    || preferences.tenxer_mode()
                {
                    return glib::Propagation::Stop;
                }
                header_left_boundary = true;
            } else if press.is_any(&[VIM_RIGHT, RIGHT.ignoring(FOCUS_MODIFIERS)]) {
                state.view.move_header_focus(gtk::DirectionType::Right);
                return glib::Propagation::Stop;
            } else if press.is_any(&[VIM_DOWN, DOWN.ignoring(FOCUS_MODIFIERS)]) {
                state.view.focus_items_from_header();
                return glib::Propagation::Stop;
            }
        }
        if sidebar_has_focus
            && !preferences.tenxer_mode()
            && let Some(direction) =
                vim_focus_direction(press).or_else(|| arrow_direction(press, FOCUS_MODIFIERS))
        {
            if direction == gtk::DirectionType::Right {
                focus_before_sidebar.borrow_mut().take();
                browser.focus_active();
            } else if !sidebar_widget.child_focus(direction) && direction == gtk::DirectionType::Up
            {
                sidebar_toggle.grab_focus();
                state.window.set_focus_visible(true);
            }
            return glib::Propagation::Stop;
        }
        if press.is(BACKSPACE.ignoring(FOCUS_MODIFIERS))
            && state.view.dismiss_empty_focused_filter()
        {
            return glib::Propagation::Stop;
        }
        if !alt && !state.view.item_view_has_focus() && !header_left_boundary {
            if preferences.tenxer_mode()
                && (arrow_direction(press, FOCUS_MODIFIERS).is_some()
                    || vim_focus_direction(press).is_some())
            {
                browser.focus_active();
                if vim_focus_direction(press).is_some() {
                    return glib::Propagation::Stop;
                }
            } else if let Some(direction) = arrow_direction(press, ModifierType::SUPER_MASK) {
                if super::focus_navigation::move_focus(state.window.upcast_ref(), direction) {
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            } else {
                return glib::Propagation::Proceed;
            }
        }
        if press.is(LEFT.ignoring(ModifierType::SUPER_MASK))
            && sidebar_toggle.is_active()
            && state.view.item_view_has_focus()
            && state.view.item_at_sidebar_edge()
            && !PreferenceManager::shared().arrow_navigation_scoped_active()
        {
            focus_before_sidebar.replace(focused.clone());
            sidebar_state.focus_active_place();
            state.window.set_focus_visible(true);
            return glib::Propagation::Stop;
        }
        if press.is(SPACE.ignoring(FOCUS_MODIFIERS)) {
            if press.is(SPACE) && state.view.activate_directory_on_space() {
                return glib::Propagation::Stop;
            }
            preview.toggle(
                preview_target(browser.focused_entry()),
                browser.active_depth(),
            );
            return glib::Propagation::Stop;
        }
        if press.is(BACKSPACE.ignoring(FOCUS_MODIFIERS)) {
            state.view.navigate_up();
            return glib::Propagation::Stop;
        }
        if !alt
            && state.view.view_mode() != super::browser_modes::BrowserMode::Columns
            && let Some(direction) = arrow_direction(press, FOCUS_MODIFIERS)
        {
            if state.view.cross_type_group(direction, false) {
                return glib::Propagation::Stop;
            }
            if press.is(UP.ignoring(ModifierType::SUPER_MASK))
                && !PreferenceManager::shared().arrow_navigation_scoped_active()
                && state.view.focus_header_from_top_item()
            {
                return glib::Propagation::Stop;
            }
            // Keep GTK's spatial movement, then reconcile selection in visual order.
            let weak = Rc::downgrade(state);
            glib::idle_add_local_once(move || {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                if !state.view.item_view_has_focus() {
                    return;
                }
                state.view.synchronize_native_selection(false);
            });
            if vim_navigation && let Some(arrow) = super::focus_navigation::spatial_arrow(press) {
                super::focus_navigation::activate_native_arrow(&state.window, arrow.key());
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }
        let alt_super = ModifierType::ALT_MASK.union(ModifierType::SUPER_MASK);
        if state.view.item_view_has_focus()
            && let Some(query) = super::window::type_to_search_query(press)
            && preferences.type_to_search()
            && match query {
                super::window::TypeToSearchQuery::Empty => state.view.show_filter(),
                super::window::TypeToSearchQuery::Character(character) => {
                    state.view.show_filter_with_query(&character.to_string())
                }
            }
        {
            return glib::Propagation::Stop;
        }
        if press.is_any(&[VIM_UP, UP.ignoring(alt_super)])
            && !PreferenceManager::shared().arrow_navigation_scoped_active()
            && state.view.focus_header_from_top_item()
        {
            return glib::Propagation::Stop;
        }
        if press.is(HISTORY_BACK.ignoring(FOCUS_MODIFIERS)) {
            browser.back();
        } else if press.is(HISTORY_FORWARD.ignoring(FOCUS_MODIFIERS)) {
            browser.forward();
        } else if press.is(PARENT_FOLDER.ignoring(FOCUS_MODIFIERS)) {
            browser.parent();
        } else if press.is(HOME_FOLDER.ignoring(FOCUS_MODIFIERS)) {
            browser.navigate(Location::local(home_directory()));
        } else if press.is_any(&[VIM_DOWN, DOWN.ignoring(FOCUS_MODIFIERS)]) {
            browser.move_selection(1);
        } else if press.is_any(&[VIM_UP, UP.ignoring(FOCUS_MODIFIERS)]) {
            browser.move_selection(-1);
        } else if press.is_any(&[VIM_LEFT, LEFT.ignoring(FOCUS_MODIFIERS)]) {
            if state.view.first_column_has_focus()
                && sidebar_toggle.is_active()
                && !PreferenceManager::shared().arrow_navigation_scoped_active()
            {
                focus_before_sidebar.replace(focused.clone());
                sidebar_state.focus_active_place();
            } else {
                state.view.navigate_left();
            }
        } else if press.is(RIGHT.ignoring(FOCUS_MODIFIERS))
            && state.view.view_mode() == BrowserMode::Columns
        {
            browser.enter_focused_directory();
        } else if press.is_any(&[VIM_RIGHT, ENTER.ignoring(FOCUS_MODIFIERS)]) {
            if !tenxer.confirm_focused() {
                state.view.activate_focused();
            }
        } else {
            return glib::Propagation::Proceed;
        }
        glib::Propagation::Stop
    };
    keys::on_press(&keys, move |_, press| {
        let Some(state) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        match handle(&state, press) {
            glib::Propagation::Proceed => edit_focused_text(&state.window, press),
            handled => handled,
        }
    });
    window.add_controller(keys.clone());
    let keys = keys.downgrade();
    window.connect_unrealize(move |window| {
        if let Some(keys) = keys.upgrade() {
            window.remove_controller(&keys);
        }
    });
}
