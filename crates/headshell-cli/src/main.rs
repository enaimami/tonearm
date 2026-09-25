//! `headshell` — çekirdeğin elle sınandığı ince CLI kabuğu.
//!
//! **Altın Kural:** burada iş mantığı yok. Bu dosya yalnızca argüman ayrıştırır,
//! `headshell_core::session::Session` çağırır, çıktıyı biçimler ve çıkış kodu verir.
//! Bir özelliği buradan silsen çekirdek onu hâlâ sunar.

mod output;
mod tui;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use headshell_core::config::Config;
use headshell_core::ids::ProviderId;
use headshell_core::model::PlayRule;
use headshell_core::playback::LiveSession;
use headshell_core::provider;
use headshell_core::session::{self, LookupMode, Session};
use headshell_core::sleeve::CardPreset;
use headshell_core::stats::StatsQuery;

/// Çıkış kodları: 0 başarı, 1 çekirdek hatası, 2 kullanım hatası (clap).
const EXIT_FAILURE: u8 = 1;

#[derive(Debug, Parser)]
#[command(
    name = "headshell",
    version,
    about = "Sağlayıcıdan bağımsız dinleme kimliği"
)]
struct Cli {
    /// Veri dizinini elle belirt. Varsayılan sistemden sisteme değişir:
    /// Linux/BSD `~/.local/share/headshell`, macOS
    /// `~/Library/Application Support/headshell`, Windows
    /// `%LOCALAPPDATA%\headshell`. `headshell diag` kullanılanı yazar.
    #[arg(long, global = true, value_name = "DİZİN")]
    data_dir: Option<PathBuf>,

    /// Çıktıyı JSON olarak ver.
    #[arg(long, global = true)]
    json: bool,

    /// Kimlik çözümlemesinde MusicBrainz'e sor.
    ///
    /// Varsayılan kapalı: `headshell` ağ olmadan da çalışır ve bir export'u içe
    /// aktarmak kimseyi sessizce ağa bağlamaz. MusicBrainz saniyede bir
    /// istek kabul ediyor — tek parçalık `resolve` için uygun, binlerce
    /// parçalık `import` için saatler sürer.
    #[arg(long, global = true)]
    online: bool,

    /// Ayrıntılı log (tekrarlanabilir: -v, -vv).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

/// Hazır kart biçimleri (clap ValueEnum — çekirdekte yok).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CardFormat {
    Square,
    Story,
}

