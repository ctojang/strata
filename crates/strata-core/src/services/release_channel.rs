// SPDX-License-Identifier: MIT

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildKind {
    Stable,
    Nightly,
    Alpha,
    Beta,
    Rc,
}

impl BuildKind {
    pub(crate) fn localized_label(self) -> String {
        crate::i18n::tr(match self {
            BuildKind::Stable => "Stable",
            BuildKind::Nightly => "build_kind.nightly",
            BuildKind::Alpha => "Alpha",
            BuildKind::Beta => "Beta",
            BuildKind::Rc => "Release candidate",
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Ordinal {
    primary: u64,
    suffix: u64,
}

#[derive(Clone, Debug)]
pub struct Prerelease {
    kind: BuildKind,
    ordinal: Ordinal,
}

/// [`Version::parse`] is the *only* place release tags are interpreted
/// anywhere in the codebase. It accepts an optional leading `v` and exactly
/// three grammar forms:
///
/// ```text
/// v?MAJOR.MINOR.PATCH
/// v?MAJOR.MINOR.PATCH-alpha.N
/// v?MAJOR.MINOR.PATCH-beta.N
/// v?MAJOR.MINOR.PATCH-rc.N
/// v?MAJOR.MINOR.PATCH-nightly.YYYYMMDD[.N]
/// ```
///
/// Anything else returns `None` -- there is no silent zero-fill fallback.
#[derive(Clone, Debug)]
pub struct Version {
    core: (u64, u64, u64),
    prerelease: Option<Prerelease>,
}

/// Parses a string as a strictly non-negative, non-signed decimal `u64`.
///
/// `str::parse` alone is not strict enough here: it accepts a leading `+`,
/// which would let a malformed tag segment slip through.
fn parse_strict_u64(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_core(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parse_strict_u64(parts.next()?)?;
    let minor = parse_strict_u64(parts.next()?)?;
    let patch = parse_strict_u64(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn parse_prerelease(suffix: &str) -> Option<Prerelease> {
    let mut parts = suffix.split('.');
    match parts.next()? {
        kind @ ("alpha" | "beta" | "rc") => {
            let n = parse_strict_u64(parts.next()?)?;
            if n == 0 || parts.next().is_some() {
                return None;
            }
            let kind = match kind {
                "alpha" => BuildKind::Alpha,
                "beta" => BuildKind::Beta,
                _ => BuildKind::Rc,
            };
            Some(Prerelease {
                kind,
                ordinal: Ordinal {
                    primary: n,
                    suffix: 0,
                },
            })
        }
        "nightly" => {
            let date_str = parts.next()?;
            if date_str.len() != 8 {
                return None;
            }
            let date = parse_strict_u64(date_str)?;
            let suffix_n = match parts.next() {
                Some(n_str) => parse_strict_u64(n_str)?,
                None => 0,
            };
            if parts.next().is_some() {
                return None;
            }
            Some(Prerelease {
                kind: BuildKind::Nightly,
                ordinal: Ordinal {
                    primary: date,
                    suffix: suffix_n,
                },
            })
        }
        _ => None,
    }
}

impl Version {
    /// Parses a release tag per the grammar documented on [`Version`].
    /// Returns `None` for anything that does not match exactly.
    pub fn parse(tag: &str) -> Option<Version> {
        let rest = tag.strip_prefix('v').unwrap_or(tag);
        if rest.is_empty() {
            return None;
        }
        let (core_str, prerelease_str) = match rest.split_once('-') {
            Some((core, suffix)) => (core, Some(suffix)),
            None => (rest, None),
        };
        let core = parse_core(core_str)?;
        let prerelease = match prerelease_str {
            Some(suffix) => Some(parse_prerelease(suffix)?),
            None => None,
        };
        Some(Version { core, prerelease })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (major, minor, patch) = self.core;
        write!(f, "{major}.{minor}.{patch}")?;
        match &self.prerelease {
            Some(prerelease) => match prerelease.kind {
                BuildKind::Alpha => write!(f, "-alpha.{}", prerelease.ordinal.primary),
                BuildKind::Beta => write!(f, "-beta.{}", prerelease.ordinal.primary),
                BuildKind::Rc => write!(f, "-rc.{}", prerelease.ordinal.primary),
                BuildKind::Nightly => {
                    let date = prerelease.ordinal.primary;
                    let suffix_n = prerelease.ordinal.suffix;
                    if suffix_n == 0 {
                        write!(f, "-nightly.{date}")
                    } else {
                        write!(f, "-nightly.{date}.{suffix_n}")
                    }
                }
                BuildKind::Stable => Ok(()),
            },
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
