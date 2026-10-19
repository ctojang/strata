// SPDX-License-Identifier: MIT

use gtk::prelude::*;

mod dismissal;
mod general;
mod restart;

pub(super) fn page_text(root: &gtk::Widget) -> String {
    let mut text = Vec::new();
    if let Ok(label) = root.clone().downcast::<gtk::Label>() {
        text.push(label.text().to_string());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        text.push(page_text(&widget));
    }
    text.join("\n")
}
