// SPDX-License-Identifier: MIT

mod accessibility;
mod actions;
mod blur;
mod browser;
mod browser_modes;
mod chooser;
mod close_guard;
mod collection_edit;
mod collection_interaction;
mod controls;
mod decoration;
mod document_media;
mod document_view;
mod entry_list_model;
mod file_providers;
mod focus_navigation;
mod folder_picker;
mod frame;
mod icons_cell;
mod inline_search;
mod input_ownership;
mod jobs;
mod keys;
mod loading_skeleton;
mod marquee;
mod media;
mod missing_tools;
mod modal;
mod motion;
mod notice;
mod open_with;
mod pointer;
pub(crate) mod preferences;
mod preview;
mod progress_dock;
mod raw_details;
mod recent_apps;
mod resize_feedback;
mod scrolling;
mod search;
mod search_session;
mod settings;
mod shortcut_footer;
mod shortcut_reference;
mod sidebar_toggle;
mod table_view;
mod tabs_session;
mod tenxer_mode;
mod terminal;
mod theme;
mod thumbnail;
pub(crate) mod thumbnail_cache;
mod top_bar_navigation;
mod virtual_preview;
mod window;

pub(in crate::ui) use window::{
    RemovableDestination, removable_destinations, resolve_removable_destination,
};
pub use window::{UnlockTarget, present, present_open, present_unlock};

#[cfg(test)]
pub(crate) fn prepare_test_ui() {
    let _theme = theme::ThemeManager::shared();
}
