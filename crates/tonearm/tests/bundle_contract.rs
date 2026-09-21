//! Paketleme yapılandırmasının sessizce kayamayacağı yerler.
//!
//! `ui_contract.rs`'in kardeşi ve aynı gerekçeyle var: `tauri.conf.json` bir
//! derleyiciden geçmiyor. Yanlış bir alan pencereyi bozmaz, **paketi** bozar
//! ve bu ancak bir etiket atıldığında, üç işletim sistemine paket üretilirken
//! görülür. O an öğrenmek için en pahalı andır.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");

/// Workspace `Cargo.toml`'daki sürüm — paketin tek kaynağı (D-057).
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
    panic!("workspace Cargo.toml'da `version` bulunamadı");
}

/// MSI sürümü, Cargo sürümünün **ön-yayın eki atılmış** hâli olmalı.
///
/// Windows Installer ön-yayın etiketi kabul etmiyor: `0.0.1-beta` ile
/// paketleme `optional pre-release identifier in app version must be
/// numeric-only` diyerek düşüyor (D-063). Bu yüzden `wix.version` ayrıca
/// yazılıyor — ve tam da bu yüzden kayabilir.
///
/// D-057 sürümün **tek yerde** yaşamasına karar vermişti; `tauri.conf.json`'dan
/// `version` alanı o gün bilerek kaldırıldı, "iki yerde yazılsaydı biri
/// kayardı" gerekçesiyle. `wix.version` o ikinci yeri geri getiriyor, o yüzden
/// kaymayı bu test imkânsız kılıyor: ikisi ayrışırsa kapı kırmızı yanar.
#[test]
fn the_msi_version_follows_the_cargo_version() {
    let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).unwrap();
    let wix_version = conf["bundle"]["windows"]["wix"]["version"]
        .as_str()
        .expect("bundle.windows.wix.version yok; MSI paketlemesi ön-yayın ekiyle düşer");

    let cargo_version = workspace_version();
    let expected = cargo_version
        .split_once('-')
        .map_or(cargo_version.as_str(), |(core, _)| core);

    assert_eq!(
        wix_version, expected,
        "wix.version ({wix_version}) ile Cargo sürümü ({cargo_version}) ayrıştı; \
         MSI paketi öteki paketlerden başka bir sürüm numarası taşır"
    );
}

/// MSI sürümünün üç alanı da sayısal ve Windows'un sınırları içinde olmalı.
///
/// Şema diyor ki: ilk iki alan en fazla 255, sonraki ikisi en fazla 65535.
/// Sınırı aşan bir sürüm yine paketleme gününde patlar.
#[test]
fn the_msi_version_stays_inside_the_windows_limits() {
    let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).unwrap();
    let wix_version = conf["bundle"]["windows"]["wix"]["version"]
        .as_str()
        .unwrap();

    let fields: Vec<&str> = wix_version.split('.').collect();
    assert!(
        (3..=4).contains(&fields.len()),
        "MSI sürümü `major.minor.patch[.build]` olmalı: {wix_version}"
    );

    for (index, field) in fields.iter().enumerate() {
        let value: u32 = field
            .parse()
            .unwrap_or_else(|_| panic!("MSI sürüm alanı sayısal değil: {field} ({wix_version})"));
        let limit = if index < 2 { 255 } else { 65535 };
        assert!(
            value <= limit,
            "MSI sürümünün {}. alanı sınırı aşıyor ({value} > {limit}): {wix_version}",
            index + 1
        );
    }
}
