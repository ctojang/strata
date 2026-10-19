// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    process::Command,
    rc::Rc,
};

use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::assets::icons;

#[cfg(test)]
mod tests;

mod about;
mod actions;
mod bindings;
mod diagnostics;
mod exclusions;
mod general;
mod install_command;
mod integrations;
mod search;
mod theme;
mod wrap;
use about::about_page;
use general::general_page;
use theme::theme_page;

use super::{
    blur::BlurBin,
    keys::{self, bindings::SELECT_ALL},
    modal::close_on_escape,
    preferences::PreferenceManager,
};

struct ResponsiveContent {
    flows: Vec<(gtk::FlowBox, u32)>,
    setting_rows: Vec<gtk::Box>,
    activation_rows: Vec<ResponsiveActivationRow>,
}

pub struct ResponsiveActivationRow {
    row: gtk::Box,
    options: Vec<gtk::Box>,
}

const DIALOG_WIDTH: i32 = 1400;
const DIALOG_HEIGHT: i32 = 1024;
const DIALOG_MARGIN: i32 = 24;
const COMPACT_NAVIGATION_BREAKPOINT: i32 = 900;
// Reflow the content before collapsing navigation: desktop toolbars need more room.
const COMPACT_CONTENT_BREAKPOINT: i32 = 1250;
const MIN_SIDE_BY_SIDE_ACTIVATION_WIDTH: i32 = 620;
const STACK_ACTIVATION_OPTIONS_BREAKPOINT: i32 = 350;
const STACK_TEXT_SIZE_BREAKPOINT: i32 = 600;
const STACK_EXCLUSION_INPUT_BREAKPOINT: i32 = 1000;

mod responsive_bin {
    use super::*;

    #[derive(Default)]
    pub struct ResponsiveBin {
        pub compact_navigation: Cell<bool>,
        pub compact_content: Cell<bool>,
        pub typography_scale: Cell<f64>,
        // Matrix widths below which the translated choices were seen squeezed: side by side,
        // or with each label beside its control.
        pub activation_side_by_side_min: Cell<i32>,
        pub activation_inline_options_min: Cell<i32>,
        pub navigation: RefCell<Option<gtk::Box>>,
        pub navigation_heading: RefCell<Option<gtk::Label>>,
        pub navigation_labels: RefCell<Vec<gtk::Label>>,
        pub navigation_contents: RefCell<Vec<gtk::Box>>,
        pub responsive_flows: RefCell<Vec<(gtk::FlowBox, u32)>>,
        pub responsive_setting_rows: RefCell<Vec<gtk::Box>>,
        pub responsive_activation_rows: RefCell<Vec<ResponsiveActivationRow>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ResponsiveBin {
        const NAME: &'static str = "StrataSettingsResponsiveBin";
        type Type = super::ResponsiveBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ResponsiveBin {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ResponsiveBin {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let natural = match orientation {
                gtk::Orientation::Horizontal => DIALOG_WIDTH + DIALOG_MARGIN * 2,
                gtk::Orientation::Vertical => DIALOG_HEIGHT + DIALOG_MARGIN * 2,
                _ => unreachable!("GTK orientations are horizontal or vertical"),
            };
            (1, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.obj().first_child() else {
                return;
            };
            let (child_width, child_height) = responsive_dialog_size(width, height);
            let logical_width = f64::from(child_width) / self.typography_scale.get().max(0.1);
            let compact_content = logical_width < f64::from(COMPACT_CONTENT_BREAKPOINT);
            let compact = uses_compact_navigation(
                (f64::from(child_width) / self.typography_scale.get().max(0.1)) as i32,
            );
            if self.compact_navigation.replace(compact) != compact {
                if let Some(navigation) = self.navigation.borrow().as_ref() {
                    search::set_compact(navigation, compact);
                    if compact {
                        navigation.add_css_class("compact");
                    } else {
                        navigation.remove_css_class("compact");
                    }
                }
                if let Some(heading) = self.navigation_heading.borrow().as_ref() {
                    heading.set_visible(!compact);
                }
                for label in self.navigation_labels.borrow().iter() {
                    label.set_visible(!compact);
                }
                for content in self.navigation_contents.borrow().iter() {
                    content.set_halign(if compact {
                        gtk::Align::Center
                    } else {
                        gtk::Align::Fill
                    });
                }
            }
            if self.compact_content.replace(compact_content) != compact_content {
                let compact = compact_content;
                for (flow, expanded_columns) in self.responsive_flows.borrow().iter() {
                    flow.set_max_children_per_line(if compact { 1 } else { *expanded_columns });
                }
                for row in self.responsive_setting_rows.borrow().iter() {
                    row.set_orientation(if compact {
                        gtk::Orientation::Vertical
                    } else {
                        gtk::Orientation::Horizontal
                    });
                    row.set_spacing(if compact { 8 } else { 16 });
                }
            }
            let available_activation_width = child_width - if compact { 100 } else { 330 };
            let scaled_activation_width = MIN_SIDE_BY_SIDE_ACTIVATION_WIDTH as f64
                + (self.typography_scale.get() - 1.0).max(0.0) * 320.0;
            let matrix_width = activation_matrix_width(&self.responsive_activation_rows.borrow());
            let activation_compact = f64::from(available_activation_width)
                < scaled_activation_width
                || matrix_width < self.activation_side_by_side_min.get();
            let stack_activation_options = available_activation_width
                < STACK_ACTIVATION_OPTIONS_BREAKPOINT
                || matrix_width < self.activation_inline_options_min.get();
            for responsive_row in self.responsive_activation_rows.borrow().iter() {
                responsive_row.row.set_orientation(if activation_compact {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                });
                responsive_row
                    .row
                    .set_spacing(if activation_compact { 4 } else { 24 });
                for option in &responsive_row.options {
                    option.set_orientation(if stack_activation_options {
                        gtk::Orientation::Vertical
                    } else {
                        gtk::Orientation::Horizontal
                    });
                    option.set_spacing(if stack_activation_options { 2 } else { 6 });
                    option.set_halign(if activation_compact && !stack_activation_options {
                        gtk::Align::End
                    } else {
                        gtk::Align::Fill
                    });
                }
                if activation_compact {
                    responsive_row.row.add_css_class("compact");
                } else {
                    responsive_row.row.remove_css_class("compact");
                }
            }
            reflow_settings(
                &child,
                compact_content,
                activation_compact,
                logical_width < f64::from(STACK_TEXT_SIZE_BREAKPOINT),
                logical_width < f64::from(STACK_EXCLUSION_INPUT_BREAKPOINT),
            );
            let x = ((width - child_width) / 2) as f32;
            let y = ((height - child_height) / 2) as f32;
            let transform = gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x, y));
            child.allocate(child_width, child_height, baseline, Some(transform));
            self.stack_squeezed_activation_choices(activation_compact, stack_activation_options);
        }
    }

