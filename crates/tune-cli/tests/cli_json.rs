//! CLI'nin `--json` çıktısının snapshot testleri.
//!
//! Amaç iki katlı: (1) betiklenebilirlik sözleşmesi kazara bozulmasın,
//! (2) GUI'nin aynı veriyi alacağının kanıtı dursun.
//!
//! Snapshot'ları güncellemek için: `UPDATE_SNAPSHOTS=1 cargo test -p tune-cli`

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Çalıştırmadan çalıştırmaya değişen alanlar — karşılaştırma öncesi sabitlenir.
const VOLATILE_KEYS: &[&str] = &[
    "started_at",
    "finished_at",
    "data_dir",
    "tune_version",
    "os",
    "arch",
    "command",
    "source",
];

fn fixtures() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures"))
}

fn temp_dir(label: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("tune-cli-{label}-{unique}"));
    std::fs::create_dir_all(&dir).expect("geçici dizin");
    dir
}

/// `tune` ikilisini çalıştırır; `(stdout, stderr, başarılı_mı)`.
fn run(data_dir: &Path, args: &[&str]) -> (String, String, bool) {
    run_with_music(data_dir, None, args)
}

/// `TUNE_MUSIC_DIRS` ayarlayarak çalıştırır (yerel sağlayıcı testleri için).
fn run_with_music(data_dir: &Path, music: Option<&Path>, args: &[&str]) -> (String, String, bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tune"));
    command.arg("--data-dir").arg(data_dir).args(args);
    match music {
        Some(dir) => {
            command.env("TUNE_MUSIC_DIRS", dir);
        }
        None => {
            // Geliştiricinin kendi müzik dizini testlere sızmasın.
            command.env("TUNE_MUSIC_DIRS", "/olmayan/dizin/tune-test");
        }
    }
    let output = command.output().expect("tune ikilisi çalışmalı");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

fn audio_fixtures() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/audio"))
}

/// Değişken alanları sabitler, böylece snapshot yalnızca anlamlı farkta kırılır.
fn normalize(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if VOLATILE_KEYS.contains(&key.as_str()) {
                    *child = serde_json::Value::String("<değişken>".to_owned());
                } else {
                    normalize(child);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(normalize),
        _ => {}
    }
}

fn assert_snapshot(name: &str, stdout: &str) {
    let mut value: serde_json::Value = serde_json::from_str(stdout)
        .unwrap_or_else(|err| panic!("{name}: çıktı geçerli JSON olmalı ({err}):\n{stdout}"));
    normalize(&mut value);
    let actual = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());

    let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots"))
        .join(format!("{name}.json"));

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::write(&path, &actual).expect("snapshot yazılmalı");
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "{}: snapshot yok ({err}). UPDATE_SNAPSHOTS=1 ile oluştur.",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "\n{name} snapshot'ı değişti. Kasıtlıysa: UPDATE_SNAPSHOTS=1 cargo test -p tune-cli\n"
    );
}

/// Bütün komutlar tek bir veri dizini üzerinde sırayla koşar; sıra önemli
/// (önce içe aktar, sonra istatistik).
#[test]
fn json_output_is_stable_across_subcommands() {
    let dir = temp_dir("json");
    let zip = fixtures().join("spotify_extended_mini.zip");

    let (stdout, stderr, ok) = run(&dir, &["--json", "import", zip.to_str().unwrap()]);
    assert!(ok, "import başarısız: {stderr}");
    assert_snapshot("import", &stdout);

    let (stdout, stderr, ok) = run(&dir, &["--json", "stats", "--top", "3"]);
    assert!(ok, "stats başarısız: {stderr}");
    assert_snapshot("stats", &stdout);

    let (stdout, stderr, ok) = run(&dir, &["--json", "stats", "--year", "2024", "--top", "2"]);
    assert!(ok, "stats --year başarısız: {stderr}");
    assert_snapshot("stats_2024", &stdout);

    let (stdout, stderr, ok) = run(&dir, &["--json", "resolve", "Radiohead - Creep"]);
    assert!(ok, "resolve başarısız: {stderr}");
    assert_snapshot("resolve", &stdout);

    let (stdout, stderr, ok) = run(&dir, &["--json", "library", "search", "radio"]);
    assert!(ok, "search başarısız: {stderr}");
    assert_snapshot("search", &stdout);

    let (stdout, stderr, ok) = run(&dir, &["--json", "wrapped", "--year", "2024"]);
    assert!(ok, "wrapped başarısız: {stderr}");
    assert_snapshot("wrapped_2024", &stdout);

    let (stdout, stderr, ok) = run(&dir, &["--json", "diag"]);
    assert!(ok, "diag başarısız: {stderr}");
    assert_snapshot("diag", &stdout);

    std::fs::remove_dir_all(&dir).ok();
}

