use anyhow::Result;

use kty::cli::{Cli, Command, Langs, SimpleArgs};
use kty::dict::{DGlossary, DGlossaryExtended, DIpa, DIpaMerged, DMain};
use kty::download::download_jsonl;
use kty::lang::{EditionLang, Lang};
use kty::path::{DictionaryType, PathManager};
use kty::utils::skip_because_file_exists;
use kty::{make_dict, setup_tracing};

fn run_command(cmd: &Command) -> Result<()> {
    match cmd {
        Command::Main(args) => {
            let pm = PathManager::new(DictionaryType::Main, args);
            // make_dict_main(args, &pm)
            make_dict(DMain, args.options(), &pm)
        }
        Command::Glossary(args) => {
            let pm = PathManager::new(DictionaryType::Glossary, args);
            make_dict(DGlossary, args.options(), &pm)
        }
        Command::GlossaryExtended(args) => {
            let pm = PathManager::new(DictionaryType::GlossaryExtended, args);
            make_dict(DGlossaryExtended, &args.options, &pm)
        }
        Command::Ipa(args) => {
            let pm = PathManager::new(DictionaryType::Ipa, args);
            make_dict(DIpa, &args.options, &pm)
        }
        Command::IpaMerged(args) => {
            let pm = PathManager::new(DictionaryType::IpaMerged, args);
            make_dict(DIpaMerged, &args.options, &pm)
        }
        Command::Download(args) => {
            let pm = PathManager::new(DictionaryType::Main, args);
            let langs = args.langs();
            let source = langs.source();
            let edition_lang: EditionLang = langs.edition().try_into().unwrap();
            let opath = pm.path_jsonl_raw(edition_lang, source);

            if opath.exists() {
                skip_because_file_exists("download", &opath);
                Ok(())
            } else {
                let _ = std::fs::create_dir(pm.dir_kaik());
                download_jsonl(edition_lang, source, &opath, args.options.quiet)
            }
        }
        Command::Iso(args) => {
            if args.edition {
                println!("{}", Lang::help_supported_editions());
            } else {
                println!("{}", Lang::help_supported_isos_coloured());
            }
            Ok(())
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse_cli()?;

    setup_tracing(cli.verbose);
    let span = tracing::info_span!("main");
    let _guard = span.enter();

    let cmd = cli.command;
    tracing::debug!("{:#?}", cmd);
    run_command(&cmd)
}
