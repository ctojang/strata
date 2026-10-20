// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use gtk::{glib, prelude::*};

use crate::{
    model::{FileEntry, MetadataValue},
    ui::preferences::PreferenceManager,
    util::{DateDisplay, modified_date_for_seconds},
};

#[cfg(test)]
mod tests;

struct ModifiedDateBinding {
    label: glib::WeakRef<gtk::Label>,
    seconds: i64,
    display: DateDisplay,
}

thread_local! {
    static MODIFIED_DATE_BINDINGS: RefCell<Vec<ModifiedDateBinding>> = const { RefCell::new(Vec::new()) };
    static DATE_FORMAT_SUBSCRIBERS: RefCell<Vec<glib::WeakRef<gtk::Label>>> = const { RefCell::new(Vec::new()) };
    static MODIFIED_DATE_TIMER_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn set_full_modified_date(label: &gtk::Label, seconds: Option<i64>) {
    bind_modified_date(label, seconds, "—", DateDisplay::Full);
}

pub fn set_modified_date(label: &gtk::Label, entry: Option<&FileEntry>, fallback: &str) {
    let seconds = entry.and_then(|entry| match entry.modified_unix_seconds {
        MetadataValue::Known(seconds) => Some(seconds),
        MetadataValue::Unknown | MetadataValue::Unavailable => None,
    });
    bind_modified_date(label, seconds, fallback, DateDisplay::Preferred);
}

fn bind_modified_date(
    label: &gtk::Label,
    seconds: Option<i64>,
    fallback: &str,
    display: DateDisplay,
) {
    let text = seconds
        .map(|seconds| modified_date_for_seconds(seconds, display))
        .unwrap_or_else(|| fallback.to_owned());
    label.set_text(&text);

    MODIFIED_DATE_BINDINGS.with_borrow_mut(|bindings| {
        bindings.retain(|binding| {
            binding
                .label
                .upgrade()
                .is_some_and(|bound_label| bound_label != *label)
        });
        if let Some(seconds) = seconds {
            bindings.push(ModifiedDateBinding {
                label: label.downgrade(),
                seconds,
                display,
            });
        }
    });

    if seconds.is_some() {
        let unbound = DATE_FORMAT_SUBSCRIBERS.with_borrow_mut(|bound| {
            bound.retain(|weak| weak.upgrade().is_some());
            if bound
                .iter()
                .any(|weak| weak.upgrade().is_some_and(|bound| bound == *label))
            {
                false
            } else {
                bound.push(label.downgrade());
                true
            }
        });
        if unbound {
            bind_date_format(label);
        }
        ensure_modified_date_timer();
    }
}

fn bind_date_format(label: &gtk::Label) {
    PreferenceManager::shared().bind_preference(
        label,
        PreferenceManager::date_format,
        |widget, _| {
            let Some(label) = widget.downcast_ref::<gtk::Label>() else {
                return;
            };
            if let Some((seconds, display)) = current_binding(label) {
                label.set_text(&modified_date_for_seconds(seconds, display));
            }
        },
    );
}

fn current_binding(label: &gtk::Label) -> Option<(i64, DateDisplay)> {
    MODIFIED_DATE_BINDINGS.with_borrow(|bindings| {
        bindings.iter().find_map(|binding| {
            binding
                .label
                .upgrade()
                .is_some_and(|bound| bound == *label)
                .then_some((binding.seconds, binding.display))
        })
    })
}

fn ensure_modified_date_timer() {
    let already_active = MODIFIED_DATE_TIMER_ACTIVE.with(|active| active.replace(true));
    if already_active {
        return;
    }

    glib::timeout_add_local(Duration::from_secs(30), || {
        let live_bindings = MODIFIED_DATE_BINDINGS.with_borrow_mut(|bindings| {
            bindings.retain(|binding| binding.label.upgrade().is_some());
            bindings
                .iter()
                .filter_map(|binding| {
                    binding
                        .label
                        .upgrade()
                        .map(|label| (label, binding.seconds, binding.display))
                })
                .collect::<Vec<_>>()
        });
        for (label, seconds, display) in &live_bindings {
            label.set_text(&modified_date_for_seconds(*seconds, *display));
        }

        if live_bindings.is_empty() {
            MODIFIED_DATE_TIMER_ACTIVE.with(|active| active.set(false));
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}
