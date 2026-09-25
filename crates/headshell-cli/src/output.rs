//! İnsan okunur çıktı biçimleme.
//!
//! Burada **iş mantığı yok** — yalnızca çekirdeğin döndürdüğü raporları
//! terminale yazmak. Her sayı çekirdekten geldiği gibi basılır.

use headshell_core::library::SearchHit;
use headshell_core::plugin::catalog::UpdateOutcome;
use headshell_core::session::{
    ImportReport, PlayReport, PluginCatalogReport, PluginConsentReport, PluginIndexReport,
    PluginInstallReport, PluginListReport, PluginRemoveReport, PluginUpdateReport,
    ProviderListReport, ProviderTestReport, ResolveReport, ScanReport, SearchReport,
    SecretListReport, SecretWriteReport, ServerAddReport, ServerListReport, ServerRemoveReport,
    SleeveResponse, StatsResponse,
};
use headshell_core::stats::StatsReport;

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
        if let Some(note) = &candidate.disambiguation {
            let _ = writeln!(out, "not     : {note}");
        }
    } else {
        let _ = writeln!(out, "eşleşme : yok (üstveri kaynağı aday döndürmedi)");
    }
    // Beraberlik sessiz kalmamalı: seçim eşdeğerler arasından yapıldıysa
    // kullanıcı bunu görmeli, yoksa keyfi bir seçimi kesin bir cevap sanır.
    if res.tied_candidates > 1 {
        let _ = writeln!(
            out,
            "belirsiz: {} aday aynı skoru aldı; seçim belirlenimci ama keyfi",
            res.tied_candidates
        );
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

/// Sleeve kartı çıktısı.
pub fn sleeve(response: &SleeveResponse) -> String {
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

/// Eklenti listesi.
pub fn plugin_list(report: &PluginListReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if report.plugins.is_empty() {
        let _ = writeln!(out, "kurulu eklenti yok");
        return out;
    }
    for entry in &report.plugins {
        let _ = writeln!(
            out,
            "{:<14} {:<20} {}",
            entry.name,
            truncate(entry.display_name.as_deref().unwrap_or("—"), 20),
            entry.status_text()
        );
        if !entry.permissions.is_empty() {
            let _ = writeln!(out, "{:<14} istediği: {}", "", entry.permissions.describe());
        }
        // Motorun kuracağı eserler **ayrı satırda** (D-055): indirmeyi
        // eklenti değil motor yapıyor, o yüzden eklentinin izin listesine
        // karışmıyor. Karışsaydı kullanıcı "bu eklenti şuraya bağlanıyor"
        // diye okurdu, oysa bağlanan motor.
        for requirement in &entry.requires {
            let _ = writeln!(
                out,
                "{:<14} motor    : {} {} ({}) — {}",
                "",
                requirement.name,
                requirement.version,
                requirement.platform,
                requirement.state.describe()
            );
        }
    }
    let s = &report.summary;
    let _ = writeln!(
        out,
        "\n{} eklenti: {} hazır, {} kurulum bekliyor, {} onay bekliyor, {} kapalı, {} sürüm uyumsuz, {} bozuk",
        s.discovered,
        s.ready,
        s.needs_install,
        s.awaiting_approval,
        s.disabled,
        s.incompatible,
        s.broken
    );
    let _ = writeln!(out, "{}", enforcement_notice(report.permissions_enforced));
    out
}

/// Onay komutu sonucu.
pub fn plugin_consent(report: &PluginConsentReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "eklenti : {}", report.name);
    let _ = writeln!(out, "komut   : {}", report.action);
    let _ = writeln!(out, "izinler : {}", report.permissions.describe());
    // Motorun indireceği eserler ayrı satırda (D-055): bunu eklenti değil
    // motor indirir, o yüzden eklentinin izin listesine karışmıyor.
    for requirement in &report.requires {
        match requirement.asset_for(&report.platform) {
            Some(asset) => {
                let _ = writeln!(
                    out,
                    "motor   : {} {} ({}) ← {}",
                    requirement.name, requirement.version, report.platform, asset.url
                );
                let _ = writeln!(out, "{:<8}  sha256 {}", "", asset.sha256);
            }
            None => {
                let _ = writeln!(
                    out,
                    "motor   : {} {} — bu platform ({}) için yayın yok",
                    requirement.name, requirement.version, report.platform
                );
            }
        }
    }
    if !report.requires.is_empty() {
        let _ = writeln!(
            out,
            "{:<8}  kurmak için: `headshell plugin install {}`",
            "", report.name
        );
    }
    let _ = writeln!(out, "durum   : {}", report.status.describe());
    let _ = writeln!(out, "{}", enforcement_notice(report.permissions_enforced));
    out
}

/// Kurulum komutu sonucu (D-055, D-071).
pub fn plugin_install(report: &PluginInstallReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let name = &report.report.plugin;
    let _ = writeln!(out, "eklenti : {name}");
    match &report.fetched {
        Some(fetched) => {
            let _ = writeln!(
                out,
                "katalog : {} indirildi ← {}",
                fetched.version, fetched.index
            );
            for file in &fetched.files {
                let _ = writeln!(
                    out,
                    "{:<8}  {:<12} sha256 {} doğrulandı",
                    "",
                    file.path,
                    short(&file.sha256)
                );
            }
        }
        // "Katalog okunmadı" ile "katalogda yoktu" ayrı cevaplar (K9).
        None => {
            let _ = writeln!(out, "katalog : okunmadı — eklenti zaten diskteydi");
        }
    }
    let _ = writeln!(out, "platform: {}", report.platform);

    if report.declared == 0 {
        // "Hiçbir şey istemiyor" ile "bakmadım" ayrı cevaplar (K9).
        let _ = writeln!(out, "eser    : yok — bu eklenti hiçbir şey istemiyor");
    }
    for (name, outcome) in &report.report.outcomes {
        let _ = writeln!(out, "eser    : {name} — {}", outcome.describe());
    }
    let _ = writeln!(out, "izinler : {}", report.permissions.describe());
    let _ = writeln!(
        out,
        "onay    : {}",
        report.consent.describe().replace("<ad>", name)
    );
    let _ = writeln!(
        out,
        "\ndurum   : {}",
        if !report.ready {
            "eksik — eklenti bu hâliyle yüklenmez"
        } else if report.consent.is_approved() {
            "hazır — eklenti çalıştırılabilir"
        } else {
            "kuruldu — onaylanınca çalışır"
        }
    );
    out
}

/// Katalog: ne kurulabilir, ne kurulu, ne güncellenebilir (D-071).
pub fn plugin_catalog(report: &PluginCatalogReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "katalog : {}\n", report.index);
    if report.plugins.is_empty() {
        let _ = writeln!(out, "katalog boş");
    }
    for plugin in &report.plugins {
        let state = match &plugin.problem {
            Some(problem) => format!("KURULAMAZ — {problem}"),
            None => plugin.installed.describe(),
        };
        let _ = writeln!(
            out,
            "{:<14} {:<20} {:<9} {}",
            plugin.name,
            truncate(plugin.display_name.as_deref().unwrap_or("—"), 20),
            plugin.version.as_deref().unwrap_or("—"),
            state
        );
        if let Some(description) = &plugin.description {
            let _ = writeln!(out, "{:<14} {}", "", truncate(description, 100));
        }
        if plugin.problem.is_none() {
            let _ = writeln!(
                out,
                "{:<14} istediği: {}",
                "",
                plugin.permissions.describe()
            );
            // Motorun kuracağı araçlar ağ izninden ayrı satırda (D-055).
            for requirement in &plugin.requires {
                let line = if requirement.asset_for(&report.platform).is_some() {
                    format!(
                        "{} {} ({})",
                        requirement.name, requirement.version, report.platform
                    )
                } else {
                    format!(
                        "{} {} — bu platform ({}) için yayın yok",
                        requirement.name, requirement.version, report.platform
                    )
                };
                let _ = writeln!(out, "{:<14} motor   : {line}", "");
            }
        }
    }
    for name in &report.delisted {
        let _ = writeln!(
            out,
            "\n{name}: katalogdan çekilmiş ama bu makinede kurulu — kaldırmak için \
             `headshell plugin remove {name}`"
        );
    }
    let s = &report.summary;
    let _ = writeln!(
        out,
        "\n{} eklenti: {} kurulabilir, {} kurulu, {} güncelleme var, {} kurulamaz",
        s.listed, s.installable, s.installed, s.updates, s.problems
    );
    let _ = writeln!(
        out,
        "kurmak için `headshell plugin install <ad>` · güncellemek için `headshell plugin update`"
    );
    out
}