    impl ResponsiveBin {
        /// Long translated choices can need more room than the fixed breakpoints allow; once
        /// a choice gets less than its natural width, stack the matrix further at this width.
        fn stack_squeezed_activation_choices(&self, compact: bool, stacked_options: bool) {
            if compact && stacked_options {
                return;
            }
            let rows = self.responsive_activation_rows.borrow();
            let squeezed = rows.iter().filter(|row| row.row.is_mapped()).any(|row| {
                row.options
                    .iter()
                    .filter_map(|option| option.last_child())
                    .any(|control| choice_label_wraps(&control))
            });
            if !squeezed {
                return;
            }
            let threshold = activation_matrix_width(&rows) + 1;
            if compact {
                self.activation_inline_options_min.set(threshold);
            } else {
                self.activation_side_by_side_min.set(threshold);
            }
            let bin = self.obj().downgrade();
            glib::idle_add_local_once(move || {
                if let Some(bin) = bin.upgrade() {
                    bin.queue_allocate();
                }
            });
        }
    }
}

glib::wrapper! {
    pub struct ResponsiveBin(ObjectSubclass<responsive_bin::ResponsiveBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ResponsiveBin {
    fn new(
        child: &impl IsA<gtk::Widget>,
        navigation: &gtk::Box,
        navigation_heading: &gtk::Label,
        navigation_labels: Vec<gtk::Label>,
        navigation_contents: Vec<gtk::Box>,
        responsive: ResponsiveContent,
    ) -> Self {
        let bin: Self = glib::Object::new();
        let imp = bin.imp();
        imp.navigation.replace(Some(navigation.clone()));
        imp.navigation_heading
            .replace(Some(navigation_heading.clone()));
        imp.navigation_labels.replace(navigation_labels);
        imp.navigation_contents.replace(navigation_contents);
        imp.responsive_flows.replace(responsive.flows);
        imp.responsive_setting_rows.replace(responsive.setting_rows);
        imp.responsive_activation_rows
            .replace(responsive.activation_rows);
        child.set_parent(&bin);
        PreferenceManager::shared().bind_interface_scale(&bin, |widget, scale| {
            let bin = widget
                .downcast_ref::<ResponsiveBin>()
                .expect("settings bin");
            bin.imp().typography_scale.set(scale);
            bin.imp().activation_side_by_side_min.set(0);
            bin.imp().activation_inline_options_min.set(0);
            bin.queue_allocate();
        });
        bin
    }

    fn add_navigation(&self, label: gtk::Label, content: gtk::Box) {
        let imp = self.imp();
        imp.navigation_labels.borrow_mut().push(label);
        imp.navigation_contents.borrow_mut().push(content);
    }

    fn add_flow(&self, flow: gtk::FlowBox, columns: u32) {
        flow.set_max_children_per_line(if self.imp().compact_content.get() {
            1
        } else {
            columns
        });
        self.imp()
            .responsive_flows
            .borrow_mut()
            .push((flow, columns));
    }
}

/// Whether a segmented control gives any choice less than its one-line width.
fn choice_label_wraps(control: &gtk::Widget) -> bool {
    let mut choice = control.first_child();
    while let Some(button) = choice {
        choice = button.next_sibling();
        if let Some(label) = button.downcast_ref::<gtk::Button>().and_then(|b| b.child())
            && label.width() < label.measure(gtk::Orientation::Horizontal, -1).1
        {
            return true;
        }
    }
    false
}

fn activation_matrix_width(rows: &[ResponsiveActivationRow]) -> i32 {
    rows.first()
        .and_then(|row| row.row.parent())
        .map_or(0, |matrix| matrix.width())
}

// Reports no horizontal minimum, so the page scroller gives the page the viewport
// width instead of the content's own width. A page wider than the viewport would
// otherwise turn the scroller into a sideways pan over the settings.
mod page_content_bin {
    use super::*;

    #[derive(Default)]
    pub struct PageContentBin;

    #[glib::object_subclass]
    impl ObjectSubclass for PageContentBin {
        const NAME: &'static str = "StrataSettingsPageContentBin";
        type Type = super::PageContentBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PageContentBin {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for PageContentBin {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            // The page height depends on the width, because description labels
            // wrap; without this the scroller caches a single-line height
            // measured before the viewport width is known and the page stops
            // short of its own content.
            self.obj()
                .first_child()
                .map_or(gtk::SizeRequestMode::ConstantSize, |child| {
                    child.request_mode()
                })
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let (minimum, natural, minimum_baseline, natural_baseline) =
                child.measure(orientation, for_size);
            let minimum = if orientation == gtk::Orientation::Horizontal {
                0
            } else {
                minimum
            };
            (minimum, natural, minimum_baseline, natural_baseline)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    pub struct PageContentBin(ObjectSubclass<page_content_bin::PageContentBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PageContentBin {
    fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let bin: Self = glib::Object::new();
        child.set_parent(&bin);
        bin
    }
}

fn reflow_settings(
    widget: &gtk::Widget,
    compact: bool,
    activation_compact: bool,
    stack_text_size: bool,
    stack_exclusion_input: bool,
) {
    if widget.has_css_class("settings-dialog") {
        if compact {
            widget.add_css_class("compact");
        } else {
            widget.remove_css_class("compact");
        }
    }
    if let Some(row) = widget.downcast_ref::<gtk::Box>()
        && [
            "settings-option",
            "settings-library-toolbar",
            "about-identity",
            "theme-library-footer",
            "settings-inline-description",
        ]
        .iter()
        .any(|class| row.has_css_class(class))
    {
        let switch_row = row
            .last_child()
            .is_some_and(|child| child.is::<gtk::Switch>());
        // Short numeric controls can stay beside wrapping copy after other rows stack.
        let stack_row = if row.has_css_class("settings-renderer-row")
            || row.has_css_class("settings-exclusions-row")
        {
            true
        } else if row.has_css_class("settings-text-size-row") {
            stack_text_size
        } else {
            compact
        };
        let orientation = if stack_row && !switch_row {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        };
        if !row.has_css_class("settings-sidebar-places") && row.orientation() != orientation {
            row.set_orientation(orientation);
        }
    }
    if widget.has_css_class("settings-integration-actions")
        && let Some(actions) = widget.downcast_ref::<gtk::Box>()
    {
        actions.set_orientation(if compact {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
    }
    if widget.has_css_class("theme-appearance-filter")
        || widget.has_css_class("settings-integration-actions")
    {
        widget.set_halign(if compact {
            gtk::Align::Fill
        } else {
            gtk::Align::End
        });
        widget.set_hexpand(compact);
        let mut child = widget.first_child();
        while let Some(button) = child {
            child = button.next_sibling();
            button.set_hexpand(compact);
        }
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && (label.has_css_class("settings-nowrap") || label.has_css_class("menu-heading"))
    {
        label.set_wrap(compact && !label.has_css_class("settings-keycap"));
    }
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && !widget.is::<gtk::ToggleButton>()
        && let Some(label) = button.child().and_downcast::<gtk::Label>()
    {
        label.set_wrap(compact);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    }
    // Keep the exclusion placeholder readable instead of squeezing it beside translated buttons.
    if widget.has_css_class("settings-exclusions-input")
        && let Some(row) = widget.downcast_ref::<gtk::Box>()
    {
        let orientation = if stack_exclusion_input {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        };
        if row.orientation() != orientation {
            row.set_orientation(orientation);
        }
    }
    if widget.has_css_class("activation-header") {
        widget.set_visible(!activation_compact);
    }
    if widget.has_css_class("activation-inline-label") {
        widget.set_visible(activation_compact);
    }
    // Beside the choices, keep long single-word view names whole; stacked, they have the full width.
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && label.has_css_class("click-activation-title")
    {
        let mode = if activation_compact {
            gtk::pango::WrapMode::WordChar
        } else {
            gtk::pango::WrapMode::Word
        };
        if label.wrap_mode() != mode {
            label.set_wrap_mode(mode);
        }
    }
    let mut child = widget.first_child();
    while let Some(next) = child {
        child = next.next_sibling();
        reflow_settings(
            &next,
            compact,
            activation_compact,
            stack_text_size,
            stack_exclusion_input,
        );
    }
}

fn responsive_dialog_size(width: i32, height: i32) -> (i32, i32) {
    (
        DIALOG_WIDTH.min((width - DIALOG_MARGIN * 2).max(1)),
        DIALOG_HEIGHT.min((height - DIALOG_MARGIN * 2).max(1)),
    )
}

fn uses_compact_navigation(dialog_width: i32) -> bool {
    dialog_width < COMPACT_NAVIGATION_BREAKPOINT
}

/// Shows a directory in the hosting window's browser.
pub(crate) type OpenFolder = Rc<dyn Fn(std::path::PathBuf)>;

#[expect(
    deprecated,
    reason = "GTK 4.12 deprecated translate_coordinates and allocation without a replacement for click-in-bounds checks"
)]
pub fn build_layer(
    settings_button: &gtk::Button,
    root: &BlurBin,
    preferences: Rc<PreferenceManager>,
    open_folder: OpenFolder,
) -> gtk::Box {
    let layer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layer.add_css_class("app-modal-layer");
    layer.add_css_class("settings-backdrop");
    layer.set_halign(gtk::Align::Fill);
    layer.set_valign(gtk::Align::Fill);
    layer.set_hexpand(true);
    layer.set_vexpand(true);
    layer.set_focusable(true);
    layer.set_visible(false);

    let panel = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    panel.add_css_class("settings-dialog");
    panel.set_overflow(gtk::Overflow::Hidden);

    let navigation = gtk::Box::new(gtk::Orientation::Vertical, 2);
    navigation.add_css_class("settings-navigation");
    let navigation_heading = append_heading(&navigation, "SETTINGS");
    let settings_search = search::append(&navigation);

    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("settings-page");
    page.set_hexpand(true);
    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    titlebar.add_css_class("settings-titlebar");
    let title = gtk::Label::new(Some(&crate::i18n::tr("General")));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("settings-title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(1);
    let close = gtk::Button::builder()
        .tooltip_text(crate::i18n::tr("Close settings"))
        .build();
    close.set_child(Some(&crate::assets::primary_icon(icons::X, 18)));
    close.add_css_class("settings-close");
    close.set_valign(gtk::Align::Center);
    titlebar.append(&title);
    titlebar.append(&close);
    page.append(&titlebar);

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(120)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .hexpand(true)
        .vexpand(true)
        .build();
    let (general, responsive_setting_rows, responsive_activation_rows) =
        general_page(preferences.clone());
    stack.add_named(&general, Some("general"));
    stack.add_named(&diagnostics::diagnostics_page(), Some("diagnostics"));
    stack.add_named(&about_page(), Some("about"));
    page.append(&stack);

    // Created before the navigation loop so lazy builders can register
    // their responsive rows and flows as pages materialize.
    let responsive_panel = ResponsiveBin::new(
        &panel,
        &navigation,
        &navigation_heading,
        Vec::new(),
        Vec::new(),
        ResponsiveContent {
            flows: Vec::new(),
            setting_rows: responsive_setting_rows,
            activation_rows: responsive_activation_rows,
        },
    );
    // Navigation entries register as their buttons are created, so the
    // responsive panel compacts correctly even with lazy pages.
    let responsive_for_nav = responsive_panel.clone();
    let built: Rc<RefCell<std::collections::HashSet<&'static str>>> = Rc::new(RefCell::new(
        ["general", "diagnostics", "about"].into_iter().collect(),
    ));
    let dismiss_hooks = DismissHooks::default();
    let close_settings: Rc<dyn Fn()> = {
        let layer = layer.clone();
        let button = settings_button.clone();
        let root = root.clone();
        let hooks = dismiss_hooks.clone();
        Rc::new(move || hide(&layer, &button, &root, &hooks))
    };
    let nav_buttons: Rc<RefCell<Vec<(gtk::Button, &'static str)>>> =
        Rc::new(RefCell::new(Vec::new()));
    for entry in &NAVIGATION {
        let name = entry.page;
        let active = name == "general";
        let (button, navigation_label, navigation_content) = navigation_button(entry);
        responsive_for_nav.add_navigation(navigation_label, navigation_content);
        if active {
            button.add_css_class("settings-nav-active");
        }
        nav_buttons.borrow_mut().push((button.clone(), name));
        let buttons = nav_buttons.clone();
        let stack = stack.clone();
        let title = title.clone();
        let page_title = entry.label;
        let built = built.clone();
        let preferences = preferences.clone();
        let responsive_panel = responsive_panel.clone();
        let search_state = settings_search.state.clone();
        let dismiss_hooks = dismiss_hooks.clone();
        let open_folder = open_folder.clone();
        let close_settings = close_settings.clone();
        button.connect_clicked(move |clicked| {
            for (candidate, _) in buttons.borrow().iter() {
                if candidate == clicked {
                    candidate.add_css_class("settings-nav-active");
                } else {
                    candidate.remove_css_class("settings-nav-active");
                }
            }
            if built.borrow_mut().insert(name) {
                match name {
                    "theme" => {
                        let page =
                            theme_page(preferences.clone(), super::theme::ThemeManager::shared());
                        search::apply(&page.widget, &search_state);
                        stack.add_named(&page.widget, Some("theme"));
                        for (flow, columns) in page.flows {
                            responsive_panel.add_flow(flow, columns);
                        }
                        dismiss_hooks.add(page.dismiss);
                    }
                    "actions" => {
                        let page = actions::actions_page();
                        search::apply(&page, &search_state);
                        stack.add_named(&page, Some("actions"));
                    }
                    "integrations" => {
                        let open_folder = open_folder.clone();
                        let close_settings = close_settings.clone();
                        let page = integrations::integrations_page(Some(Rc::new(move |path| {
                            close_settings();
                            open_folder(path);
                        })));
                        search::apply(&page, &search_state);
                        stack.add_named(&page, Some("integrations"));
                    }
                    _ => {}
                }
            }
            stack.set_visible_child_name(name);
            title.set_text(&crate::i18n::tr(page_title));
        });
        navigation.append(&button);
    }

    settings_search.install(&stack, &title, &nav_buttons);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    navigation.append(&spacer);

    let navigation_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_width(true)
        .child(&navigation)
        .build();
    panel.append(&navigation_scroll);
    panel.append(&page);
    responsive_panel.set_hexpand(false);
    responsive_panel.set_vexpand(false);
    let top = gtk::Box::new(gtk::Orientation::Vertical, 0);
    top.set_vexpand(true);
    let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bottom.set_vexpand(true);
    let left = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    left.set_hexpand(true);
    let right = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    right.set_hexpand(true);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.append(&left);
    row.append(&responsive_panel);
    row.append(&right);
    layer.append(&top);
    layer.append(&row);
    layer.append(&bottom);

    // A window closed mid-preview never runs `hide`; the preview is process-wide.
    layer.connect_unrealize(move |_| dismiss_hooks.run());
    let close_hide = close_settings.clone();
    close.connect_clicked(move |_| close_hide());
    let escape_hide = close_settings.clone();
    close_on_escape(&layer, move || escape_hide());

    let click_layer = layer.clone();
    let click_dialog = responsive_panel.clone();
    let click = gtk::GestureClick::new();
    click.connect_pressed(move |_, _, x, y| {
        let on_dialog = click_dialog
            .translate_coordinates(&click_layer, 0.0, 0.0)
            .is_some_and(|(dx, dy)| {
                let alloc = click_dialog.allocation();
                x >= dx
                    && x < dx + alloc.width() as f64
                    && y >= dy
                    && y < dy + alloc.height() as f64
            });
        if !on_dialog {
            close_settings();
        }
    });
    layer.add_controller(click);
    super::focus_navigation::install(&layer);
    layer
}

/// Work that every route closing Settings must run, such as discarding a theme preview.
type DismissHookList = RefCell<Vec<Rc<dyn Fn()>>>;

#[derive(Clone, Default)]
struct DismissHooks(Rc<DismissHookList>);

impl DismissHooks {
    fn add(&self, hook: Rc<dyn Fn()>) {
        self.0.borrow_mut().push(hook);
    }

    /// Hooks are idempotent: they run on every close and again when the layer unrealizes.
    fn run(&self) {
        let hooks = self.0.borrow().clone();
        for hook in hooks {
            hook();
        }
    }
}

fn hide(layer: &gtk::Box, button: &gtk::Button, root: &BlurBin, hooks: &DismissHooks) {
    hooks.run();
    if layer.has_css_class("dismissing") {
        return;
    }
    layer.add_css_class("dismissing");
    layer.set_sensitive(false);
    let layer_for_anim = layer.clone();
    let layer = layer.clone();
    let root = root.clone();
    let button = button.clone();
    super::browser::animate_out(&layer_for_anim, move || {
        layer.set_visible(false);
        layer.remove_css_class("dismissing");
        layer.set_sensitive(true);
        root.set_blurred(false);
        button.remove_css_class("active");
    });
}

fn process_start_time(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm (field 2) may contain spaces or parentheses; the remaining fields
    // begin after its final closing parenthesis. starttime is field 22.
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
}

fn restart_waiter(current_exe: &std::path::Path, parent_pid: u32) -> Option<Command> {
    use std::{os::unix::process::CommandExt, process::Stdio};

    let mut command = crate::trusted_command::command("sh").ok()?;
    let sleep = crate::trusted_command::resolve("sleep").ok()?;
    let start_time = process_start_time(parent_pid).unwrap_or_default();
    command
        .args([
            "-c",
            "pid=$1; binary=$2; sleeper=$3; born=$4; while kill -0 \"$pid\" 2>/dev/null && [ -r \"/proc/$pid/stat\" ]; do IFS= read -r stat < \"/proc/$pid/stat\" || break; rest=${stat##*) }; set -- $rest; shift 19; [ \"${1:-}\" = \"$born\" ] || break; \"$sleeper\" 0.1; done; \"$sleeper\" 0.5; exec \"$binary\"",
            "strata-restart",
        ])
        .arg(parent_pid.to_string())
        .arg(current_exe)
        .arg(sleep)
        .arg(start_time)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if std::env::var_os(crate::CAIRO_SELECTED_BY_STRATA).as_deref()
        == Some(std::ffi::OsStr::new("1"))
        && std::env::var_os("GSK_RENDERER").as_deref() == Some(std::ffi::OsStr::new("cairo"))
    {
        // The next process must read the saved renderer, not inherit our default.
        command.env_remove("GSK_RENDERER");
        command.env_remove(crate::CAIRO_SELECTED_BY_STRATA);
    }
    Some(command)
}

/// A closed window may no longer have an application; never exit the process in that case.
fn restart(application: Option<&gtk::Application>) {
    let Some(application) = application else {
        return;
    };
    let Ok(current_exe) = crate::services::installed_executable() else {
        return;
    };
    // Wait for this process to exit completely before relaunching. A fixed
    // delay could overlap the old and new GTK/Wayland clients and rapidly hand
    // keyboard focus through an underlying terminal. Besides re-activating the
    // old GApplication instance, that exposed a Foot/libxkbcommon crash on
    // affected systems. Detach the waiter from inherited terminal streams and
    // put it in its own process group so restarting cannot disturb the terminal
    // that launched Strata.
    let Some(mut waiter) = restart_waiter(&current_exe, std::process::id()) else {
        return;
    };
    if restart_blocker_shown(application) {
        return;
    }
    let Ok(mut waiter) = waiter.spawn() else {
        return;
    };
    // Do not leave a waiter behind when an unregistered close handler refuses.
    if !close_windows_for_restart(application) {
        let _ = waiter.kill();
        let _ = waiter.wait();
        return;
    }
    application.quit();
}

/// Checks every window before closing any, so a refusal cannot leave some closed.
fn restart_blocker_shown(application: &gtk::Application) -> bool {
    let Some((window, blocker)) = crate::ui::close_guard::application_blocker(application) else {
        return false;
    };
    window.present();
    blocker.show(&window);
    true
}

fn close_windows_for_restart(application: &gtk::Application) -> bool {
    if restart_blocker_shown(application) {
        return false;
    }
    for window in application.windows() {
        window.close();
    }
    application.windows().is_empty()
}

struct NavigationEntry {
    page: &'static str,
    label: &'static str,
    subtitle: &'static str,
    icon: &'static str,
    accessible_label: Option<&'static str>,
}

/// Settings pages in navigation order. Search finds page titles here and routes
/// to the button built from each entry.
static NAVIGATION: [NavigationEntry; 6] = [
    NavigationEntry {
        page: "general",
        label: "General",
        subtitle: "Browsing, search, files",
        icon: icons::SLIDERS,
        accessible_label: None,
    },
    NavigationEntry {
        page: "theme",
        label: "Appearance",
        subtitle: "Theme, text, motion",
        icon: icons::PALETTE,
        accessible_label: Some("Appearance settings"),
    },
    NavigationEntry {
        page: "actions",
        label: "Actions",
        subtitle: "Custom actions",
        icon: icons::PLAY,
        accessible_label: None,
    },
    NavigationEntry {
        page: "integrations",
        label: "Integrations",
        subtitle: "Apps and services",
        icon: icons::BLOCKS,
        accessible_label: None,
    },
    NavigationEntry {
        page: "diagnostics",
        label: "Diagnostics",
        subtitle: "Troubleshooting",
        icon: icons::STETHOSCOPE,
        accessible_label: None,
    },
    NavigationEntry {
        page: "about",
        label: "About",
        subtitle: "Version and links",
        icon: icons::INFO,
        accessible_label: None,
    },
];

fn navigation_button(entry: &NavigationEntry) -> (gtk::Button, gtk::Label, gtk::Box) {
    let label = entry.label;
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let icon_image = crate::assets::primary_icon(entry.icon, 18);
    let text = gtk::Label::new(None);
    text.set_markup(&format!(
        "{}\n<span size=\"small\" weight=\"normal\">{}</span>",
        glib::markup_escape_text(&crate::i18n::tr(label)),
        glib::markup_escape_text(&crate::i18n::tr(entry.subtitle))
    ));
    text.set_xalign(0.0);
    text.add_css_class("settings-nav-copy");
    content.append(&icon_image);
    content.append(&text);
    let button = gtk::Button::builder().child(&content).build();
    button.set_widget_name(entry.page);
    button.set_has_frame(false);
    button.set_cursor_from_name(Some("pointer"));
    super::accessibility::set_label(
        &button,
        &crate::i18n::tr(entry.accessible_label.unwrap_or(label)),
    );
    (button, text, content)
}

fn scrollable_page(content: &gtk::Box, class: Option<&str>) -> gtk::Widget {
    let empty = gtk::Label::new(Some(&crate::i18n::tr(
        "No matching settings are available on this installation.",
    )));
    empty.add_css_class("settings-search-page-empty");
    empty.add_css_class("settings-option-description");
    empty.set_visible(false);
    content.append(&empty);
    constrain_page_text(content.upcast_ref());
    content.set_hexpand(true);
    let page = PageContentBin::new(content);
    // GTK treats an `External` horizontal policy as scrollable, so sideways
    // swipes pan the page and paint the theme's near-black overshoot stripe.
    // `Never` refuses them; the bin keeps the page fitting the viewport, so
    // nothing is clipped.
    let scroller = gtk::ScrolledWindow::builder()
        .child(&page)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();
    scroller.add_css_class("settings-content-scroll");
    if let Some(class) = class {
        scroller.add_css_class(class);
    }
    scroller.upcast()
}

// Prose wraps to the viewport; long editable values scroll inside their entry,
// rather than making the whole settings page wider.
fn constrain_page_text(widget: &gtk::Widget) {
    // Action buttons keep native label sizing; segmented choices may wrap.
    if widget.is::<gtk::Button>() && !widget.is::<gtk::ToggleButton>() {
        return;
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && label.ellipsize() == gtk::pango::EllipsizeMode::None
    {
        label.set_wrap(
            !label.has_css_class("settings-nowrap")
                && !label.has_css_class("menu-heading")
                && !label.has_css_class("settings-control-label"),
        );
        label.set_wrap_mode(if label.has_css_class("settings-word-wrap") {
            gtk::pango::WrapMode::Word
        } else {
            gtk::pango::WrapMode::WordChar
        });
        crate::ui::controls::keep_words_whole(label);
    }
    if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
        entry.set_width_chars(1);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        constrain_page_text(&widget);
    }
}

fn page_content() -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("settings-preferences");
    content
}

/// Translates Settings copy, rendering the shortcuts it names.
fn translated(message: &str) -> String {
    translated_in(&rust_i18n::locale(), message)
}

fn translated_in(locale: &str, message: &str) -> String {
    use super::keys::{
        Keys::One,
        bindings::{REFERENCE, SIDEBAR_FOCUS, TENXER_MODE},
        label_in,
    };
    rust_i18n::t!(
        message,
        locale = locale,
        reference_keys = label_in(locale, &One(REFERENCE)),
        sidebar_focus_keys = label_in(locale, &One(SIDEBAR_FOCUS)),
        tenxer_mode_keys = label_in(locale, &One(TENXER_MODE))
    )
    .into_owned()
}

fn settings_option(title: &str, description: &str, active: bool) -> (gtk::Box, gtk::Switch) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    search::tag(&row, title);
    row.add_css_class("settings-option");
    let copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
    copy.set_hexpand(true);
    copy.set_valign(gtk::Align::Center);
    let title_label = gtk::Label::new(Some(&translated(title)));
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    title_label.add_css_class("settings-option-title");
    let description_label = gtk::Label::new(Some(&translated(description)));
    description_label.set_xalign(0.0);
    description_label.set_wrap(true);
    description_label.add_css_class("settings-option-description");
    description_label.set_visible(!description.is_empty());
    copy.append(&title_label);
    copy.append(&description_label);
    let toggle = gtk::Switch::builder()
        .active(active)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .build();
    toggle.update_property(&[
        gtk::accessible::Property::Label(&translated(title)),
        gtk::accessible::Property::Description(&translated(description)),
    ]);
    row.append(&copy);
    row.append(&toggle);
    (row, toggle)
}

fn search_field(placeholder: &str) -> (gtk::Overlay, gtk::Entry, gtk::Button) {
    let theme_search = gtk::Entry::new();
    theme_search.add_css_class("form-control");
    theme_search.add_css_class("settings-search");
    theme_search.set_placeholder_text(Some(&crate::i18n::tr(placeholder)));
    super::accessibility::set_label(&theme_search, &crate::i18n::tr(placeholder));
    let search_keys = gtk::EventControllerKey::new();
    search_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let selected_search = theme_search.downgrade();
    keys::on_press(&search_keys, move |_, press| {
        if !press.is(SELECT_ALL) {
            return glib::Propagation::Proceed;
        }
        let Some(search) = selected_search.upgrade() else {
            return glib::Propagation::Proceed;
        };
        search.select_region(0, -1);
        glib::Propagation::Stop
    });
    theme_search.add_controller(search_keys);
    let clear_search = gtk::Button::builder()
        .child(&crate::assets::primary_icon(icons::X, 15))
        .tooltip_text(crate::i18n::tr("Clear search"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .margin_end(6)
        .visible(false)
        .build();
    clear_search.add_css_class("theme-search-clear");
    clear_search.set_has_frame(false);
    let search_overlay = gtk::Overlay::new();
    search_overlay.set_child(Some(&theme_search));
    let icon = crate::assets::primary_icon(icons::SEARCH, 16);
    icon.set_halign(gtk::Align::Start);
    icon.set_valign(gtk::Align::Center);
    icon.set_margin_start(12);
    icon.add_css_class("settings-search-icon");
    icon.set_can_target(false);
    search_overlay.add_overlay(&icon);
    search_overlay.add_overlay(&clear_search);
    let search = theme_search.downgrade();
    clear_search.connect_clicked(move |_| {
        if let Some(search) = search.upgrade() {
            search.set_text("");
            search.grab_focus();
        }
    });
    (search_overlay, theme_search, clear_search)
}

fn settings_group(content: &gtk::Box, heading: &str) -> gtk::Box {
    if !heading.is_empty() {
        append_heading(content, heading);
    }
    let group = gtk::Box::new(gtk::Orientation::Vertical, 0);
    group.add_css_class("settings-group");
    group.set_overflow(gtk::Overflow::Hidden);
    content.append(&group);
    group
}

fn control_row(title: &str, description: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let (row, placeholder) = settings_option(title, description, false);
    row.remove(&placeholder);
    row.append(control);
    row
}

fn indent_row(row: &gtk::Box) {
    let arrow = crate::assets::primary_icon(icons::CORNER_DOWN_RIGHT, 18);
    arrow.add_css_class("settings-indent");
    arrow.set_valign(gtk::Align::Center);
    // Keep the dependency marker with its heading when the control stacks below.
    if let Some(copy) = row.first_child().and_downcast::<gtk::Box>()
        && let Some(title) = copy.first_child()
    {
        copy.remove(&title);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        heading.append(&arrow);
        heading.append(&title);
        copy.prepend(&heading);
    }
    row.add_css_class("settings-dependent");
}

fn append_heading(container: &gtk::Box, text: &str) -> gtk::Label {
    let heading = gtk::Label::new(Some(&crate::i18n::tr(text)));
    heading.set_xalign(0.0);
    heading.add_css_class("menu-heading");
    container.append(&heading);
    heading
}
