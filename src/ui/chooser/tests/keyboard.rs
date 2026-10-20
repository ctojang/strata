// SPDX-License-Identifier: MIT

use super::acceptance::wait_until;
use super::*;
use crate::ui::browser_modes::BrowserMode;
use gtk::gdk::{Key, ModifierType};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Response = Option<PathBuf>;

struct Chooser {
    state: Rc<ChooserState>,
    response: Rc<RefCell<Option<Response>>>,
    responses: Rc<Cell<usize>>,
    keys: gtk::EventControllerKey,
    root: tempfile::TempDir,
}

impl Chooser {
    fn open(mode: BrowserMode, tree: &[&str]) -> Self {
        Self::open_with(mode, tree, true)
    }

    fn open_with(mode: BrowserMode, tree: &[&str], tenxer: bool) -> Self {
        crate::ui::prepare_test_ui();
        let preferences = PreferenceManager::shared();
        preferences.set_browser_mode(mode);
        preferences.set_tenxer_mode(tenxer);
        let root = tempfile::tempdir().expect("fixture");
        for path in tree {
            let target = root.path().join(path.trim_end_matches('/'));
            if path.ends_with('/') {
                std::fs::create_dir_all(&target).expect("fixture folder");
            } else {
                std::fs::write(&target, path.as_bytes()).expect("fixture file");
            }
        }
        let response = Rc::new(RefCell::new(None));
        let responses = Rc::new(Cell::new(0));
        let (received, counted) = (response.clone(), responses.clone());
        let state = build_chooser(destination(root.path().to_path_buf()), move |value| {
            counted.set(counted.get() + 1);
            received.replace(Some(value));
        });
        let browser = state.view.browser();
        wait_until(|| {
            browser
                .column_snapshot(0)
                .is_some_and(|column| !column.loading)
        });
        let controllers = state.window.observe_controllers();
        let keys = (0..controllers.n_items())
            .find_map(|index| {
                controllers
                    .item(index)
                    .and_downcast::<gtk::EventControllerKey>()
            })
            .expect("chooser key controller");
        Self {
            state,
            response,
            responses,
            keys,
            root,
        }
    }

    fn press(&self, key: Key) -> bool {
        self.press_with(key, ModifierType::empty())
    }

    fn press_with(&self, key: Key, modifiers: ModifierType) -> bool {
        self.keys
            .emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
    }

    fn press_reference(&self, key: Key) -> bool {
        let panel = widget_with_class(self.state.window.upcast_ref(), "shortcut-reference-panel")
            .expect("reference panel");
        let controllers = panel.observe_controllers();
        (0..controllers.n_items())
            .find_map(|index| {
                controllers
                    .item(index)
                    .and_downcast::<gtk::EventControllerKey>()
            })
            .expect("reference key controller")
            .emit_by_name::<bool>("key-pressed", &[&key, &0u32, &ModifierType::empty()])
    }

    fn focus_files(&self) {
        self.state.view.browser().focus_active();
        wait_until(|| self.state.view.item_view_has_focus());
    }

    fn cursor_name(&self) -> Option<String> {
        self.state
            .view
            .focused_target()
            .map(|entry| entry.display_name)
    }

    fn move_to(&self, name: &str) {
        let next = if self.state.view.view_mode() == BrowserMode::Icons {
            Key::l
        } else {
            Key::j
        };
        self.press(Key::Home);
        for _ in 0..12 {
            if self.cursor_name().as_deref() == Some(name) {
                return;
            }
            self.press(next);
        }
        panic!("cursor never reached {name}: {:?}", self.cursor_name());
    }

    fn feedback(&self) -> String {
        widget_with_class(self.state.window.upcast_ref(), "shortcut-footer-feedback")
            .and_downcast::<gtk::Label>()
            .map(|label| label.text().to_string())
            .unwrap_or_default()
    }

