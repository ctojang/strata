// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::browser_modes::BrowserMode;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

#[track_caller]
pub(super) fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "chooser did not settle");
        glib::MainContext::default().iteration(false);
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn visible_collection_selection(widget: &gtk::Widget) -> Option<gtk::SelectionModel> {
    let selection = widget
        .clone()
        .downcast::<gtk::ListView>()
        .ok()
        .and_then(|view| view.is_mapped().then(|| view.model()).flatten())
        .or_else(|| {
            widget
                .clone()
                .downcast::<gtk::GridView>()
                .ok()
                .and_then(|view| view.is_mapped().then(|| view.model()).flatten())
        });
    if selection.is_some() {
        return selection;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(selection) = visible_collection_selection(&current) {
            return Some(selection);
        }
        child = current.next_sibling();
    }
    None
}

#[test]
fn directory_confirmation_distinguishes_load_cursor_from_explicit_selection() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::acceptance::directory_confirmation_distinguishes_load_cursor_from_explicit_selection",
        || {
            crate::ui::prepare_test_ui();
            for mode in [BrowserMode::List, BrowserMode::Icons, BrowserMode::Columns] {
                PreferenceManager::shared().set_browser_mode(mode);
                for explicit in [false, true] {
                    let root = tempfile::tempdir().expect("fixture");
                    let child = root.path().join("child");
                    std::fs::create_dir(&child).expect("child folder");
                    std::fs::write(root.path().join("hidden-by-folder-policy.txt"), "file")
                        .expect("file");
                    let result = Rc::new(RefCell::new(None));
                    let received = result.clone();
                    let state =
                        build_chooser(destination(root.path().to_path_buf()), move |value| {
                            received.replace(Some(value));
                        });
                    let browser = state.view.browser();
                    wait_until(|| {
                        browser
                            .column_snapshot(0)
                            .is_some_and(|column| !column.loading && column.count == 1)
                    });
                    assert!(browser.selection_is_load_cursor(), "{mode:?}");
                    if explicit {
                        browser.select(0, 0);
                        assert!(!browser.selection_is_load_cursor(), "{mode:?}");
                    }
                    accept_button(&state).emit_clicked();
                    wait_until(|| result.borrow().is_some());
                    let selected = result
                        .borrow_mut()
                        .take()
                        .expect("result")
                        .expect("accepted");
                    let expected = if explicit {
                        child.as_path()
                    } else {
                        root.path()
                    };
                    assert_eq!(selected, expected, "{mode:?}, explicit={explicit}");
                    state.window.close();
                }
            }
        },
    );
}

#[test]
fn folder_only_requests_filter_recursive_results() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::acceptance::folder_only_requests_filter_recursive_results",
        || {
            crate::ui::prepare_test_ui();
            PreferenceManager::shared().set_filter_include_subfolders(true);
            for mode in [BrowserMode::List, BrowserMode::Icons, BrowserMode::Columns] {
                PreferenceManager::shared().set_browser_mode(mode);
                let root = tempfile::tempdir().expect("fixture");
                for name in ["needle-directory-a", "needle-directory-b"] {
                    std::fs::create_dir(root.path().join(name)).expect("folder");
                }
                for index in 0..120 {
                    std::fs::write(root.path().join(format!("needle-{index:03}.txt")), "file")
                        .expect("file");
                }
                let result = Rc::new(RefCell::new(None));
                let received = result.clone();
                let state = build_chooser(destination(root.path().to_path_buf()), move |value| {
                    received.replace(Some(value));
                });
                let browser = state.view.browser();
                wait_until(|| {
                    browser
                        .column_snapshot(0)
                        .is_some_and(|column| !column.loading && column.count == 2)
                });
                assert!((0..2).all(|position| {
                    browser
                        .entry_at(0, position)
                        .is_some_and(|entry| entry.is_directory())
                }));
                assert!(state.view.show_filter_with_query("needle"));
                wait_until(|| {
                    state.view.selected_search_results().is_some()
                        && visible_collection_selection(&state.view.widget())
                            .is_some_and(|results| results.n_items() == 2)
                });
                let results = visible_collection_selection(&state.view.widget()).expect("results");
                results.select_item(0, true);
                let entries = state
                    .view
                    .selected_search_results()
                    .expect("search selection");
                let [entry] = entries.as_slice() else {
                    panic!("{mode:?}: one selected result, got {entries:?}");
                };
                assert!(entry.is_directory());
                let expected = entry.location.native_path().expect("local").to_path_buf();
                accept_button(&state).emit_clicked();
                wait_until(|| result.borrow().is_some());
                let selected = result
                    .borrow_mut()
                    .take()
                    .expect("result")
                    .expect("accepted");
                assert_eq!(selected, expected, "{mode:?}");
            }
        },
    );
}

