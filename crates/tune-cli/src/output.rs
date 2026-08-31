//! İnsan okunur çıktı biçimleme.
//!
//! Burada **iş mantığı yok** — yalnızca çekirdeğin döndürdüğü raporları
//! terminale yazmak. Her sayı çekirdekten geldiği gibi basılır.

use tune_core::library::SearchHit;
use tune_core::session::{
    ImportReport, PlayReport, ProviderListReport, ProviderTestReport, ResolveReport, ScanReport,
    SearchReport, ServerAddReport, ServerListReport, ServerRemoveReport, StatsResponse,
    WrappedResponse,
};
use tune_core::stats::StatsReport;

/// Milisaniyeyi `12s 3dk` gibi okunur süreye çevirir.
fn duration(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}sa {minutes}dk")
    } else {
        format!("{minutes}dk")
    }
}

/// İçe aktarma raporu.
pub fn import(report: &ImportReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let i = &report.import;
    let _ = writeln!(out, "içe aktarıldı: {} ({})", i.source, i.export);
    let _ = writeln!(out, "  eşleşen dosya : {}", i.files_matched);
    let _ = writeln!(out, "  ham kayıt     : {}", i.records_total);
    let _ = writeln!(out, "  dinleme       : {}", i.listens);
    if i.skipped_total() > 0 {
        let detail = i
            .skipped
            .iter()
            .map(|(reason, count)| format!("{reason} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "  atlanan       : {} ({detail})", i.skipped_total());
    }
    let _ = writeln!(out, "  ISRC'li       : {}", i.with_isrc);

    let d = &report.identity;
    let _ = writeln!(
        out,
        "\nkimlik: isrc {} · mbid {} · bulanık {} · parmak izi {} · yerel {} (otoriteli %{:.1})",
        d.by_isrc,
        d.by_mbid,
        d.by_fuzzy,
        d.by_fingerprint,
        d.by_local_key,
        d.authoritative_ratio() * 100.0
    );

    let w = &report.write;
    let _ = writeln!(
        out,
        "kütüphane: yeni {} · tekrar {} · yeni parça {}",
        w.inserted, w.duplicates, w.new_tracks
    );
    out
}

/// İstatistik raporu.
pub fn stats(response: &StatsResponse) -> String {
    use std::fmt::Write as _;
    let r: &StatsReport = &response.report;
    let mut out = String::new();

    let scope = r
        .query
        .year
        .map_or_else(|| "tüm zamanlar".to_owned(), |y| y.to_string());
    let _ = writeln!(out, "dönem: {scope}");
    let _ = writeln!(
        out,
        "{} çalma · {:.1} saat · {} parça · {} sanatçı",
        r.plays,
        r.total_hours(),
        r.unique_tracks,
        r.unique_artists
    );
    let _ = writeln!(
        out,
        "kapsamda {} kayıt, {} kısa çalma atlandı, {} kayıt kapsam dışı, {} kayıt kimliksiz",
        r.listens_in_scope, r.skipped_short, r.out_of_scope, r.without_canonical_id
    );

    if !r.top_artists.is_empty() {
        let _ = writeln!(out, "\nen çok dinlenen sanatçılar");
        for (rank, artist) in r.top_artists.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {:>2}. {:<32} {:>5} çalma  {:>8}  {} parça",
                rank + 1,
                truncate(&artist.artist, 32),
                artist.plays,
                duration(artist.ms_played),
                artist.unique_tracks
            );
        }
    }

    if !r.top_tracks.is_empty() {
        let _ = writeln!(out, "\nen çok dinlenen parçalar");
        for (rank, track) in r.top_tracks.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {:>2}. {:<44} {:>5} çalma  {:>8}",
                rank + 1,
                truncate(&format!("{} - {}", track.artist, track.title), 44),
                track.plays,
                duration(track.ms_played)
            );
        }
    }

    if !r.top_albums.is_empty() {
        let _ = writeln!(out, "\nen çok dinlenen albümler");
        for (rank, album) in r.top_albums.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {:>2}. {:<44} {:>5} çalma",
                rank + 1,
                truncate(&format!("{} - {}", album.artist, album.album), 44),
                album.plays
            );
        }
    }

    if r.by_year.len() > 1 {
        let _ = writeln!(out, "\nyıllara göre");
        for year in &r.by_year {
            let _ = writeln!(
                out,
                "  {}  {:>6} çalma  {:>8}",
                year.year,
                year.plays,
                duration(year.ms_played)
            );
        }
    }
    out
}