    fn chord_panel(&self) -> Vec<String> {
        let mut labels = Vec::new();
        let panel = widget_with_class(self.state.window.upcast_ref(), "shortcut-chord-popover");
        let mut pending: Vec<_> = panel
            .filter(|panel| panel.is_visible())
            .into_iter()
            .collect();
        while let Some(widget) = pending.pop() {
            if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                labels.push(label.text().to_string());
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                child = current.next_sibling();
                pending.push(current);
            }
        }
        labels
    }

    fn prompt(&self) -> gtk::Entry {
        widget_with_class(self.state.window.upcast_ref(), "shortcut-footer-prompt")
            .and_downcast::<gtk::Entry>()
            .expect("footer prompt")
    }

    fn footer_visible(&self) -> bool {
        widget_with_class(self.state.window.upcast_ref(), "chooser-footer")
            .is_some_and(|footer| footer.is_visible())
    }

    fn reference_search_focused(&self) -> bool {
        widget_with_class(self.state.window.upcast_ref(), "shortcut-reference-search").is_some_and(
            |search| {
                gtk::prelude::RootExt::focus(&self.state.window)
                    .is_some_and(|focus| focus == search || focus.is_ancestor(&search))
            },
        )
    }

    fn reference_visible(&self) -> bool {
        widget_with_class(self.state.window.upcast_ref(), "shortcut-reference-panel")
            .is_some_and(|panel| panel.is_visible())
    }

    fn prompt_focused(&self) -> bool {
        let prompt = self.prompt();
        gtk::prelude::RootExt::focus(&self.state.window)
            .is_some_and(|focus| focus == prompt || focus.is_ancestor(&prompt))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn chosen(&self) -> PathBuf {
        wait_until(|| self.response.borrow().is_some());
        self.response
            .borrow_mut()
            .take()
            .expect("response")
            .expect("accepted")
    }

    fn cancelled(&self) -> bool {
        matches!(self.response.borrow().as_ref(), Some(None))
    }

    fn open_request(&self) -> bool {
        self.responses.get() == 0 && self.state.window.is_visible()
    }
}

impl Drop for Chooser {
    fn drop(&mut self) {
        self.state.window.close();
    }
}

#[test]
fn folder_keys_navigate_and_ctrl_enter_chooses_the_cursor_folder() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::keyboard::folder_keys_navigate_and_ctrl_enter_chooses_the_cursor_folder",
        || {
            let chooser = Chooser::open(
                BrowserMode::List,
                &["alpha/", "beta/", "beta/inner/", "notes.txt"],
            );
            chooser.focus_files();
            chooser.move_to("beta");
            chooser.press(Key::Return);
            let browser = chooser.state.view.browser();
            let beta = Location::local(chooser.root.path().join("beta"));
            wait_until(|| browser.active_location().as_ref() == Some(&beta));
            assert!(chooser.open_request(), "Enter on a folder chose it");

            chooser.press(Key::BackSpace);
            wait_until(|| {
                browser.active_location() == Some(Location::local(chooser.root.path()))
                    && browser
                        .column_snapshot(0)
                        .is_some_and(|column| !column.loading)
            });
            chooser.focus_files();
            chooser.move_to("alpha");
            assert!(chooser.press_with(Key::Return, ModifierType::CONTROL_MASK));
            assert_eq!(chooser.chosen(), chooser.path("alpha"));
        },
    );
}

