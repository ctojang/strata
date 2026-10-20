// SPDX-License-Identifier: MIT

use super::*;

mod acceptance;
mod column_widths;
mod filtered_preview;
mod keyboard;
mod sizing;

pub(super) fn destination(initial_directory: PathBuf) -> DestinationRequest {
    let parent = gtk::Window::new();
    parent.present();
    DestinationRequest {
        parent,
        title: "Choose a folder".into(),
        accept_label: "Choose".into(),
        initial_directory,
        root_limit: None,
        allow_create: true,
        validate: Rc::new(|path| Ok(path.to_path_buf())),
    }
}

pub(super) fn accept_button(state: &ChooserState) -> gtk::Button {
    widget_with_class(state.window.upcast_ref(), "action-dialog-confirm")
        .and_downcast()
        .expect("accept button")
}

pub(super) fn widget_with_class(widget: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
    if widget.has_css_class(class) {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = widget_with_class(&current, class) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}
