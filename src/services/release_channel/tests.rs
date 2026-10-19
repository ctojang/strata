// SPDX-License-Identifier: MIT

use super::Version;

fn parse(tag: &str) -> Version {
    Version::parse(tag).unwrap_or_else(|| panic!("expected {tag} to parse"))
}

#[test]
fn accepts_canonical_forms() {
    assert!(Version::parse("0.0.0").is_some());
    assert!(Version::parse("v0.5.0").is_some());
    assert!(Version::parse("0.5.0").is_some());
    assert!(Version::parse("v0.5.0-alpha.1").is_some());
    assert!(Version::parse("v0.5.0-beta.1").is_some());
    assert!(Version::parse("v0.5.0-rc.1").is_some());
    assert!(Version::parse("v0.5.0-nightly.20260901").is_some());
    assert!(Version::parse("v0.5.0-nightly.20260901.2").is_some());
}

/// Missing or malformed segments must be rejected outright. The old
/// `parse_version` silently zero-filled them, which was the bug behind
/// issue #61: a truncated tag could compare as a real version.
#[test]
fn rejects_malformed_tags() {
    assert!(Version::parse("0.5").is_none());
    assert!(Version::parse("0.5.x").is_none());
    assert!(Version::parse("v0.5.0-preview.1").is_none());
    assert!(Version::parse("v0.5.0-alpha.0").is_none());
    assert!(Version::parse("v0.5.0-beta.0").is_none());
    assert!(Version::parse("v0.5.0-rc.0").is_none());
    assert!(Version::parse("v0.5.0-rc").is_none());
    assert!(Version::parse("v0.5.0-rc.x").is_none());
    assert!(Version::parse("").is_none());
    assert!(Version::parse("vv0.5.0").is_none());
    assert!(Version::parse("0.5.0-rc.1-extra").is_none());
}

#[test]
fn display_renders_canonical_tags() {
    for (input, expected) in [
        ("v0.5.0-alpha.1", "0.5.0-alpha.1"),
        ("v0.5.0-beta.2", "0.5.0-beta.2"),
        ("v0.5.0-rc.3", "0.5.0-rc.3"),
        ("v0.5.0", "0.5.0"),
        ("v0.5.0-nightly.20260901.2", "0.5.0-nightly.20260901.2"),
        ("v0.5.0-nightly.20260901", "0.5.0-nightly.20260901"),
        (
            "v0.5.0-nightly.20260901.1000",
            "0.5.0-nightly.20260901.1000",
        ),
    ] {
        assert_eq!(parse(input).to_string(), expected);
    }
}
