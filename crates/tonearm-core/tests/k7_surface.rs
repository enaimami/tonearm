//! K7 yüzey denetimi: dışa açılan tipler `uniffi` ile ifade edilebilir mi?
//!
//! Bu test `PlayOptions<'a>` sınıfı bir kaymayı yakalamak için var. O tip
//! aylarca dışa açılan yüzeyde lifetime taşıdı; kimse fark etmedi çünkü
//! `cargo clippy` bunu bir kusur saymıyor — kural PLAN.md'de yazılıydı,
//! kodda değil. Artık kodda.
//!
//! **Ne sınanıyor:** public bir `struct` / `enum` / `type` lifetime parametresi
//! aldı mı. Aldıysa `uniffi` onu bir record olarak ifade edemez (D-052).
//! Ayrıca public bir imza closure parametresi alıyor mu — K7 onu da yasaklıyor.
//!
//! **Ne sınanmıyor:** `pub fn new(x: impl Into<String>)` tarzı ergonomik
//! yapıcılar. D-052 bunları kuralın dışında bıraktı: `uniffi` yalnızca
//! işaretlenmiş öğeye bakar, Faz 6'da yanlarına `#[uniffi::constructor]`
//! eklenir.
//!
//! **Bu gerçek `uniffi` scaffolding üretimi değildir.** Gerçek kontrol
//! çekirdekteki tipleri `#[derive(uniffi::Record)]` ile işaretlemeyi
//! gerektirir; o iş Faz 6'ya ait ve D-052'nin "bilinen açık"ı (boxed future
//! taşıyan dört trait) onu bugün zaten kırmızı yakardı. Bu test o kontrolün
//! yerine geçmez, ondan önce gelen ucuz bir süzgeçtir.

// K8 testleri muaf tutuyor; burada `expect` bir kusur değil: dosya
// okunamıyorsa denetim sessizce boş geçmemeli, gürültüyle düşmeli.
#![allow(clippy::expect_used)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Lifetime taşımasına **izin verilen** public tipler.
///
/// Üçü de aynı şey: `async fn` yerine elle yazılmış kutulanmış future.
/// Trait'in `dyn` uyumlu olması gerekiyor (K7 `Arc<dyn Trait>`'i serbest
/// bırakıyor) ve Rust'ta bunun makrosuz başka yolu yok.
///
/// Bu liste **borç kaydıdır, muafiyet değil**: `uniffi` bir trait metodunun
/// dönüşünde `Pin<Box<dyn Future + Send + 'a>>` ifade edemez. Faz 6'da bu
/// dört trait yeniden yazılacak (D-052). Listeye yeni bir ad eklemek, o
/// borcu büyütmek demektir — önce sor.
const BOXED_FUTURE_ALIASES: &[&str] = &["ProviderFuture", "HttpFuture", "LookupFuture"];

#[test]
fn no_public_type_carries_a_lifetime() {
    let mut findings = String::new();

    for file in core_sources() {
        let source = std::fs::read_to_string(&file).expect("kaynak dosya okunabilmeli");
        let body = without_test_modules(&source);

        for (line_no, line) in body.lines().enumerate() {
            let Some((kind, name, generics)) = public_type_header(line) else {
                continue;
            };
            if !generics.contains('\'') {
                continue;
            }
            if BOXED_FUTURE_ALIASES.contains(&name) {
                continue;
            }
            let _ = writeln!(
                findings,
                "  {}:{} — pub {kind} {name}{generics}",
                display_path(&file),
                line_no + 1,
            );
        }
    }

    assert!(
        findings.is_empty(),
        "ADIM: K7_SURFACE\n\
         Dışa açılan tipte lifetime var; `uniffi` bunu record olarak ifade edemez:\n\
         {findings}\n\
         Düzeltme: ödünç alanı sahipli tipe çevir (`&'a str` → `String`).\n\
         Gerekçe PLAN.md §2 K7 ve D-052. Bu gerçekten kaçınılmazsa listeyi\n\
         genişletmeden önce sor — `BOXED_FUTURE_ALIASES` bir borç kaydıdır."
    );
}

