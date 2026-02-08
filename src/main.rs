use anyhow::Result;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;

use kty::cli::{Cli, Command, LangSpecs};
use kty::dict::{DGlossary, DGlossaryExtended, DIpa, DIpaMerged, DMain, make_dict};
use kty::download::download_jsonl;
use kty::lang::{Edition, Lang};
use kty::path::PathManager;
use kty::utils::skip_because_file_exists;

fn init_logger(verbose: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if verbose {
            // Only we are set to debug. ureq and other libs stay the same.
            EnvFilter::new(format!("{}=debug", env!("CARGO_PKG_NAME")))
        } else {
            EnvFilter::new("warn")
        }
    });

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
            "%H:%M:%S".to_string(),
        ))
        .init();
}

#[tracing::instrument(skip_all, level = "debug")]
fn run(cmd: Command) -> Result<()> {
    tracing::trace!("{:#?}", cmd);

    match cmd {
        Command::Main(args) => make_dict(DMain, args),
        Command::Glossary(args) => make_dict(DGlossary, args),
        Command::GlossaryExtended(args) => make_dict(DGlossaryExtended, args),
        Command::Ipa(args) => make_dict(DIpa, args),
        Command::IpaMerged(args) => make_dict(DIpaMerged, args),
        Command::Download(args) => {
            let langs: LangSpecs = args.langs.clone().try_into()?;
            let quiet = args.options.quiet;
            let source: Lang = langs.source.try_into().unwrap();
            let edition_lang: Edition = langs.edition.try_into().unwrap();
            let pm = PathManager::try_from(args)?;
            let opath = pm.path_jsonl(edition_lang, source);

            if opath.exists() {
                skip_because_file_exists("download", &opath);
                Ok(())
            } else {
                let _ = std::fs::create_dir(pm.dir_kaik());
                // Should really take the Kind as argument, but this command may disappear anyway
                let kind = kty::dict::edition_to_kind(edition_lang);
                download_jsonl(edition_lang, Some(source), kind, &opath, quiet)
            }
        }
        Command::Iso(args) => {
            if args.edition {
                println!("{}", Lang::help_editions());
            } else {
                println!("{}", Lang::help_isos_coloured());
            }
            Ok(())
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse_cli();
    init_logger(cli.verbose);
    run(cli.command)
}