/// Güncelleme sonucu (D-071).
pub fn plugin_update(report: &PluginUpdateReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "katalog : {}\n", report.index);
    if report.plugins.is_empty() {
        let _ = writeln!(
            out,
            "katalogdan kurulmuş eklenti yok — neler var: `headshell plugin catalog`"
        );
        return out;
    }
    for plugin in &report.plugins {
        let _ = writeln!(out, "{:<14} {}", plugin.name, plugin.outcome.describe());
        if let UpdateOutcome::Updated {
            permissions_added,
            tools_changed,
            ..
        } = &plugin.outcome
        {
            if !permissions_added.is_empty() {
                let _ = writeln!(
                    out,
                    "{:<14} yeni izin istiyor: {} — `headshell plugin approve {}`",
                    "",
                    permissions_added.describe(),
                    plugin.name
                );
            }
            // Araç değişikliği onay istemez (D-071) ama söylenir: ayrı bir
            // program sessizce değişmemeli.
            for change in tools_changed {
                let _ = writeln!(
                    out,
                    "{:<14} araç değişti: {} (onay istenmez)",
                    "",
                    change.describe()
                );
            }
        }
        for (name, outcome) in &plugin.tools {
            let _ = writeln!(out, "{:<14} eser: {name} — {}", "", outcome.describe());
        }
        if let Some(error) = &plugin.tools_error {
            let _ = writeln!(out, "{:<14} ARAÇLAR KURULAMADI — {error}", "");
        }
        if let Some(consent) = &plugin.consent
            && !consent.is_approved()
        {
            let _ = writeln!(
                out,
                "{:<14} onay: {}",
                "",
                consent.describe().replace("<ad>", &plugin.name)
            );
        }
    }
    let s = &report.summary;
    let _ = writeln!(
        out,
        "\n{} eklenti: {} güncellendi, {} güncel, {} atlandı, {} başarısız",
        s.checked, s.updated, s.current, s.skipped, s.failed
    );
    out
}