#[test]
fn chooser_refuses_commands_its_request_does_not_allow() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::keyboard::chooser_refuses_commands_its_request_does_not_allow",
        || {
            let chooser = Chooser::open(BrowserMode::List, &["a/", "b/", "sub/"]);
            chooser.focus_files();
            chooser.move_to("a");
            let browser = chooser.state.view.browser();
            let origin = browser.active_location();
            for (key, modifiers, feedback) in [
                (
                    Key::y,
                    ModifierType::empty(),
                    "Not available in the file chooser",
                ),
                (
                    Key::p,
                    ModifierType::empty(),
                    "Not available in the file chooser",
                ),
                (
                    Key::O,
                    ModifierType::SHIFT_MASK,
                    "Not available in the file chooser",
                ),
                (
                    Key::M,
                    ModifierType::SHIFT_MASK,
                    "Not available in the file chooser",
                ),
                (
                    Key::C,
                    ModifierType::SHIFT_MASK,
                    "Not available in the file chooser",
                ),
                (
                    Key::R,
                    ModifierType::SHIFT_MASK,
                    "Not available in the file chooser",
                ),
                (
                    Key::semicolon,
                    ModifierType::empty(),
                    "Not available in the file chooser",
                ),
                (
                    Key::i,
                    ModifierType::empty(),
                    "Not available in the file chooser",
                ),
                (
                    Key::Q,
                    ModifierType::SHIFT_MASK,
                    "Not available in the file chooser",
                ),
                (
                    Key::v,
                    ModifierType::CONTROL_MASK,
                    "Not available in the file chooser",
                ),
                (
                    Key::space,
                    ModifierType::empty(),
                    "Only one item can be chosen",
                ),
                (Key::v, ModifierType::empty(), "Only one item can be chosen"),
                (
                    Key::a,
                    ModifierType::CONTROL_MASK,
                    "Only one item can be chosen",
                ),
            ] {
                assert!(chooser.press_with(key, modifiers), "{key:?}");
                assert_eq!(chooser.feedback(), feedback, "{key:?}");
                assert!(chooser.open_request(), "{key:?} ended the request");
                assert!(browser.selected_entries().is_empty(), "{key:?} filled");
            }
            assert_eq!(chooser.cursor_name().as_deref(), Some("a"));

            chooser.press(Key::g);
            wait_until(|| chooser.chord_panel().iter().any(|label| label == "Recent"));
            let panel = chooser.chord_panel();
            assert!(panel.iter().any(|label| label == "Pins"), "{panel:?}");
            for refused in ["Trash", "Network", "Pin / unpin folder"] {
                assert!(
                    !panel.iter().any(|label| label == refused),
                    "the g panel offers {refused}: {panel:?}"
                );
            }
            chooser.press(Key::t);
            assert_eq!(chooser.feedback(), "Only local folders can be opened here");
            chooser.press(Key::g);
            chooser.press(Key::n);
            assert_eq!(browser.active_location(), origin);
            for (key, modifiers) in [
                (Key::plus, ModifierType::SHIFT_MASK),
                (Key::minus, ModifierType::empty()),
            ] {
                chooser.press(Key::g);
                assert!(chooser.press_with(key, modifiers), "g {key:?}");
                assert_eq!(
                    chooser.feedback(),
                    "Not available in the file chooser",
                    "g {key:?}"
                );
            }
            assert!(chooser.open_request());
        },
    );
}

#[test]
fn escape_dismisses_one_interaction_before_cancelling_and_the_toggle_keeps_the_request() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::keyboard::escape_dismisses_one_interaction_before_cancelling_and_the_toggle_keeps_the_request",
        || {
            let chooser = Chooser::open(BrowserMode::List, &["a/", "b/"]);
            chooser.focus_files();
            chooser.move_to("b");
            assert!(chooser.press(Key::slash));
            wait_until(|| chooser.prompt_focused());
            assert!(chooser.press(Key::Escape));
            assert!(chooser.open_request(), "Esc in the prompt cancelled");
            assert!(chooser.state.view.item_view_has_focus());

            assert!(chooser.press(Key::F1));
            wait_until(|| chooser.reference_search_focused());
            assert!(chooser.press(Key::Escape));
            assert!(chooser.open_request(), "Esc in the F1 reference cancelled");
            assert!(!chooser.reference_visible());

            assert!(chooser.press(Key::Escape));
            assert!(chooser.cancelled());
            drop(chooser);

            let chooser = Chooser::open(BrowserMode::List, &["a/"]);
            chooser.focus_files();
            assert!(chooser.press(Key::q));
            assert!(PreferenceManager::shared().tenxer_mode(), "q left the mode");
            assert!(chooser.press_with(
                Key::m,
                ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK
            ));
            assert!(!PreferenceManager::shared().tenxer_mode());
            assert!(
                chooser.open_request(),
                "leaving the mode cancelled the request"
            );
            wait_until(|| !chooser.footer_visible());
            chooser.press(Key::Escape);
            assert!(chooser.cancelled());
            assert_eq!(chooser.responses.get(), 1);
        },
    );
}