#[test]
fn no_public_signature_takes_a_closure() {
    let mut findings = String::new();

    for file in core_sources() {
        let source = std::fs::read_to_string(&file).expect("kaynak dosya okunabilmeli");
        let body = without_test_modules(&source);

        for (line_no, signature) in public_signatures(&body) {
            if signature.contains("impl Fn")
                || signature.contains("F: Fn")
                || signature.contains("dyn Fn")
            {
                let _ = writeln!(
                    findings,
                    "  {}:{} — {}",
                    display_path(&file),
                    line_no,
                    signature.trim(),
                );
            }
        }
    }

    assert!(
        findings.is_empty(),
        "ADIM: K7_SURFACE\n\
         Dışa açılan imza closure parametresi alıyor; K7 bunu yasaklıyor\n\
         çünkü `uniffi` closure'ı bağlamalara geçiremez:\n\
         {findings}\n\
         Düzeltme: closure yerine `Arc<dyn Trait>` al — K7 onu serbest bırakıyor\n\
         ve `uniffi` callback interface olarak modelliyor."
    );
}

/// `tonearm-core/src` altındaki bütün `.rs` dosyaları.
fn core_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(
        !files.is_empty(),
        "çekirdek kaynakları bulunamadı: {root:?}"
    );
    files.sort();
    files
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("kaynak dizini okunabilmeli");
    for entry in entries {
        let path = entry.expect("dizin girdisi").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Kaynağın test modülleri **çıkarılmış** hâli.
///
/// Testler K7'ye tabi değil: orada `&str` ödünç almak serbest. Ama yalnızca
/// test bloğu düşer, dosyanın geri kalanı değil — ilk `#[cfg(test)]`'ten
/// sonrasını topluca kesmek, test modülünden *sonra* tanımlanan her public
/// tipi denetimin dışında bırakıyordu. Bu kusur, denetimin kendisini
/// sınarken çıktı: enjekte edilen ihlal yakalanmadı çünkü dosyanın sonundaydı.
///
/// Süslü parantez sayarak atlıyoruz; satır numaraları korunsun diye atlanan
/// satırlar boşaltılıyor, silinmiyor.
fn without_test_modules(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut depth = 0usize;
    let mut in_test_module = false;
    let mut armed = false;

    for line in source.lines() {
        if in_test_module {
            depth += line.matches('{').count();
            depth = depth.saturating_sub(line.matches('}').count());
            if depth == 0 {
                in_test_module = false;
            }
            out.push('\n');
            continue;
        }

        if line.trim_start().starts_with("#[cfg(test)]") {
            armed = true;
            out.push('\n');
            continue;
        }

        // `#[cfg(test)]` bir `mod`'u işaretliyorsa bloğu atla; bir `use`'u
        // ya da tek bir öğeyi işaretliyorsa yalnızca o satır düşer.
        if armed {
            armed = false;
            if line.contains("mod ") {
                let opens = line.matches('{').count();
                let closes = line.matches('}').count();
                if opens > closes {
                    in_test_module = true;
                    depth = opens - closes;
                }
                out.push('\n');
                continue;
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

/// `pub struct/enum/type Ad<...>` satırını parçalar.
///
/// Dönüş: (tür, ad, generic listesi). Generic yoksa `None`.
fn public_type_header(line: &str) -> Option<(&'static str, &str, &str)> {
    let kind = ["struct", "enum", "type"]
        .into_iter()
        .find(|kind| line.starts_with(&format!("pub {kind} ")))?;

    let rest = line["pub ".len() + kind.len() + 1..].trim_start();
    let open = rest.find('<')?;
    let name = &rest[..open];
    // Ad ile `<` arasında boşluk varsa bu bir tip başlığı değil.
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    let close = rest.rfind('>')?;
    if close <= open {
        return None;
    }
    Some((kind, name, &rest[open..=close]))
}

/// Public fonksiyon imzalarını (çok satırlı olanlar birleştirilmiş) döndürür.
fn public_signatures(body: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = body.lines().collect();
    let mut out = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ")) {
            continue;
        }

        // İmza `)` ile kapanana kadar topla; gövdeye girme.
        let mut signature = String::new();
        for line in lines.iter().skip(index).take(20) {
            signature.push_str(line.trim());
            signature.push(' ');
            if line.contains(')') {
                break;
            }
        }
        out.push((index + 1, signature));
    }

    out
}

/// Hata mesajında depo köküne göre yol göster — mutlak yol gürültü.
fn display_path(path: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |rel| rel.display().to_string(),
    )
}