impl CardFormat {
    fn to_preset(self) -> CardPreset {
        match self {
            Self::Square => CardPreset::Square,
            Self::Story => CardPreset::Story,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Bir veri export arşivini (zip ya da açılmış dizin) içe aktar.
    Import {
        /// Export zip'i ya da açılmış export dizini.
        path: PathBuf,
    },
    /// Dinleme istatistikleri.
    Stats {
        /// Yalnızca bu yıl (UTC).
        #[arg(long, value_name = "YIL")]
        year: Option<i16>,
        /// Listelerde kaç satır.
        #[arg(long, default_value_t = 10, value_name = "N")]
        top: usize,
        /// "Dinlendi" sayılma eşiği (ms).
        #[arg(long, value_name = "MS")]
        min_ms: Option<u64>,
    },
    /// Tek bir parçayı kimlik zincirinden geçir.
    Resolve {
        /// `"Sanatçı - Başlık"` biçiminde sorgu.
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        query: Option<String>,
        /// Sorgu yerine bir ses dosyası: üstveri etiketlerinden okunur ve
        /// metin halkaları sonuçsuz kalırsa ses parmak izi sorulur.
        #[arg(long, value_name = "YOL")]
        file: Option<PathBuf>,
    },
    /// Kütüphane işlemleri.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Paylaşılabilir dinleme kartı (Sleeve) üret.
    Sleeve {
        /// Yalnızca bu yıl (UTC).
        #[arg(long, value_name = "YIL")]
        year: Option<i16>,
        /// Çıktı dosyası; uzantı biçimi belirler (.svg / .png).
        /// Verilmezse kart verisi yalnızca ekrana/JSON'a yazılır.
        #[arg(long, value_name = "DOSYA")]
        out: Option<PathBuf>,
        /// Kart biçimi.
        #[arg(long, default_value_t = CardFormat::Square, value_enum)]
        format: CardFormat,
    },
    /// Sağlayıcı işlemleri.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Eklenti işlemleri: katalog, kurulum, onay, güncelleme, kaldırma.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// Sır deposu: eklenti ve sağlayıcı kimlik bilgileri.
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
    /// Yerel bir parçayı çal.
    Play {
        /// Çalınacak parçayı bulmak için arama metni.
        query: String,
        /// Eşleşen ilk parça yerine tümünü kuyruğa al.
        #[arg(long)]
        all: bool,
        /// Kuyruğu karıştır.
        #[arg(long)]
        shuffle: bool,
        /// Çalmayı beklemeden çık (yalnızca kuyruğu göster).
        #[arg(long)]
        dry_run: bool,
        /// Terminal arayüzünü aç (kuyruk, ilerleme, tuş kumandası).
        #[arg(long)]
        tui: bool,
    },
    /// Son çalıştırmanın tanı raporu.
    Diag,
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    /// Kayıtlı sağlayıcıları listele.
    List,
    /// Bir sağlayıcıyı sına (ayakta mı, kaç parça görüyor).
    Test {
        /// Sağlayıcı adı (`local`).
        name: String,
    },
    /// Yerel müzik dizinlerini yeniden tara.
    Scan {
        /// Yalnızca dizinler son taramadan beri değiştiyse tara.
        ///
        /// Dizin damgalarına bakar; tam tarama yapmaz. Yerinde yeniden
        /// etiketlenen dosyaları göremez — o durumda düz `scan` gerekir.
        #[arg(long)]
        if_stale: bool,
    },
    /// Uzak bir sunucu kaydet (Subsonic ya da Jellyfin).
    ///
    /// Parola `HEADSHELL_PASSWORD` ortam değişkeninden ya da sorulan istemden
    /// alınır; komut satırına yazılmaz (kabuk geçmişine düşerdi).
    Add {
        /// Sunucu türü: `subsonic` | `jellyfin`.
        kind: String,
        /// Taban adres (`https://muzik.ev`).
        #[arg(long, value_name = "URL")]
        url: String,
        /// Kullanıcı adı.
        #[arg(long, value_name = "AD")]
        user: String,
        /// Sağlayıcı adı; verilmezse adresten türetilir.
        #[arg(long, value_name = "AD")]
        name: Option<String>,
        /// Parola yerine doğrudan API anahtarı (yalnızca Jellyfin).
        #[arg(long, value_name = "ANAHTAR")]
        api_key: Option<String>,
        /// Kaydetmeden önce sunucuya bağlanıp kimliği doğrulama.
        #[arg(long)]
        no_verify: bool,
    },
    /// Kayıtlı bir uzak sunucuyu sil.
    Remove {
        /// Sağlayıcı adı.
        name: String,
    },
    /// Kayıtlı uzak sunucuları listele (kimlik bilgisi gösterilmez).
    Servers,
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Kurulu eklentileri ve durumlarını listele (eklentileri çalıştırmaz).
    List,
    /// Eklenti kataloğunu göster: ne kurulabilir, ne kurulu, ne güncellenebilir.
    ///
    /// Katalog `headshell/plugins` deposundaki `index.json`'dur;
    /// `HEADSHELL_PLUGIN_INDEX` başka bir adres verir (bir ayna, bir çatal).
    /// Yalnızca okur, hiçbir şey kurmaz. `--online` beklemez: listeyi istemek
    /// ağa çıkmaktır.
    Catalog,
    /// Bir eklentinin beyan ettiği izinleri onayla.
    ///
    /// Onaylanan izinler zorlanır (D-069): eklenti yalnızca beyan ettiği
    /// ana bilgisayarlara bağlanabilir ve dosya sistemine erişemez. Motorun
    /// kurduğu araçlar (yt-dlp gibi) ayrı programlardır ve bu sınırın
    /// dışındadır.
    Approve {
        /// Eklenti adı (dizin adı).
        name: String,
    },
    /// Bir eklentiyi kur: diskte yoksa katalogdan indir, sonra araçlarını kur.
    ///
    /// Katalogdan gelen her dosya sha256'sıyla doğrulanır ve inen manifest
    /// katalogda gösterilenle karşılaştırılır; tutmazsa hiçbir şey yazılmaz.
    /// Kurulan eklenti **onay bekler**: `headshell plugin approve <ad>`.
    /// Diskte zaten olan eklenti için katalog okunmaz, yalnızca araçları
    /// kurulur — güncellemek `update`'in işi (D-071).
    ///
    /// Araçları (yt-dlp gibi) motor indirir, eklenti değil; her biri
    /// sabitlenmiş bir sürümle ve platform başına ayrı bir ikiliyle gelir,
    /// sha256'sı doğrulanmadan yerine konmaz. Root istenmez, Python gerekmez
    /// (D-055, D-069).
    ///
    /// `--online` beklemez: indirme komutun kendisidir, yan etkisi değil.
    Install {
        /// Eklenti adı.
        name: String,
    },
    /// Katalogdan kurulmuş eklentileri katalogdaki sürüme getir.
    ///
    /// Ad verilmezse katalogdan kurulmuş hepsi. Elle konmuş ya da yerelde
    /// değiştirilmiş bir eklentinin üstüne yazılmaz. Yeni sürüm fazladan
    /// izin istiyorsa eklenti yeniden onay bekler; araç (yt-dlp) değiştiyse
    /// çıktı bunu yazar (D-071).
    Update {
        /// Eklenti adı; verilmezse hepsi.
        name: Option<String>,
    },
    /// Bir eklentiyi kaldır: dizinini siler, onayını unutur.
    ///
    /// Sırlar ve motorun kurduğu araçlar silinmez; kalan sırların adları
    /// yazılır. Dizin bir bağlantıysa yalnızca bağlantı kaldırılır.
    Remove {
        /// Eklenti adı.
        name: String,
    },
    /// Katalog deposunun indeksini üret ya da denetle (katalog bakımı).
    ///
    /// `DİZİN` bir `headshell/plugins` kopyası: her eklenti `<ad>/plugin.json`
    /// ve betiği. Her manifest kurulumdaki doğrulamanın aynısından geçer,
    /// dosyaların sha256'sı hesaplanır ve `<DİZİN>/index.json` yazılır.
    Index {
        /// Katalog deposunun kökü.
        #[arg(value_name = "DİZİN")]
        dir: PathBuf,
        /// Dosya adresi şablonu: `{name}`, `{version}` ve `{path}` zorunlu.
        /// Verilmezse var olan `index.json`'daki şablon kullanılır.
        #[arg(long, value_name = "ŞABLON")]
        url_template: Option<String>,
        /// Hiçbir şey yazma; indeks güncel değilse neyin farklı olduğunu
        /// söyleyip başarısız ol (katalog deposunun CI'ı için).
        #[arg(long)]
        check: bool,
    },
    /// Bir eklentiyi kapat (onay kaydı korunur).
    Disable {
        /// Eklenti adı.
        name: String,
    },
    /// Kapalı bir eklentiyi yeniden aç.
    Enable {
        /// Eklenti adı.
        name: String,
    },
    /// Onayı tamamen unut; bir dahaki sefere baştan sorulur.
    Forget {
        /// Eklenti adı.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum SecretCommand {
    /// Ad alanlarını ve anahtar adlarını listele (değerler gösterilmez).
    List,
    /// Bir sır yaz. Değer `HEADSHELL_SECRET`'ten ya da yankısız istemden okunur.
    Set {
        /// Ad alanı (`plugin:soundcloud`).
        namespace: String,
        /// Anahtar adı (`client_id`).
        key: String,
    },
    /// Bir sırrı sil.
    Remove {
        /// Ad alanı.
        namespace: String,
        /// Anahtar adı.
        key: String,
    },
}

#[derive(Debug, Subcommand)]
enum LibraryCommand {
    /// Tam metin arama.
    Search {
        /// Aranacak metin.
        query: String,
        /// En fazla kaç sonuç.
        #[arg(long, default_value_t = 20, value_name = "N")]
        limit: usize,
        /// "Dinlendi" sayılma eşiği (ms) — `stats` ile aynı kural.
        #[arg(long, value_name = "MS")]
        min_ms: Option<u64>,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    // Kısmi başarı üreten bir komut (bütün eklentileri güncellemek) raporunu
    // yine basar ama sıfır dönmez: betik "bir şey olmadı"yı görebilmeli.
    let mut succeeded = true;
    match run(&cli, &mut succeeded).await {
        Ok(text) => {
            print!("{text}");
            if succeeded {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_FAILURE)
            }
        }
        Err(err) => {
            eprintln!("{}", err.chain_text());
            eprintln!("\nayrıntı için: headshell diag");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Komutu çalıştırır ve basılacak metni döndürür. Komut kısmen
/// başarısızsa `succeeded`'ı indirir; metin yine basılır.
async fn run(cli: &Cli, succeeded: &mut bool) -> headshell_core::Result<String> {
    let config = match &cli.data_dir {
        Some(dir) => Config::with_data_dir(dir),
        None => Config::discover()?,
    };
    let mut session = Session::open(config)?;
    let lookup_mode = if cli.online {
        LookupMode::Online
    } else {
        LookupMode::Offline
    };

    match &cli.command {
        Command::Import { path } => {
            let report = session
                .import_archive(path, session::lookup_for(lookup_mode)?)
                .await?;
            render(cli.json, &report, || output::import(&report))
        }
        Command::Stats { year, top, min_ms } => {
            let query = StatsQuery {
                year: *year,
                top: *top,
                min_ms_played: min_ms.unwrap_or(headshell_core::stats::DEFAULT_MIN_MS_PLAYED),
            };
            let response = session.stats(query)?;
            render(cli.json, &response, || output::stats(&response))
        }
        Command::Resolve { query, file } => {
            let report = match (query, file) {
                (_, Some(path)) => {
                    session
                        .resolve_file(
                            path,
                            session::lookup_for(lookup_mode)?,
                            session.fingerprint_lookup_for(lookup_mode)?,
                        )
                        .await?
                }
                (Some(query), None) => {
                    session
                        .resolve_track(query, session::lookup_for(lookup_mode)?)
                        .await?
                }
                // `clap` bu bileşimi zaten reddediyor (`required_unless_present`);
                // yine de sessiz bir varsayılan üretmiyoruz.
                (None, None) => {
                    return Err(headshell_core::Error::new(
                        headshell_core::diag::Stage::IdentityResolve,
                        headshell_core::error::ErrorKind::InvalidInput {
                            detail: "sorgu ya da --file verilmeli".to_owned(),
                        },
                    ));
                }
            };
            render(cli.json, &report, || output::resolve(&report))
        }
        Command::Library { command } => match command {
            LibraryCommand::Search {
                query,
                limit,
                min_ms,
            } => {
                let rule = min_ms.map_or_else(PlayRule::default, PlayRule::new);
                let report = session.search(query, *limit, rule)?;
                render(cli.json, &report, || output::search(&report))
            }
        },
        Command::Sleeve { year, out, format } => {
            let query = StatsQuery {
                year: *year,
                top: 10,
                min_ms_played: headshell_core::stats::DEFAULT_MIN_MS_PLAYED,
            };
            let size = format.to_preset().size();
            let response = session.sleeve(query, size, out.as_deref())?;
            render(cli.json, &response, || output::sleeve(&response))
        }
        Command::Provider { command } => {
            let registry = provider::default_registry(session.config())?;
            match command {
                ProviderCommand::List => {
                    let report = session.providers(&registry)?;
                    render(cli.json, &report, || output::provider_list(&report))
                }
                ProviderCommand::Test { name } => {
                    let id = ProviderId::new(name.clone());
                    let report = session.test_provider(&registry, &id).await?;
                    render(cli.json, &report, || output::provider_test(&report))
                }
                ProviderCommand::Scan { if_stale } => {
                    let report = if *if_stale {
                        session.scan_providers_if_stale(&registry).await?
                    } else {
                        session.scan_providers(&registry).await?
                    };
                    render(cli.json, &report, || output::scan(&report))
                }
                ProviderCommand::Add {
                    kind,
                    url,
                    user,
                    name,
                    api_key,
                    no_verify,
                } => {
                    let kind = provider::remote::ServerKind::parse(kind)?;
                    let id = match name {
                        Some(name) => ProviderId::new(name.clone()),
                        // Öneri çekirdekte üretiliyor: GUI de aynısını
                        // gösterecek (Altın Kural).
                        None => provider::remote::suggest_id(url, kind),
                    };
                    // Parola yalnızca istem/ortamdan: argüman olarak almak
                    // onu kabuk geçmişine ve `ps` çıktısına yazardı.
                    let password = match api_key {
                        Some(_) => None,
                        None => Some(read_password(&id)?),
                    };
                    let spec = provider::remote::NewServer {
                        id,
                        kind,
                        url: url.clone(),
                        username: user.clone(),
                        password,
                        api_key: api_key.clone(),
                        verify: !*no_verify,
                    };
                    let http = headshell_core::net::default_http_client()?;
                    let report = session.add_server(spec, http).await?;
                    render(cli.json, &report, || output::server_add(&report))
                }
                ProviderCommand::Remove { name } => {
                    let report = session.remove_server(&ProviderId::new(name.clone()))?;
                    render(cli.json, &report, || output::server_remove(&report))
                }
                ProviderCommand::Servers => {
                    let report = session.list_servers()?;
                    render(cli.json, &report, || output::server_list(&report))
                }
            }
        }
        Command::Plugin { command } => match command {
            PluginCommand::List => {
                let report = session.plugins()?;
                render(cli.json, &report, || output::plugin_list(&report))
            }
            PluginCommand::Approve { name } => {
                let report = session.approve_plugin(name)?;
                render(cli.json, &report, || output::plugin_consent(&report))
            }
            PluginCommand::Catalog => {
                let http = headshell_core::net::default_http_client()?;
                let report = session.plugin_catalog(http).await?;
                render(cli.json, &report, || output::plugin_catalog(&report))
            }
            PluginCommand::Install { name } => {
                let http = headshell_core::net::default_http_client()?;
                let report = session.install_plugin(name, http).await?;
                render(cli.json, &report, || output::plugin_install(&report))
            }
            PluginCommand::Update { name } => {
                let http = headshell_core::net::default_http_client()?;
                let report = session.update_plugins(name.as_deref(), http).await?;
                *succeeded = report.summary.failed == 0;
                render(cli.json, &report, || output::plugin_update(&report))
            }
            PluginCommand::Remove { name } => {
                let report = session.remove_plugin(name)?;
                render(cli.json, &report, || output::plugin_remove(&report))
            }
            PluginCommand::Index {
                dir,
                url_template,
                check,
            } => {
                let report = session.build_plugin_index(dir, url_template.as_deref(), *check)?;
                render(cli.json, &report, || output::plugin_index(&report))
            }
            PluginCommand::Disable { name } => {
                let report = session.disable_plugin(name)?;
                render(cli.json, &report, || output::plugin_consent(&report))
            }
            PluginCommand::Enable { name } => {
                let report = session.enable_plugin(name)?;
                render(cli.json, &report, || output::plugin_consent(&report))
            }
            PluginCommand::Forget { name } => {
                let report = session.forget_plugin(name)?;
                render(cli.json, &report, || output::plugin_consent(&report))
            }
        },
        Command::Secret { command } => match command {
            SecretCommand::List => {
                let report = session.secrets()?;
                render(cli.json, &report, || output::secret_list(&report))
            }
            SecretCommand::Set { namespace, key } => {
                // Değer argüman olarak alınmıyor: parolayla aynı gerekçe
                // (kabuk geçmişi + `ps` çıktısı).
                let value = read_hidden_value(&format!("{namespace} / {key}"))?;
                let report = session.set_secret(namespace, key, &value)?;
                render(cli.json, &report, || output::secret_write(&report))
            }
            SecretCommand::Remove { namespace, key } => {
                let report = session.remove_secret(namespace, key)?;
                render(cli.json, &report, || output::secret_write(&report))
            }
        },
        Command::Play {
            query,
            all,
            shuffle,
            dry_run,
            tui: use_tui,
        } => {
            // Tarama **yapılmıyor**: indeks kalıcı (SQLite `provider_tracks`).
            // Kullanıcı `headshell provider scan` ile bir kez tarar; `play` yalnızca
            // arar. Katalog boşsa hata kullanıcıyı taramaya yönlendirir.
            let registry = provider::default_registry(session.config())?;

            let options = session::PlayOptions {
                all: *all,
                shuffle: *shuffle,
                dry_run: *dry_run,
                ..session::PlayOptions::new(query.clone())
            };

            if *use_tui {
                // Terminal ses aygıtından **önce** sınanıyor. İkisi de
                // gerekli, ama biri bedava bir sınama, öteki bir donanım
                // kaynağı; ters sırada ses kartsız bir makinede kullanıcı
                // `--tui` yazdığı hâlde ALSA hatası görüyordu (K9: hata,
                // kullanıcının yaptığı şeyi anlatmalı).
                tui::require_terminal().map_err(|err| anyhow_to_core(&anyhow::Error::from(err)))?;

                // TUI yalnızca döngüyü sürer; tick ve dinleme kaydı
                // `LiveSession`'ın işi (çekirdekte, K1).
                let player = session.player_from_search(&registry, options).await?;
                let mut live = LiveSession::new(session, player);
                let recorded = tui::run(&mut live)
                    .await
                    .map_err(|err| anyhow_to_core(&err))?;
                return Ok(format!("kaydedilen dinleme: {recorded}\n"));
            }

            let report = session.play(&registry, options).await?;
            render(cli.json, &report, || output::play(&report))
        }
        Command::Diag => {
            let report = session.last_diag()?;
            match report {
                Some(report) => render(cli.json, &report, || report.render()),
                None => Ok(if cli.json {
                    "null\n".to_owned()
                } else {
                    "henüz çalıştırılmış bir komut yok\n".to_owned()
                }),
            }
        }
    }
}

/// TUI'den gelen terminal/G-Ç hatasını çekirdek hata tipine sarar.
///
/// TUI bir sunum katmanı ve `anyhow` kullanabiliyor (konvansiyon: CLI'de
/// serbest); ama `run` çekirdek hata tipi döndürüyor. Aşama
/// [`Stage::PlaybackOutput`]: kullanıcının gördüğü yüzey bozulmuş demek.
fn anyhow_to_core(err: &anyhow::Error) -> headshell_core::Error {
    headshell_core::Error::new(
        headshell_core::diag::Stage::PlaybackOutput,
        headshell_core::ErrorKind::Audio {
            detail: format!("terminal arayüzü: {err}"),
        },
    )
}

/// Parolanın komut satırı yerine okunabileceği ortam değişkeni.
const PASSWORD_ENV: &str = "HEADSHELL_PASSWORD";

/// Sır değerinin okunabileceği ortam değişkeni (`headshell secret set`).
const SECRET_ENV: &str = "HEADSHELL_SECRET";

/// Kullanım hatası üretir (aşama: yapılandırma okuma).
fn input_err(detail: impl Into<String>) -> headshell_core::Error {
    headshell_core::Error::new(
        headshell_core::diag::Stage::ConfigLoad,
        headshell_core::ErrorKind::InvalidInput {
            detail: detail.into(),
        },
    )
}

/// Parolayı `HEADSHELL_PASSWORD`'dan ya da terminalden **yankısız** okur.
///
/// Argüman olarak almıyoruz: komut satırına yazılan parola kabuk geçmişine
/// ve `ps` çıktısına düşer. Betikler ortam değişkenini kullanır, insanlar
/// istemi.
fn read_password(id: &ProviderId) -> headshell_core::Result<String> {
    if let Ok(from_env) = std::env::var(PASSWORD_ENV) {
        if from_env.is_empty() {
            // Boşu parola saymak, sunucunun anlamsız bir "yetkisiz"
            // hatasıyla dönmesine yol açardı.
            return Err(input_err(format!("{PASSWORD_ENV} tanımlı ama boş")));
        }
        return Ok(from_env);
    }
    prompt_password(id)
}

/// Sır değerini `HEADSHELL_SECRET`'ten ya da terminalden **yankısız** okur.
///
/// Parolayla aynı gerekçe: komut satırına yazılan bir sır kabuk geçmişine ve
/// `ps` çıktısına düşer (D-042).
fn read_hidden_value(label: &str) -> headshell_core::Result<String> {
    if let Ok(from_env) = std::env::var(SECRET_ENV) {
        if from_env.is_empty() {
            return Err(input_err(format!("{SECRET_ENV} tanımlı ama boş")));
        }
        return Ok(from_env);
    }
    prompt_hidden(&format!("{label} değeri"), SECRET_ENV)
}

/// Terminali ham kipe alıp parolayı ekrana basmadan okur.
fn prompt_password(id: &ProviderId) -> headshell_core::Result<String> {
    prompt_hidden(&format!("{id} parolası"), PASSWORD_ENV)
}

/// Yankısız istem. `env_hint`: tty yoksa kullanıcıya önerilecek değişken.
fn prompt_hidden(label: &str, env_hint: &str) -> headshell_core::Result<String> {
    use crossterm::terminal;
    use std::io::Write as _;

    eprint!("{label}: ");
    let _ = std::io::stderr().flush();

    terminal::enable_raw_mode().map_err(|source| {
        // Tty yoksa (boru hattı, CI) sessizce yankılı okumaya düşmüyoruz:
        // parolayı ekrana basmaktansa ne yapılacağını söylemek iyidir.
        input_err(format!(
            "terminal yankısız kipe alınamadı ({source}); değeri {env_hint} ile verin"
        ))
    })?;
    let secret = read_secret();
    // Ham kip her yolda geri veriliyor — hata da olsa kullanıcının terminali
    // bozuk kalmamalı (TUI'deki `TerminalGuard` ile aynı gerekçe).
    let _ = terminal::disable_raw_mode();
    eprintln!();
    secret
}

/// Ham kipte bir satır okur; hiçbir tuş ekrana yansımaz.
fn read_secret() -> headshell_core::Result<String> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

    let mut secret = String::new();
    loop {
        let event =
            event::read().map_err(|source| input_err(format!("tuş okunamadı: {source}")))?;
        let Event::Key(key) = event else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        // Ham kipte Ctrl+C sinyal üretmez; iptali kendimiz karşılıyoruz.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            return Err(input_err("giriş iptal edildi"));
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            continue;
        }
        match key.code {
            KeyCode::Enter => break,
            KeyCode::Backspace => {
                secret.pop();
            }
            KeyCode::Char(c) => secret.push(c),
            _ => {}
        }
    }
    if secret.is_empty() {
        return Err(input_err("boş değer kabul edilmiyor"));
    }
    Ok(secret)
}

/// `--json` verildiyse veriyi seri hâle getirir, yoksa insan biçimini kullanır.
fn render<T: serde::Serialize>(
    json: bool,
    value: &T,
    human: impl FnOnce() -> String,
) -> headshell_core::Result<String> {
    if json {
        let mut text = serde_json::to_string_pretty(value).map_err(|source| {
            headshell_core::Error::new(
                headshell_core::diag::Stage::ConfigLoad,
                headshell_core::ErrorKind::Json {
                    entry: "stdout".to_owned(),
                    source,
                },
            )
        })?;
        text.push('\n');
        Ok(text)
    } else {
        Ok(human())
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(format!("headshell_core={level},headshell_cli={level}"))
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
