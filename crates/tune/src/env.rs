//! Linux'ta pencere ortamının düzeltilmesi (D-029, D-031).
//!
//! **Neden gerekli.** §3.1 ölçümü (D-028) Wayland + DMABUF yolunda kare
//! hızının 2.4× düştüğünü buldu: 58.8 → 23.8 fps. İki değişken **birlikte**
//! gerekiyor; DMABUF'u tek başına kapatmak kaydırmayı *kötüleştiriyor*.
//!
//! **Neden `exec`.** `std::env::set_var` Rust 2024'te `unsafe` ve workspace
//! `unsafe_code = "forbid"` diyor — `forbid` paket düzeyinde `allow` ile
//! geçersiz kılınamaz. `CommandExt::exec` güvenli bir çağrı: süreç imajını
//! değiştirir, PID korunur (masaüstü/servis bütünleşmesi bozulmaz), yeni
//! ortam çocuğa doğar.
//!
//! **Döngü koruması yapıdan geliyor, bayraktan değil:** yalnızca *eksik*
//! değişkenler kuruluyor. Yeniden başlayan sürecin gözünde eksik yok, o
//! yüzden ikinci kez `exec` etmiyor.

/// Kullanıcının kendi ayarı **ezilmiyor**: bilerek Wayland'da koşmak isteyen
/// biri `GDK_BACKEND=wayland` verdiğinde ona karışmıyoruz.
#[cfg(target_os = "linux")]
const DUZELTME: [(&str, &str); 2] = [
    ("GDK_BACKEND", "x11"),
    ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
];

/// Ortamı düzeltip süreci yeniden başlatır. Gerekmiyorsa hiçbir şey yapmaz.
///
/// Dönerse iki şeyden biri olmuştur: düzeltme gerekmedi, ya da yeniden
/// başlatma başarısız oldu. İkincisinde sessizce yavaş çalışmıyoruz — sebep
/// yazılıyor (K9).
#[cfg(target_os = "linux")]
pub fn duzelt() {
    use std::os::unix::process::CommandExt as _;

    let eksik: Vec<_> = DUZELTME
        .iter()
        .filter(|(ad, _)| std::env::var_os(ad).is_none())
        .collect();
    if eksik.is_empty() {
        return;
    }

    let Ok(kendi) = std::env::current_exe() else {
        eprintln!("ADIM: ORTAM_DUZELTME — kendi yolum bulunamadı, düzeltme atlandı");
        return;
    };

    let mut komut = std::process::Command::new(kendi);
    komut.args(std::env::args_os().skip(1));
    for (ad, deger) in &eksik {
        komut.env(ad, deger);
    }
    let kurulan: Vec<&str> = eksik.iter().map(|(ad, _)| *ad).collect();
    eprintln!(
        "ADIM: ORTAM_DUZELTME — {} kurulup yeniden başlatılıyor (D-028)",
        kurulan.join(", ")
    );

    // `exec` yalnızca **başarısızsa** döner.
    let hata = komut.exec();
    eprintln!("ADIM: ORTAM_DUZELTME — yeniden başlatılamadı ({hata}), düzeltmesiz devam");
}

/// Linux dışında düzeltilecek bir şey yok: ölçüm WebKitGTK'ya özgüydü.
#[cfg(not(target_os = "linux"))]
pub fn duzelt() {}
