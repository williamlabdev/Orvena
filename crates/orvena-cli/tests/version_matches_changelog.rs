//! The crate version must match the newest released CHANGELOG entry.
//!
//! The `v0.9.0` tag was cut while `workspace.package.version` still said
//! 0.8.0, so a binary built from that tag reported the wrong version. CI is
//! not always available to catch this, so the local test suite is the gate:
//! bump the version and the CHANGELOG together, or this fails.

#[test]
fn crate_version_matches_newest_released_changelog_entry() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../CHANGELOG.md");
    let changelog = std::fs::read_to_string(path).expect("CHANGELOG.md at the workspace root");

    // First `## [x.y.z]` heading. `## [Unreleased]` is skipped because it has
    // no digits; anything else in brackets is not a Keep-a-Changelog release.
    let released = changelog
        .lines()
        .filter_map(|l| l.strip_prefix("## ["))
        .filter_map(|rest| rest.split_once(']').map(|(v, _)| v.trim()))
        .find(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .expect("CHANGELOG.md has at least one `## [x.y.z]` release heading");

    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        released,
        "workspace.package.version ({}) != newest released CHANGELOG entry ({}). \
         Bump Cargo.toml and CHANGELOG.md together before tagging.",
        env!("CARGO_PKG_VERSION"),
        released
    );
}