fn key_controller(window: &gtk::Window) -> gtk::EventControllerKey {
    let controllers = window.observe_controllers();
    for index in 0..controllers.n_items() {
        if let Some(controller) = controllers
            .item(index)
            .and_downcast::<gtk::EventControllerKey>()
        {
            return controller;
        }
    }
    panic!("no EventControllerKey on the chooser window");
}

fn press(window: &gtk::Window, key: gtk::gdk::Key) -> bool {
    key_controller(window).emit_by_name::<bool>(
        "key-pressed",
        &[&key, &0u32, &gtk::gdk::ModifierType::empty()],
    )
}

#[test]
fn arrow_scope_keeps_left_in_the_chooser_file_view() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::acceptance::arrow_scope_keeps_left_in_the_chooser_file_view",
        || {
            crate::ui::prepare_test_ui();
            PreferenceManager::shared().set_arrow_navigation_scoped(true);
            let root = tempfile::tempdir().expect("fixture");
            for name in ["a", "b", "c"] {
                std::fs::create_dir(root.path().join(name)).expect("fixture folder");
            }
            for mode in [BrowserMode::List, BrowserMode::Icons, BrowserMode::Columns] {
                PreferenceManager::shared().set_browser_mode(mode);
                let state = build_chooser(destination(root.path().to_path_buf()), |_| {});
                state.view.set_view_mode(mode);
                let browser = state.view.browser();
                wait_until(|| {
                    browser
                        .column_snapshot(0)
                        .is_some_and(|column| !column.loading && column.count == 3)
                });
                for scoped in [true, false, true] {
                    PreferenceManager::shared().set_arrow_navigation_scoped(scoped);
                    for key in [gtk::gdk::Key::Left, gtk::gdk::Key::Up] {
                        browser.select(0, 0);
                        browser.focus_active();
                        wait_until(|| {
                            state.view.item_view_has_focus() && state.view.item_at_sidebar_edge()
                        });
                        press(&state.window, key);
                        assert_eq!(
                            state.view.item_view_has_focus(),
                            scoped,
                            "{mode:?}: {key:?} with arrow scope {scoped}"
                        );
                    }
                }
                state.window.close();
            }
        },
    );
}

#[test]
fn item_menu_offers_compress_for_native_folder() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::acceptance::item_menu_offers_compress_for_native_folder",
        || {
            use gtk::prelude::*;
            crate::ui::prepare_test_ui();
            let root = tempfile::tempdir().expect("fixture");
            std::fs::create_dir(root.path().join("folder")).expect("folder");
            std::fs::write(root.path().join("notes.txt"), "notes").expect("file");
            let state = build_chooser(destination(root.path().to_path_buf()), |_| {});
            let browser = state.view.browser();
            wait_until(|| {
                browser
                    .column_snapshot(0)
                    .is_some_and(|column| !column.loading && column.count == 1)
            });
            browser.select(0, 0);
            browser.focus_active();
            let visible_popovers = || {
                descendants(state.window.upcast_ref())
                    .into_iter()
                    .filter_map(|widget| widget.downcast::<gtk::Popover>().ok())
                    .filter(|popover| popover.is_visible())
                    .collect::<Vec<_>>()
            };
            let before = visible_popovers()
                .iter()
                .map(|popover| popover.as_ptr())
                .collect::<Vec<_>>();
            wait_until(|| {
                if visible_popovers()
                    .iter()
                    .any(|candidate| !before.contains(&candidate.as_ptr()))
                {
                    return true;
                }
                state.view.open_focused_context_menu()
                    && visible_popovers()
                        .iter()
                        .any(|candidate| !before.contains(&candidate.as_ptr()))
            });
            let popover = visible_popovers()
                .into_iter()
                .find(|candidate| !before.contains(&candidate.as_ptr()))
                .expect("chooser item menu");
            wait_until(|| popover.is_mapped());
            let labels = descendants(popover.upcast_ref())
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Label>().ok())
                .filter(|label| label.is_visible() && !label.text().is_empty())
                .map(|label| label.text().to_string())
                .collect::<Vec<_>>();
            assert!(
                labels.iter().any(|label| label == "Compress…"),
                "{labels:?}"
            );
            popover.popdown();
            state.window.close();
        },
    );
}

fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut result = vec![widget.clone()];
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        result.extend(descendants(&current));
    }
    result
}