/// Faz 0.5'in bitti ölçütü: `tune wrapped --out kart.png` gerçek bir PNG üretmeli.
#[test]
fn wrapped_writes_a_real_png_and_svg() {
    let dir = temp_dir("wrapped");
    let zip = fixtures().join("spotify_extended_mini.zip");
    let (_, stderr, ok) = run(&dir, &["import", zip.to_str().unwrap()]);
    assert!(ok, "{stderr}");

    let png = dir.join("kart.png");
    let (stdout, stderr, ok) = run(&dir, &["wrapped", "--out", png.to_str().unwrap()]);
    assert!(ok, "wrapped --out png başarısız: {stderr}");
    assert!(stdout.contains("yazıldı"), "{stdout}");
    let bytes = std::fs::read(&png).expect("png dosyası yazılmalı");
    assert_eq!(&bytes[0..8], b"\x89PNG\r\n\x1a\n", "gerçek PNG olmalı");

    let svg = dir.join("kart.svg");
    let (_, stderr, ok) = run(
        &dir,
        &[
            "wrapped",
            "--format",
            "story",
            "--out",
            svg.to_str().unwrap(),
        ],
    );
    assert!(ok, "wrapped --out svg başarısız: {stderr}");
    let text = std::fs::read_to_string(&svg).expect("svg dosyası yazılmalı");
    assert!(text.contains("height=\"1920\""), "story ölçüsü: {text}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Tanınmayan uzantı sessizce yanlış biçim yazmamalı; aşamayı söyleyerek düşmeli.
#[test]
fn wrapped_rejects_an_unknown_extension() {
    let dir = temp_dir("wrappedext");
    let zip = fixtures().join("spotify_account_mini.zip");
    let (_, stderr, ok) = run(&dir, &["import", zip.to_str().unwrap()]);
    assert!(ok, "{stderr}");

    let bad = dir.join("kart.gif");
    let (_, stderr, ok) = run(&dir, &["wrapped", "--out", bad.to_str().unwrap()]);
    assert!(!ok, "tanınmayan uzantı başarısız olmalı");
    assert!(stderr.contains("ADIM: WRAPPED_RENDER"), "{stderr}");
    assert!(!bad.exists(), "hatalı biçimde dosya yazılmamalı");

    std::fs::remove_dir_all(&dir).ok();
}

/// Faz 1'in bitti ölçütü: yerel dosya çalınıyor ve bir `listen` kaydı üretiyor.
///
/// Ses aygıtı yoksa test kendini atlar — susturmak değil, koşulun
/// sağlanmadığını söyleyip geçmek.
#[test]
fn playing_a_local_file_records_a_listen_in_the_same_table_as_imports() {
    let dir = temp_dir("play");
    let music = audio_fixtures();

    // Önce indeks: tarama ne bulduğunu saymalı.
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "scan"]);
    assert!(ok, "tarama başarısız: {stderr}");
    assert!(stdout.contains("indekslenen"), "{stdout}");

    // Sonra çal.
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["play", "sinüs"]);
    if !ok {
        // Ses aygıtı olmayan ortamda çalma kurulamaz; bunu ayırt et.
        if stderr.contains("PLAYBACK_OUTPUT") {
            eprintln!("ses çıkışı yok — çalma testi atlanıyor:\n{stderr}");
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
        panic!("çalma başarısız: {stderr}");
    }
    assert!(stdout.contains("kuyruğa alındı"), "{stdout}");
    assert!(stdout.contains("kaydedilen dinleme: 1"), "{stdout}");

    // §1.6'nın asıl iddiası: scrobble import verisiyle aynı tabloda.
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["stats"]);
    assert!(ok, "{stderr}");
    assert!(
        stdout.contains("Test Sanatçı"),
        "çalınan parça istatistikte görünmeli:\n{stdout}"
    );
    assert!(stdout.contains("1 çalma"), "{stdout}");

    std::fs::remove_dir_all(&dir).ok();
}

