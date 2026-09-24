//! `fixtures/` altındaki kırpılmış gerçek export'lar üzerinden uçtan uca test.
//!
//! Ağa çıkılmaz, geçici dizin kullanılır.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::PathBuf;

use headshell_core::config::Config;
use headshell_core::identity::ResolveMethod;
use headshell_core::model::{ExportKind, PlayRule};
use headshell_core::session::{self, Session};
use headshell_core::stats::StatsQuery;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures")).join(name)
}

/// Test başına tekil geçici dizin. `std` dışına çıkmadan.
fn temp_dir(label: &str) -> support::TempDir {
    support::TempDir::new(&format!("import-{label}"))
}

#[tokio::test]
async fn extended_export_imports_resolves_and_counts() {
    let dir = temp_dir("extended");
    let mut sess = Session::open(Config::with_data_dir(dir.path())).unwrap();

    let report = sess
        .import_archive(
            &fixture("spotify_extended_mini.zip"),
            session::default_lookup(),
        )
        .await
        .unwrap();

    assert_eq!(report.import.export, ExportKind::SpotifyExtended);
    assert_eq!(
        report.import.files_matched, 2,
        "iki geçmiş dosyası eşleşmeli"
    );
    assert_eq!(report.import.records_total, 63);
    assert_eq!(report.import.listens, 60);
    assert_eq!(
        report.import.skipped_total(),
        3,
        "2 podcast + 1 bozuk zaman"
    );

    // Faz 0'da ağ yok: her şey yerel anahtara düşmeli, uydurma otorite olmamalı.
    assert_eq!(report.identity.total, 60);
    assert_eq!(report.identity.by_local_key, 60);
    assert_eq!(report.identity.authoritative_ratio(), 0.0);

    assert_eq!(report.write.inserted, 60);
    assert_eq!(report.write.duplicates, 0);
    // "Creep" ve "Creep - Remastered" tek parçada birleşmeli.
    assert_eq!(
        report.write.new_tracks, 6,
        "7 satırdan 6 benzersiz parça çıkmalı"
    );

    assert!(report.diag.succeeded());
    assert_eq!(report.diag.counters["import.listens"], 60);
    assert_eq!(report.diag.counters["identity.by_local_key"], 60);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn reimporting_the_same_export_is_idempotent() {
    let dir = temp_dir("idempotent");
    let mut sess = Session::open(Config::with_data_dir(dir.path())).unwrap();
    let path = fixture("spotify_extended_mini.zip");

    let first = sess
        .import_archive(&path, session::default_lookup())
        .await
        .unwrap();
    let second = sess
        .import_archive(&path, session::default_lookup())
        .await
        .unwrap();

    assert_eq!(first.write.inserted, 60);
    assert_eq!(second.write.inserted, 0);
    assert_eq!(second.write.duplicates, 60);

    let stats = sess.stats(StatsQuery::default()).unwrap();
    assert_eq!(
        stats.report.listens_in_scope, 60,
        "ikinci içe aktarma sayıyı şişirmemeli"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn account_export_is_detected_and_stats_reflect_it() {
    let dir = temp_dir("account");
    let mut sess = Session::open(Config::with_data_dir(dir.path())).unwrap();

    let report = sess
        .import_archive(
            &fixture("spotify_account_mini.zip"),
            session::default_lookup(),
        )
        .await
        .unwrap();
    assert_eq!(report.import.export, ExportKind::SpotifyAccount);
    assert_eq!(report.import.listens, 2);
    assert_eq!(report.import.skipped_total(), 1);

    let stats = sess.stats(StatsQuery::default()).unwrap();
    assert_eq!(stats.report.plays, 2);
    assert_eq!(stats.report.unique_artists, 2);
    assert_eq!(
        stats.report.without_canonical_id, 0,
        "içe aktarma kimlik atamalı"
    );

    let hits = sess.search("portis", 10, PlayRule::default()).unwrap();
    assert_eq!(hits.hits.len(), 1);
    assert_eq!(hits.hits[0].title, "Roads");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn stats_group_creep_variants_together() {
    let dir = temp_dir("stats");
    let mut sess = Session::open(Config::with_data_dir(dir.path())).unwrap();
    sess.import_archive(
        &fixture("spotify_extended_mini.zip"),
        session::default_lookup(),
    )
    .await
    .unwrap();

    // Eşiği sıfırlayıp bakıyoruz: fixture'da "Creep" kayıtlarının yarısı kasten
    // kısa çalma, varsayılan eşikle elenirler. Birleşmeyi ölçmek istediğimiz
    // için burada hepsini sayıyoruz.
    let all = sess
        .stats(StatsQuery {
            min_ms_played: 0,
            ..StatsQuery::default()
        })
        .unwrap();
    let creep = all
        .report
        .top_tracks
        .iter()
        .find(|t| t.title.starts_with("Creep"))
        .expect("Creep listede olmalı");
    assert_eq!(
        creep.plays, 18,
        "\"Creep\" ve \"Creep - Remastered\" tek satırda toplanmalı: {creep:?}"
    );

    let stats = sess.stats(StatsQuery::default()).unwrap();
    assert_eq!(stats.report.unique_tracks, 6);
    assert_eq!(
        stats.report.skipped_short, 9,
        "kısa çalmalar sayılmalı, yutulmamalı"
    );
    assert_eq!(stats.report.by_year.len(), 2, "2023 ve 2024");

    let only_2024 = sess
        .stats(StatsQuery {
            year: Some(2024),
            ..StatsQuery::default()
        })
        .unwrap();
    assert!(only_2024.report.out_of_scope > 0);
    assert_eq!(only_2024.report.by_year.len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn unsupported_archive_fails_at_the_detect_stage() {
    let dir = temp_dir("unsupported");
    let mut sess = Session::open(Config::with_data_dir(dir.path())).unwrap();
    let bogus = dir.join("bogus");
    std::fs::create_dir_all(&bogus).unwrap();
    std::fs::write(bogus.join("notes.txt"), b"bu bir export degil").unwrap();

    let err = sess
        .import_archive(&bogus, session::default_lookup())
        .await
        .unwrap_err();
    assert_eq!(err.stage(), headshell_core::diag::Stage::ImportDetect);
    assert!(
        err.chain_text().contains("notes.txt"),
        "{}",
        err.chain_text()
    );

    // Hata da tanı raporuna yazılmış olmalı — `headshell diag` bunu göstermeli.
    let diag = sess
        .last_diag()
        .unwrap()
        .expect("son çalıştırma kaydedilmeli");
    assert!(!diag.succeeded());
    assert_eq!(
        diag.failed_at,
        Some(headshell_core::diag::Stage::ImportDetect)
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn resolve_reports_the_chain_step_it_used() {
    let dir = temp_dir("resolve");
    let sess = Session::open(Config::with_data_dir(dir.path())).unwrap();

    let report = sess
        .resolve_track("Radiohead - Creep", session::default_lookup())
        .await
        .unwrap();
    assert_eq!(report.artist, "Radiohead");
    assert_eq!(report.title, "Creep");
    assert_eq!(report.resolution.method, ResolveMethod::LocalKey);

    let err = sess
        .resolve_track("ayirici yok", session::default_lookup())
        .await
        .unwrap_err();
    assert_eq!(err.stage(), headshell_core::diag::Stage::IdentityResolve);

    std::fs::remove_dir_all(&dir).ok();
}
