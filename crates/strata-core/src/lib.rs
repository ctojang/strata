// SPDX-License-Identifier: MIT

// build.rs embeds data/locales; this path holds no catalogs, so the macro adds none itself.
rust_i18n::i18n!(
    "src/i18n/no-catalogs",
    fallback = "en",
    backend = crate::i18n::Catalogs::compiled()
);

pub mod adapters;
pub mod app;
pub mod build_info;
pub mod i18n;
pub mod logging;
pub mod media;
pub mod metrics;
pub mod model;
#[cfg(feature = "rar")]
pub mod rar_extraction;
pub mod sandbox;
pub mod sandbox_helper;
pub mod services;
pub mod storage;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod trusted_command;
pub mod util;

pub fn register_bundled_resources() -> Result<(), glib::Error> {
    gio::resources_register_include!("strata.gresource")
}