/// Tek parça çözümlemesi.
pub fn resolve(report: &ResolveReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let res = &report.resolution;
    let _ = writeln!(out, "sorgu   : {} - {}", report.artist, report.title);
    let _ = writeln!(out, "kimlik  : {}", res.canonical_id);
    let _ = writeln!(
        out,
        "yöntem  : {} (güven %{:.1})",
        res.method,
        res.confidence * 100.0
    );
    if let Some(candidate) = &res.matched {
        let _ = writeln!(
            out,
            "eşleşme : {} - {} [{}]",
            candidate.artist, candidate.title, candidate.mbid
        );
    } else {
        let _ = writeln!(out, "eşleşme : yok (üstveri kaynağı aday döndürmedi)");
    }
    out
}

/// Arama sonuçları.
pub fn search(report: &SearchReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if report.hits.is_empty() {
        let _ = writeln!(out, "{:?} için sonuç yok", report.query);
        return out;
    }
    for hit in &report.hits {
        let SearchHit {
            artist,
            title,
            album,
            play_count,
            ms_played,
            ..
        } = hit;
        let _ = writeln!(
            out,
            "{:<44} {:>4} çalma  {:>8}  {}",
            truncate(&format!("{artist} - {title}"), 44),
            play_count,
            duration(*ms_played),
            album.as_deref().unwrap_or("")
        );
    }
    out
}

/// Wrapped kartı çıktısı.
pub fn wrapped(response: &WrappedResponse) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let d = &response.data;
    let period = d
        .year
        .map_or_else(|| "tüm zamanlar".to_owned(), |y| y.to_string());
    let _ = writeln!(out, "dönem: {period}");
    let _ = writeln!(
        out,
        "{} × {}  {} çalma  {} parça  {} sanatçı",
        response.size.width, response.size.height, d.plays, d.unique_tracks, d.unique_artists
    );
    if d.plays > 0 {
        let (value, unit) = if d.total_ms_played >= 3_600_000 {
            (d.total_ms_played / 3_600_000, "saat")
        } else {
            (d.total_ms_played / 60_000, "dakika")
        };
        let _ = writeln!(out, "{value} {unit} dinleme");
        if let Some(artist) = &d.top_artist {
            let _ = writeln!(out, "en çok: {} ({} çalma)", artist.artist, artist.plays);
        }
    } else {
        let _ = writeln!(out, "henüz dinleme yok");
    }
    if let Some(written) = &response.written {
        let _ = writeln!(
            out,
            "yazıldı: {} ({} bayt, {})",
            written.path.display(),
            written.bytes,
            written.kind
        );
    }
    out
}

/// Sağlayıcı listesi.
pub fn provider_list(report: &ProviderListReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if report.providers.is_empty() {
        let _ = writeln!(out, "kayıtlı sağlayıcı yok");
        return out;
    }
    for info in &report.providers {
        let _ = writeln!(
            out,
            "{:<10} {:<22} {}",
            info.id,
            truncate(&info.display_name, 22),
            info.capabilities
        );
    }
    out
}

/// Sağlayıcı sınaması.
pub fn provider_test(report: &ProviderTestReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "sağlayıcı : {}", report.info.id);
    let _ = writeln!(out, "ad        : {}", report.info.display_name);
    let _ = writeln!(out, "yetenek   : {}", report.info.capabilities);
    // "ERİŞİLEMİYOR" demiyoruz: `reachable == false`'ın iki sebebi var ve
    // biri "sunucu ayakta ama kimliği reddetti". Başlık ikisini de kapsayan
    // kelimeyi seçiyor, hangisi olduğunu alttaki `not` söylüyor (D-023).
    let _ = writeln!(
        out,
        "durum     : {}",
        if report.health.reachable {
            "kullanılabilir"
        } else {
            "KULLANILAMIYOR"
        }
    );
    if let Some(count) = report.health.track_count {
        let _ = writeln!(out, "parça     : {count}");
    }
    if let Some(detail) = &report.health.detail {
        let _ = writeln!(out, "not       : {detail}");
    }
    out
}