#[test]
fn chooser_shortcuts_stay_off_while_the_reference_stays_open_outside_tenxer() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::keyboard::chooser_shortcuts_stay_off_while_the_reference_stays_open_outside_tenxer",
        || {
            let chooser = Chooser::open(BrowserMode::List, &["a/", "folder/"]);
            chooser.focus_files();
            assert!(chooser.press(Key::F1));
            wait_until(|| chooser.reference_search_focused());
            PreferenceManager::shared().set_tenxer_mode(false);
            wait_until(|| !chooser.footer_visible());
            assert!(chooser.reference_visible());
            let browser = chooser.state.view.browser();
            let location = browser.active_location();
            let (none, control, alt) = (
                ModifierType::empty(),
                ModifierType::CONTROL_MASK,
                ModifierType::ALT_MASK,
            );
            for (key, modifiers) in [
                (Key::F5, none),
                (Key::h, control),
                (Key::period, control),
                (Key::_2, control),
                (Key::Up, alt),
                (Key::Left, alt),
                (Key::Home, alt),
            ] {
                assert!(
                    !chooser.press_with(key, modifiers),
                    "{modifiers:?}+{key:?} acted behind the reference"
                );
            }
            assert_eq!(chooser.state.view.view_mode(), BrowserMode::List);
            assert_eq!(browser.active_location(), location);
            assert!(!chooser.press(Key::Escape), "the chooser took Esc");
            assert!(chooser.press_reference(Key::Escape));
            assert!(!chooser.reference_visible());
            assert!(chooser.open_request(), "Esc cancelled the request");
        },
    );
}

#[test]
fn live_mode_changes_switch_the_chooser_between_maps() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::keyboard::live_mode_changes_switch_the_chooser_between_maps",
        || {
            let chooser = Chooser::open_with(BrowserMode::List, &["a/"], false);
            chooser.focus_files();
            assert!(!chooser.footer_visible());
            chooser.press(Key::slash);
            assert!(
                !chooser.prompt().is_mapped(),
                "the default map opened a prompt"
            );

            assert!(chooser.press_with(
                Key::M,
                ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK
            ));
            assert!(PreferenceManager::shared().tenxer_mode());
            wait_until(|| chooser.footer_visible());
            chooser.focus_files();
            assert!(chooser.press(Key::slash));
            wait_until(|| chooser.prompt_focused());

            PreferenceManager::shared().set_tenxer_mode(false);
            wait_until(|| !chooser.footer_visible());
            assert!(chooser.prompt().text().is_empty());
            assert!(chooser.open_request());
        },
    );
}

#[test]
fn folder_only_search_hits_exclude_files_and_choose_the_focused_folder() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::keyboard::folder_only_search_hits_exclude_files_and_choose_the_focused_folder",
        || {
            let chooser = Chooser::open(
                BrowserMode::List,
                &[
                    "docs/",
                    "docs/nested/",
                    "docs/nested.txt",
                    "nested-note.txt",
                ],
            );
            chooser.focus_files();
            assert!(chooser.press(Key::s));
            wait_until(|| chooser.prompt_focused());
            chooser.prompt().set_text("nest");
            assert!(chooser.press(Key::Return));
            wait_until(|| {
                chooser
                    .state
                    .view
                    .focused_target()
                    .is_some_and(|entry| entry.display_name == "nested")
            });
            let hits = chooser.state.view.selected_search_results();
            assert!(
                hits.iter().flatten().all(|entry| entry.is_directory()),
                "{hits:?}"
            );
            assert!(chooser.press(Key::y));
            assert!(chooser.open_request());
            assert!(chooser.press_with(Key::Return, ModifierType::CONTROL_MASK));
            assert_eq!(chooser.chosen(), chooser.path("docs/nested"));
        },
    );
}

#[test]
fn footer_create_and_rename_keep_the_request_open() {
    crate::test_support::gtk_test(
        "ui::chooser::tests::keyboard::footer_create_and_rename_keep_the_request_open",
        || {
            let chooser = Chooser::open(BrowserMode::List, &["draft/"]);
            chooser.focus_files();
            chooser.move_to("draft");

            assert!(chooser.press(Key::r));
            wait_until(|| chooser.prompt_focused());
            assert_eq!(chooser.prompt().text(), "draft");
            assert!(chooser.press(Key::Escape));
            assert!(chooser.state.view.item_view_has_focus());
            assert!(chooser.root.path().join("draft").exists());

            assert!(chooser.press(Key::r));
            wait_until(|| chooser.prompt_focused());
            chooser.prompt().set_text("final");
            assert!(chooser.press(Key::Return));
            wait_until(|| chooser.root.path().join("final").is_dir());

            chooser.focus_files();
            assert!(chooser.press(Key::a));
            wait_until(|| chooser.prompt_focused());
            chooser.prompt().set_text("made/");
            assert!(chooser.press(Key::Return));
            wait_until(|| chooser.root.path().join("made").is_dir());
            assert!(chooser.open_request());
            assert!(!chooser.root.path().join("draft").exists());
        },
    );
}