/// İndeks kalıcı: `play` tarama yapmaz, bir kez taranmış katalogdan okur.
#[test]
fn the_catalog_persists_so_play_does_not_rescan() {
    let dir = temp_dir("katalog");
    let music = audio_fixtures();

    // Bir kez tara.
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "scan", "--json"]);
    assert!(ok, "{stderr}");
    let first: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert!(
        first["write"]["inserted"].as_u64().unwrap_or(0) >= 3,
        "ilk tarama katalog satırı yazmalı: {}",
        first["write"]
    );

    // İkinci tarama: damgalar değişmedi, hiçbir dosya yeniden okunmamalı.
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "scan", "--json"]);
    assert!(ok, "{stderr}");
    let second: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(
        second["write"]["inserted"],
        serde_json::json!(0),
        "değişmemiş dosyalar yeniden yazılmamalı"
    );
    assert!(
        second["summary"]["unchanged"].as_u64().unwrap_or(0) >= 3,
        "damga eşleşmesi sayılmalı: {}",
        second["summary"]
    );

    // Asıl sınav: müzik dizini **verilmeden** arama çalışmalı.
    // Katalog diskte olduğu için `play` taramaya ihtiyaç duymuyor.
    let (stdout, stderr, ok) = run(&dir, &["play", "sinüs", "--dry-run", "--json"]);
    assert!(ok, "katalog kalıcı olmalıydı: {stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(
        value["queued"].as_array().map(Vec::len),
        Some(1),
        "taranmış katalogdan bulunmalı"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Diskten silinen dosya katalogdan düşer ama **geçmişi** silinmez.
#[test]
fn a_deleted_file_leaves_the_catalog_but_keeps_its_history() {
    let dir = temp_dir("silinen");
    let music = temp_dir("silinen-muzik");
    std::fs::copy(
        audio_fixtures().join("etiketli.flac"),
        music.join("etiketli.flac"),
    )
    .expect("fixture kopyalanmalı");

    let (_, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "scan"]);
    assert!(ok, "{stderr}");

    // Çal ki geçmişi olsun.
    let (_, stderr, ok) = run_with_music(&dir, Some(&music), &["play", "sinüs"]);
    if !ok && stderr.contains("PLAYBACK_OUTPUT") {
        eprintln!("ses çıkışı yok — test atlanıyor");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&music).ok();
        return;
    }
    assert!(ok, "{stderr}");

    // Dosyayı sil ve yeniden tara.
    std::fs::remove_file(music.join("etiketli.flac")).expect("silinmeli");
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "scan", "--json"]);
    assert!(ok, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(
        value["write"]["removed"],
        serde_json::json!(1),
        "silinen dosya katalogdan düşmeli"
    );

    // Katalogda yok...
    let (_, stderr, ok) = run_with_music(&dir, Some(&music), &["play", "sinüs", "--dry-run"]);
    assert!(!ok, "silinen dosya çalınabilir görünmemeli");
    assert!(stderr.contains("PLAYBACK_RESOLVE"), "{stderr}");

    // ...ama geçmiş duruyor. Diskten sildiğin dosya geçmişini silmez.
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["stats"]);
    assert!(ok, "{stderr}");
    assert!(
        stdout.contains("Test Sanatçı"),
        "dinleme geçmişi korunmalı:\n{stdout}"
    );

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&music).ok();
}