/// Tarama özeti.
pub fn scan(report: &ScanReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if report.dirs.is_empty() {
        let _ = writeln!(
            out,
            "müzik dizini bulunamadı — TUNE_MUSIC_DIRS ayarlayın\n\
             (örnek: TUNE_MUSIC_DIRS=~/Müzik tune provider scan)"
        );
        return out;
    }
    if !report.scanned {
        // Atlandığını **söylüyoruz**: sessizce hiçbir şey yapmamak,
        // kullanıcıya taradığımızı düşündürürdü.
        let _ = writeln!(out, "tarama atlandı ({})", report.reason);
        return out;
    }
    for dir in &report.dirs {
        let _ = writeln!(out, "tarandı: {}", dir.display());
    }
    let s = &report.summary;
    let _ = writeln!(out, "  görülen dosya : {}", s.files_seen);
    let _ = writeln!(out, "  ses dosyası   : {}", s.audio_files);
    let _ = writeln!(out, "  indekslenen   : {}", s.indexed);
    if s.unchanged > 0 {
        let _ = writeln!(out, "  değişmemiş    : {} (yeniden okunmadı)", s.unchanged);
    }
    if s.tag_fallback > 0 {
        let _ = writeln!(out, "  etiketsiz     : {} (dosya adından)", s.tag_fallback);
    }
    if s.failed > 0 {
        let _ = writeln!(out, "  okunamayan    : {}", s.failed);
    }
    if s.unreadable_dirs > 0 {
        let _ = writeln!(out, "  atlanan dizin : {}", s.unreadable_dirs);
    }

    let w = &report.write;
    let _ = writeln!(
        out,
        "katalog: yeni {} · güncellenen {} · düşen {}",
        w.inserted, w.updated, w.removed
    );
    out
}

/// Sunucu kaydı sonucu.
///
/// Token ya da anahtar **basılmaz**: çekirdeğin verdiği özet zaten onları
/// taşımıyor, burada da yeniden okunacak bir yer yok.
pub fn server_add(report: &ServerAddReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let s = &report.server;
    let _ = writeln!(out, "kaydedildi: {} ({})", s.id, s.kind);
    let _ = writeln!(out, "adres     : {}", s.url);
    let _ = writeln!(out, "kullanıcı : {}", s.username);
    let _ = writeln!(out, "kimlik    : {}", s.auth);
    let _ = writeln!(
        out,
        "doğrulama : {}",
        if report.verified {
            "sunucuya bağlanıldı"
        } else {
            "atlandı (--no-verify)"
        }
    );
    // Gözlemler yutulmuyor: zayıf entropi ya da öğrenilemeyen alan
    // kullanıcının görmesi gereken şeyler (K9).
    for note in &report.notes {
        let _ = writeln!(out, "not       : {note}");
    }
    let _ = writeln!(out, "\nsına: tune provider test {}", s.id);
    out
}

/// Sunucu kaydının silinmesi.
pub fn server_remove(report: &ServerRemoveReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "silindi: {} (kalan kayıt: {})",
        report.id, report.remaining
    );
    out
}

/// Kayıtlı uzak sunucular.
pub fn server_list(report: &ServerListReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if report.servers.is_empty() {
        let _ = writeln!(
            out,
            "kayıtlı uzak sunucu yok\n\
             (örnek: tune provider add subsonic --url https://muzik.ev --user adin)"
        );
        return out;
    }
    for server in &report.servers {
        let _ = writeln!(
            out,
            "{:<12} {:<9} {:<34} {:<14} {}",
            server.id,
            server.kind,
            truncate(&server.url, 34),
            truncate(&server.username, 14),
            server.auth
        );
    }
    // "Nereye yazıldı?" sorusu tanıya ait; kullanıcı dosyayı yedeklerken
    // ya da elle düzeltirken buna bakıyor.
    let _ = writeln!(out, "\nkayıt dosyası: {}", report.path.display());
    out
}

/// Çalma sonucu.
pub fn play(report: &PlayReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} parça kuyruğa alındı{}",
        report.queued.len(),
        if report.played { "" } else { " (çalınmadı)" }
    );
    for (index, item) in report.queued.iter().enumerate().take(10) {
        let _ = writeln!(
            out,
            "  {:>2}. {}",
            index + 1,
            truncate(&item.track.display_name(), 56)
        );
    }
    if report.queued.len() > 10 {
        let _ = writeln!(out, "  … ve {} parça daha", report.queued.len() - 10);
    }
    if report.played {
        let _ = writeln!(out, "kaydedilen dinleme: {}", report.listens_recorded);
    }
    out
}

/// Metni belirtilen genişliğe kırpar (Unicode karakter sayısına göre).
fn truncate(input: &str, width: usize) -> String {
    if input.chars().count() <= width {
        return input.to_owned();
    }
    let kept: String = input.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_switches_to_hours() {
        assert_eq!(duration(90_000), "1dk");
        assert_eq!(duration(3_600_000), "1sa 0dk");
        assert_eq!(duration(5_400_000), "1sa 30dk");
    }

    #[test]
    fn truncate_respects_unicode_boundaries() {
        assert_eq!(truncate("Şebnem", 10), "Şebnem");
        assert_eq!(truncate("Şebnem Ferah", 7), "Şebnem…");
    }
}
