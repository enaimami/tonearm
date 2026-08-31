// PLAN §3.1 GO/NO-GO — ATILABILIR ölçüm koşumu.
//
// Ölçülen dört şey:
//   1. 50.000 satırlık sanallaştırılmış listede kaydırma akıcılığı
//   2. Aynı anda dönen CSS animasyonunun bedeli
//   3. IPC gidiş-dönüş gecikmesi (§3.2'nin "saniyede yüzlerce mesaj" korkusu)
//   4. Rust -> JS olay akış hızı
//
// Arayüz raporu `save_report` ile diske yazar ve uygulama kendi kapanır;
// ölçüm ekrana bakmayı gerektirmesin, tekrarlanabilir olsun diye.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone)]
struct Row {
    id: u64,
    title: String,
    artist: String,
    album: String,
    ms: u64,
}

/// Faz 4'ün oda primitifiyle (D-015) aynı şekil — gerçekçi yük olsun diye.
#[derive(Serialize, Clone)]
struct Anchor {
    track: u64,
    wall_ms: u64,
    position_ms: u64,
    rate: f32,
    state: &'static str,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// En küçük yük: saf gidiş-dönüş maliyeti.
#[tauri::command]
fn ping(seq: u64) -> u64 {
    seq
}

/// Gerçekçi küçük yük: GUI'nin pozisyon için soracağı şey.
#[tauri::command]
fn anchor(track: u64, position_ms: u64) -> Anchor {
    Anchor {
        track,
        wall_ms: now_ms(),
        position_ms,
        rate: 1.0,
        state: "playing",
    }
}

/// Gerçekçi büyük yük: listenin bir sayfası.
#[tauri::command]
fn page(offset: u64, limit: u64) -> Vec<Row> {
    (offset..offset + limit)
        .map(|id| Row {
            id,
            title: format!("Parça {id}"),
            artist: format!("Sanatçı {}", id % 997),
            album: format!("Albüm {}", id % 313),
            ms: 120_000 + (id % 240_000),
        })
        .collect()
}

/// Rust -> JS olay akışı. Ayrı iş parçacığında, çağrıyı bloklamadan.
#[tauri::command]
fn emit_burst(app: AppHandle, count: u64) {
    std::thread::spawn(move || {
        for i in 0..count {
            if app.emit("tick", i).is_err() {
                return;
            }
        }
    });
}

/// Ölçüm nerede takılırsa takılsın run.log'da görünsün diye (K9 ruhu:
/// başarısızlık hangi aşamada olduğunu söylemeli).
#[tauri::command]
fn mark(msg: String) {
    eprintln!("ADIM: {msg}");
}

/// Her adımdan sonra çağrılır. Rapor **artımlı** yazılıyor: 7. adım asılırsa
/// 1-6'nın sayıları kaybolmasın. Kısmi başarı da rapor verir.
#[tauri::command]
fn save_report(json: String) {
    let path = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join("report.json");
    if let Err(e) = std::fs::write(&path, json) {
        eprintln!("RAPOR YAZILAMADI: {e}");
    }
}

#[tauri::command]
fn quit(app: AppHandle) {
    eprintln!("BITTI");
    app.exit(0);
}

/// D-029: ortam düzeltmesini uygulamanın kendisi kurar.
///
/// Varsayım değil, sınanan şey: GDK `GDK_BACKEND`'i `gtk_init` sırasında,
/// WebKit `WEBKIT_DISABLE_DMABUF_RENDERER`'ı web süreci doğarken okur —
/// ikisi de Tauri kurulumundan sonra. `main()`'in ilk satırı yeterince erken mi?
///
/// Kullanıcının kendi ayarı **ezilmiyor**: bilerek Wayland'da koşmak isteyen
/// biri `GDK_BACKEND=wayland` verdiğinde ona karışmıyoruz.
#[cfg(target_os = "linux")]
fn ortami_duzelt() {
    for (ad, deger) in [
        ("GDK_BACKEND", "x11"),
        ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
    ] {
        if std::env::var_os(ad).is_none() {
            // Güvenli: main()'in ilk işi, henüz başka iş parçacığı yok.
            // (Rust 2024'te set_var `unsafe` — gerçek GUI paketinde workspace'in
            // `unsafe_code = "forbid"` kuralıyla çakışacak, bkz. D-029.)
            unsafe { std::env::set_var(ad, deger) };
            eprintln!("ORTAM: {ad}={deger} kuruldu");
        } else {
            eprintln!("ORTAM: {ad} zaten tanımlı, dokunulmadı");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn ortami_duzelt() {}

fn main() {
    ortami_duzelt();
    let started = std::time::Instant::now();
    tauri::Builder::default()
        .setup(move |app| {
            // Pencere gerçekten görününce ölç: "başlatma" kullanıcı için budur.
            // Odak şart: WebKitGTK görünmeyen pencerede rAF'ı kısar,
            // kısılmış rAF ölçümü değil kısılmayı ölçer.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
            eprintln!("KURULUM_MS: {}", started.elapsed().as_millis());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            anchor,
            page,
            emit_burst,
            mark,
            save_report,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("tauri koşumu başlatılamadı");
}