#[test]
fn dry_run_queues_without_playing() {
    let dir = temp_dir("dryrun");
    let music = audio_fixtures();

    // Katalog kalıcı; `play` taramıyor, önce bir kez taranmalı.
    let (_, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "scan"]);
    assert!(ok, "{stderr}");

    let (stdout, stderr, ok) = run_with_music(
        &dir,
        Some(&music),
        &["play", "sinüs", "--dry-run", "--json"],
    );
    assert!(ok, "{stderr}");

    let value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(value["played"], serde_json::json!(false));
    assert_eq!(value["listens_recorded"], serde_json::json!(0));
    assert_eq!(
        value["queued"].as_array().map(Vec::len),
        Some(1),
        "tek parça kuyruğa alınmalı"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn provider_commands_report_capabilities_and_scan_counts() {
    let dir = temp_dir("provider");
    let music = audio_fixtures();

    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "list"]);
    assert!(ok, "{stderr}");
    assert!(stdout.contains("local"), "{stdout}");
    assert!(stdout.contains("STREAM"), "yetenekler görünmeli: {stdout}");
    assert!(
        !stdout.contains("CONTROL"),
        "yerel sağlayıcı kumanda edilemez: {stdout}"
    );

    // Tarama K9'a uygun rapor vermeli: bozuk fixture sayılmalı, yutulmamalı.
    let (stdout, stderr, ok) = run_with_music(&dir, Some(&music), &["provider", "scan", "--json"]);
    assert!(ok, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    let summary = &value["summary"];
    assert!(summary["audio_files"].as_u64().unwrap_or(0) >= 4);
    assert_eq!(
        summary["failed"],
        serde_json::json!(1),
        "bozuk.flac sayılmalı: {summary}"
    );
    assert!(summary["indexed"].as_u64().unwrap_or(0) >= 3, "{summary}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn testing_an_unknown_provider_lists_the_known_ones() {
    let dir = temp_dir("providertest");
    let (_, stderr, ok) = run(&dir, &["provider", "test", "spotify"]);
    assert!(!ok, "olmayan sağlayıcı başarısız olmalı");
    assert!(stderr.contains("PROVIDER_CALL"), "{stderr}");
    assert!(
        stderr.contains("local"),
        "kullanıcıya kayıtlı sağlayıcılar söylenmeli:\n{stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn playing_with_no_match_says_what_to_do() {
    let dir = temp_dir("eslesmeyen");
    let music = audio_fixtures();
    let (_, stderr, ok) = run_with_music(&dir, Some(&music), &["play", "boylebirsarkiyok"]);

    assert!(!ok, "eşleşme yoksa başarısız olmalı");
    assert!(stderr.contains("PLAYBACK_RESOLVE"), "{stderr}");
    assert!(
        stderr.contains("provider scan"),
        "kullanıcıya ne yapacağı söylenmeli:\n{stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn human_output_names_the_stage_on_failure() {
    let dir = temp_dir("hata");
    let missing = dir.join("olmayan.zip");
    let (_, stderr, ok) = run(&dir, &["import", missing.to_str().unwrap()]);

    assert!(!ok, "olmayan dosya başarısız olmalı");
    assert!(
        stderr.contains("ADIM: IMPORT_READ"),
        "aşama basılmalı:\n{stderr}"
    );
    assert!(
        stderr.contains("tune diag"),
        "kullanıcı diag'a yönlendirilmeli:\n{stderr}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn diag_without_any_run_is_not_an_error() {
    let dir = temp_dir("bosdiag");
    let (stdout, _, ok) = run(&dir, &["diag"]);
    assert!(ok);
    assert!(stdout.contains("henüz"), "{stdout}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn human_stats_output_is_readable() {
    let dir = temp_dir("insan");
    let zip = fixtures().join("spotify_account_mini.zip");
    let (_, stderr, ok) = run(&dir, &["import", zip.to_str().unwrap()]);
    assert!(ok, "{stderr}");

    let (stdout, _, ok) = run(&dir, &["stats"]);
    assert!(ok);
    assert!(stdout.contains("en çok dinlenen sanatçılar"), "{stdout}");
    assert!(stdout.contains("Portishead"), "{stdout}");
    std::fs::remove_dir_all(&dir).ok();
}