/// Kaldırma sonucu (D-071).
pub fn plugin_remove(report: &PluginRemoveReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "kaldırıldı: {} ({})",
        report.name,
        report.removed.path.display()
    );
    if let Some(target) = &report.removed.link_target {
        let _ = writeln!(
            out,
            "            bir bağlantıydı; hedefine dokunulmadı: {}",
            target.display()
        );
    }
    let _ = writeln!(
        out,
        "onay      : {}",
        if report.consent_forgotten {
            "unutuldu — yeniden kurulursa baştan sorulur"
        } else {
            "kaydı yoktu"
        }
    );
    if !report.kept_secrets.is_empty() {
        let _ = writeln!(
            out,
            "sırlar    : plugin:{} içinde kaldı: {} — silmek için `headshell secret remove \
             plugin:{} <anahtar>`",
            report.name,
            report.kept_secrets.join(", "),
            report.name
        );
    }
    let _ = writeln!(
        out,
        "not       : motorun kurduğu araçlar eklentiler arasında paylaşılır, silinmedi"
    );
    out
}

/// İndeks üretimi (katalog bakımı, D-071).
pub fn plugin_index(report: &PluginIndexReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "indeks : {} — {}",
        report.path.display(),
        if report.written {
            "yazıldı"
        } else if report.up_to_date {
            "zaten güncel"
        } else {
            "yazılmadı"
        }
    );
    let _ = writeln!(out, "şablon : {}", report.url_template);
    for plugin in &report.plugins {
        let _ = writeln!(out, "\n{} {}", plugin.name, plugin.version);
        for file in &plugin.files {
            let _ = writeln!(
                out,
                "  {:<12} sha256 {}  {}",
                file.path,
                short(&file.sha256),
                file.url
            );
        }
    }
    let _ = writeln!(out, "\n{} eklenti", report.plugins.len());
    out
}

/// Karmanın ilk 12 hanesi — terminalde 64 hane okunmaz.
fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// İzinlerin ne kadarının zorlandığını söyleyen not (D-040 → D-069).
///
/// Her liste ve onay çıktısında görünüyor: kullanıcı olmayan bir korumaya
/// güvenmemeli, var olanın sınırını da bilmeli. api 2'de eklentinin kendisi
/// hapsediliyor; motorun kurduğu araçlar (yt-dlp) hapsedilmiyor.
fn enforcement_notice(enforced: bool) -> &'static str {
    if enforced {
        "not: izinler zorlanıyor — eklenti yalnızca beyan ettiği ana bilgisayarlara bağlanabilir \
         ve dosya sistemine erişemez.\n\
         motorun kurduğu araçlar (yt-dlp gibi) ayrı programlardır ve bu sınırın dışındadır."
    } else {
        "not: izinler zorlanmıyor — beyan bir sözleşmedir, güvenlik duvarı değil.\n\
         eklenti sizin bütün yetkinizle çalışır."
    }
}

/// Sır deposundaki anahtar adları.
pub fn secret_list(report: &SecretListReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if report.namespaces.is_empty() {
        let _ = writeln!(out, "kayıtlı sır yok");
        return out;
    }
    for (namespace, keys) in &report.namespaces {
        let _ = writeln!(out, "{namespace}: {}", keys.join(", "));
    }
    let _ = writeln!(out, "\ndeğerler gösterilmez; yalnızca anahtar adları.");
    out
}

/// Sır yazma/silme sonucu.
pub fn secret_write(report: &SecretWriteReport) -> String {
    let verb = if report.action == "remove" {
        "silindi"
    } else {
        "yazıldı"
    };
    format!("{} / {} {verb}\n", report.namespace, report.key)
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
            "müzik dizini bulunamadı — HEADSHELL_MUSIC_DIRS ayarlayın\n\
             (örnek: HEADSHELL_MUSIC_DIRS=~/Müzik headshell provider scan)"
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
    let _ = writeln!(out, "\nsına: headshell provider test {}", s.id);
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
             (örnek: headshell provider add subsonic --url https://muzik.ev --user adin)"
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
