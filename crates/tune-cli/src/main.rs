//! `tune` — çekirdeğin elle sınandığı ince CLI kabuğu.
//!
//! **Altın Kural:** burada iş mantığı yok. Bu dosya yalnızca argüman ayrıştırır,
//! `tune_core::session::Session` çağırır, çıktıyı biçimler ve çıkış kodu verir.
//! Bir özelliği buradan silsen çekirdek onu hâlâ sunar.

mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tune_core::config::Config;
use tune_core::ids::ProviderId;
use tune_core::model::PlayRule;
use tune_core::provider;
use tune_core::session::{self, Session};
use tune_core::stats::StatsQuery;
use tune_core::wrapped::CardPreset;

/// Çıkış kodları: 0 başarı, 1 çekirdek hatası, 2 kullanım hatası (clap).
const EXIT_FAILURE: u8 = 1;

#[derive(Debug, Parser)]
#[command(
    name = "tune",
    version,
    about = "Sağlayıcıdan bağımsız dinleme kimliği"
)]
struct Cli {
    /// Veri dizinini elle belirt (varsayılan: $XDG_DATA_HOME/tune).
    #[arg(long, global = true, value_name = "DİZİN")]
    data_dir: Option<PathBuf>,

    /// Çıktıyı JSON olarak ver.
    #[arg(long, global = true)]
    json: bool,

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
        query: String,
    },
    /// Kütüphane işlemleri.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Paylaşılabilir dinleme kartı (Wrapped) üret.
    Wrapped {
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
    Scan,
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

    match run(&cli).await {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{}", err.chain_text());
            eprintln!("\nayrıntı için: tune diag");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Komutu çalıştırır ve basılacak metni döndürür.
async fn run(cli: &Cli) -> tune_core::Result<String> {
    let config = match &cli.data_dir {
        Some(dir) => Config::with_data_dir(dir),
        None => Config::discover()?,
    };
    let mut session = Session::open(config)?;

    match &cli.command {
        Command::Import { path } => {
            let report = session
                .import_archive(path, session::default_lookup())
                .await?;
            render(cli.json, &report, || output::import(&report))
        }
        Command::Stats { year, top, min_ms } => {
            let query = StatsQuery {
                year: *year,
                top: *top,
                min_ms_played: min_ms.unwrap_or(tune_core::stats::DEFAULT_MIN_MS_PLAYED),
            };
            let response = session.stats(query)?;
            render(cli.json, &response, || output::stats(&response))
        }
        Command::Resolve { query } => {
            let report = session
                .resolve_track(query, session::default_lookup())
                .await?;
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
        Command::Wrapped { year, out, format } => {
            let query = StatsQuery {
                year: *year,
                top: 10,
                min_ms_played: tune_core::stats::DEFAULT_MIN_MS_PLAYED,
            };
            let size = format.to_preset().size();
            let response = session.wrapped(query, size, out.as_deref())?;
            render(cli.json, &response, || output::wrapped(&response))
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
                ProviderCommand::Scan => {
                    let report = session.scan_providers(&registry).await?;
                    render(cli.json, &report, || output::scan(&report))
                }
            }
        }
        Command::Play {
            query,
            all,
            shuffle,
            dry_run,
        } => {
            // Tarama indeksi bellekte: aynı süreçte önce taramak gerekiyor.
            // (Kalıcı indeks Faz 1.2'nin devamı — SQLite'a yazılacak.)
            let registry = provider::default_registry(session.config())?;
            session.scan_providers(&registry).await?;

            let options = session::PlayOptions {
                query,
                all: *all,
                shuffle: *shuffle,
                dry_run: *dry_run,
                ..session::PlayOptions::new(query)
            };
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

/// `--json` verildiyse veriyi seri hâle getirir, yoksa insan biçimini kullanır.
fn render<T: serde::Serialize>(
    json: bool,
    value: &T,
    human: impl FnOnce() -> String,
) -> tune_core::Result<String> {
    if json {
        let mut text = serde_json::to_string_pretty(value).map_err(|source| {
            tune_core::Error::new(
                tune_core::diag::Stage::ConfigLoad,
                tune_core::ErrorKind::Json {
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
        tracing_subscriber::EnvFilter::new(format!("tune_core={level},tune_cli={level}"))
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
