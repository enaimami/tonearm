//! The places where the packaging configuration must not drift silently.
//!
//! The sibling of `ui_contract.rs`, and it exists for the same reason:
//! `tauri.conf.json` does not go through a compiler. A wrong field does not
//! break the window, it breaks **the package**, and that is only seen when a
//! tag is pushed, while packages are being built for three operating
//! systems. That is the most expensive moment to learn it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");

/// The version in the workspace `Cargo.toml` — the package's single source
/// (D-057).
fn workspace_version() -> String {
    for line in WORKSPACE_MANIFEST.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("version") {
            let Some((_, value)) = rest.split_once('=') else {
                continue;
            };
            return value.trim().trim_matches('"').to_owned();
        }
    }
    panic!("no `version` found in the workspace Cargo.toml");
}

/// The MSI version must be the Cargo version **with the pre-release suffix
/// dropped**.
///
/// Windows Installer does not accept a pre-release tag: packaging with
/// `0.0.1-beta` fails saying `optional pre-release identifier in app version
/// must be numeric-only` (D-063). That is why `wix.version` is written
/// separately — and exactly why it can drift.
///
/// D-057 decided that the version lives **in one place**; that day the
/// `version` field was removed from `tauri.conf.json` on purpose, on the
/// grounds that "if it were written in two places one would drift".
/// `wix.version` brings that second place back, so this test makes drifting
/// impossible: if the two part ways, the gate turns red.
#[test]
fn the_msi_version_follows_the_cargo_version() {
    let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).unwrap();
    let wix_version = conf["bundle"]["windows"]["wix"]["version"]
        .as_str()
        .expect("no bundle.windows.wix.version; MSI packaging fails with the pre-release suffix");

    let cargo_version = workspace_version();
    let expected = cargo_version
        .split_once('-')
        .map_or(cargo_version.as_str(), |(core, _)| core);

    assert_eq!(
        wix_version, expected,
        "wix.version ({wix_version}) and the Cargo version ({cargo_version}) have parted ways; \
         the MSI package would carry a different version number from the other packages"
    );
}

/// All three fields of the MSI version must be numeric and within Windows'
/// limits.
///
/// The schema says: the first two fields at most 255, the next two at most
/// 65535. A version over the limit also blows up on packaging day.
#[test]
fn the_msi_version_stays_inside_the_windows_limits() {
    let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).unwrap();
    let wix_version = conf["bundle"]["windows"]["wix"]["version"]
        .as_str()
        .unwrap();

    let fields: Vec<&str> = wix_version.split('.').collect();
    assert!(
        (3..=4).contains(&fields.len()),
        "the MSI version must be `major.minor.patch[.build]`: {wix_version}"
    );

    for (index, field) in fields.iter().enumerate() {
        let value: u32 = field.parse().unwrap_or_else(|_| {
            panic!("an MSI version field is not numeric: {field} ({wix_version})")
        });
        let limit = if index < 2 { 255 } else { 65535 };
        assert!(
            value <= limit,
            "field {} of the MSI version exceeds its limit ({value} > {limit}): {wix_version}",
            index + 1
        );
    }
}
