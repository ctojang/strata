// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::browser_modes::BrowserMode;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

#[track_caller]
fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "filtered chooser did not settle");
        glib::MainContext::default().iteration(false);
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn keys(widget: &impl IsA<gtk::Widget>) -> Vec<gtk::EventControllerKey> {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
        .collect()
}

fn press(
    keys: &[gtk::EventControllerKey],
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> bool {
    keys.iter()
        .any(|keys| keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers]))
}

#[test]
fn typing_a_character_opens_the_filter_with_that_query() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::filtered_preview::typing_a_character_opens_the_filter_with_that_query",
        || {
            crate::ui::prepare_test_ui();
            let root = tempfile::tempdir().expect("fixture");
            std::fs::create_dir(root.path().join("notes")).expect("folder");
            for mode in [BrowserMode::Columns, BrowserMode::Icons, BrowserMode::List] {
                PreferenceManager::shared().set_browser_mode(mode);
                PreferenceManager::shared().set_type_to_search(true);
                let state = build_chooser(destination(root.path().to_path_buf()), |_| {});
                // Finish deferred startup focus before simulating user input.
                let initialized = Rc::new(Cell::new(false));
                let notify = initialized.clone();
                glib::idle_add_local_once(move || notify.set(true));
                wait_until(|| initialized.get());
                let browser = state.view.browser();
                wait_until(|| {
                    browser
                        .column_snapshot(0)
                        .is_some_and(|column| !column.loading)
                });
                browser.select(0, 0);
                browser.focus_active();
                wait_until(|| state.view.item_view_has_focus());
                let window_keys = keys(&state.window);
                assert!(
                    press(
                        &window_keys,
                        gtk::gdk::Key::n,
                        gtk::gdk::ModifierType::empty()
                    ),
                    "{mode:?}: typing 'n' opens the filter"
                );
                wait_until(|| state.view.filter_has_focus());
                let entry = gtk::prelude::RootExt::focus(&state.window)
                    .and_then(|focused| focused.ancestor(gtk::Entry::static_type()))
                    .and_downcast::<gtk::Entry>()
                    .expect("focused filter entry");
                assert_eq!(entry.text(), "n", "{mode:?}: the key seeds the query");
                if let Some(output) = std::env::var_os("STRATA_FILTER_PREVIEW_SCREENSHOTS") {
                    super::sizing::capture(
                        &state.window,
                        Path::new(&output),
                        &format!("type-to-search-{mode:?}"),
                    );
                }
                assert!(press(
                    &window_keys,
                    gtk::gdk::Key::Escape,
                    gtk::gdk::ModifierType::empty()
                ));
                wait_until(|| state.view.item_view_has_focus());
                assert!(
                    press(
                        &window_keys,
                        gtk::gdk::Key::slash,
                        gtk::gdk::ModifierType::empty()
                    ),
                    "{mode:?}: '/' opens the filter"
                );
                wait_until(|| state.view.filter_has_focus());
                assert!(press(
                    &window_keys,
                    gtk::gdk::Key::Escape,
                    gtk::gdk::ModifierType::empty()
                ));
                wait_until(|| state.view.item_view_has_focus());
                PreferenceManager::shared().set_type_to_search(false);
                assert!(
                    !press(
                        &window_keys,
                        gtk::gdk::Key::n,
                        gtk::gdk::ModifierType::empty()
                    ) && !state.view.filter_has_focus(),
                    "{mode:?}: the preference disables typing-to-search"
                );
            }
        },
    );
}

#[test]
fn escape_on_a_focused_result_dismisses_the_filter_without_cancelling() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::filtered_preview::escape_on_a_focused_result_dismisses_the_filter_without_cancelling",
        || {
            crate::ui::prepare_test_ui();
            let root = tempfile::tempdir().expect("fixture");
            std::fs::create_dir(root.path().join("notes")).expect("folder");
            std::fs::create_dir(root.path().join("other")).expect("folder");
            for mode in [BrowserMode::Columns, BrowserMode::Icons, BrowserMode::List] {
                PreferenceManager::shared().set_browser_mode(mode);
                let state = build_chooser(destination(root.path().to_path_buf()), |_| {});
                // Finish deferred startup focus before simulating user input.
                let initialized = Rc::new(Cell::new(false));
                let notify = initialized.clone();
                glib::idle_add_local_once(move || notify.set(true));
                wait_until(|| initialized.get());
                let browser = state.view.browser();
                wait_until(|| {
                    browser
                        .column_snapshot(0)
                        .is_some_and(|column| !column.loading)
                });
                assert!(state.view.show_filter_with_query("notes"));
                let field = gtk::prelude::RootExt::focus(&state.window)
                    .and_then(|focused| focused.ancestor(gtk::Entry::static_type()))
                    .and_downcast::<gtk::Entry>()
                    .expect("focused filter entry");
                wait_until(|| state.view.filter_result_names() == ["notes"]);
                assert!(press(
                    &keys(&field),
                    gtk::gdk::Key::Down,
                    gtk::gdk::ModifierType::empty()
                ));
                wait_until(|| {
                    state.view.filter_focus() == Some(crate::ui::browser::FilterFocus::Results)
                });

                assert!(press(
                    &keys(&state.window),
                    gtk::gdk::Key::Escape,
                    gtk::gdk::ModifierType::empty()
                ));
                assert!(
                    state.completion.borrow().is_some(),
                    "{mode:?}: Escape on a result cancelled the request"
                );
                wait_until(|| {
                    state.view.listing_filter().is_none()
                        && state.view.filter_focus().is_none()
                        && state.view.item_view_has_focus()
                });
                state.cancel();
            }
        },
    );
}
