// SPDX-License-Identifier: MIT

use gtk::{glib, prelude::*};

use super::set_modified_date;
use crate::{
    model::{EntryKind, FileEntry, Location, MetadataValue},
    test_support::gtk_test,
    ui::preferences::{PreferenceManager, fixtures::seed_saved_preferences_for_test},
    util::DateFormat,
};

#[test]
fn saved_date_format_renders_before_settings_and_updates_bound_labels() {
    gtk_test(
        "ui::modified_date::tests::saved_date_format_renders_before_settings_and_updates_bound_labels",
        || {
            seed_saved_preferences_for_test();
            let manager = PreferenceManager::shared();
            let seconds = glib::DateTime::now_local().expect("local time").to_unix() - 120;
            let entry = FileEntry {
                location: Location::local("/fixture/recent.txt"),
                native_name: "recent.txt".into(),
                display_name: "recent.txt".into(),
                thumbnail_path: None,
                kind: EntryKind::File,
                size: MetadataValue::Known(4),
                modified_unix_seconds: MetadataValue::Known(seconds),
                mode: MetadataValue::Known(0o100644),
                recent_unix_seconds: MetadataValue::Unknown,
                is_hidden: false,
                image_dimensions: MetadataValue::Unknown,
                child_count: MetadataValue::Unknown,
                duration_seconds: MetadataValue::Unknown,
                recent_uri: None,
            };
            let absolute = |pattern: &str| {
                glib::DateTime::from_unix_local(seconds)
                    .expect("modified date")
                    .format(pattern)
                    .expect("format")
                    .to_string()
            };
            let windows = [gtk::Window::new(), gtk::Window::new()];
            let labels: Vec<gtk::Label> = windows
                .iter()
                .map(|window| {
                    let label = gtk::Label::new(None);
                    window.set_child(Some(&label));
                    set_modified_date(&label, Some(&entry), "—");
                    label
                })
                .collect();
            for label in &labels {
                assert_eq!(label.label(), absolute("%Y-%m-%d %H:%M"));
            }
            manager.set_date_format(DateFormat::Long);
            for label in &labels {
                assert_eq!(label.label(), absolute("%B %-d, %Y, %H:%M"));
            }
            set_modified_date(&labels[0], None, "unknown");
            manager.set_date_format(DateFormat::Relative);
            assert_eq!(labels[0].label(), "unknown");
            let text = labels[1].label();
            assert!(text == "Just now" || text.ends_with(" ago"), "{text}");
            let mut rebound = entry.clone();
            rebound.modified_unix_seconds = MetadataValue::Known(seconds - 86400);
            set_modified_date(&labels[0], Some(&rebound), "—");
            manager.set_date_format(DateFormat::Iso8601);
            let expected = glib::DateTime::from_unix_local(seconds - 86400)
                .expect("rebound date")
                .format("%Y-%m-%d %H:%M")
                .expect("format");
            assert_eq!(labels[0].label(), expected);
            assert_eq!(labels[1].label(), absolute("%Y-%m-%d %H:%M"));
            let rebuilt = gtk::Label::new(None);
            windows[1].set_child(Some(&rebuilt));
            set_modified_date(&rebuilt, Some(&entry), "—");
            assert_eq!(rebuilt.label(), absolute("%Y-%m-%d %H:%M"));
            manager.set_date_format(DateFormat::Long);
            assert_eq!(rebuilt.label(), absolute("%B %-d, %Y, %H:%M"));
            for window in windows {
                window.close();
            }
        },
    );
}
