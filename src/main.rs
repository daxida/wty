use anyhow::{Result, ensure};

use kty::cli::{Cli, Command, DictionaryType, FilterKey, Langs, PathManager, SimpleArgs};
use kty::download::html::download_jsonl;
use kty::lang::{Edition, EditionLang, Lang};
use kty::{DGlossary, DGlossaryExtended, DIpa, DIpaMerged, DMain};
use kty::{make_dict_simple, setup_tracing};

fn push_filter_key_lang(filter: &mut Vec<(FilterKey, String)>, lang: Lang) {
    filter.push((FilterKey::LangCode, lang.to_string()));
}

fn prepare_command(cmd: &mut Command) -> Result<()> {
    match cmd {
        Command::Main(args) => {
            args.langs.edition = args.langs.target;
            push_filter_key_lang(&mut args.options.filter, args.langs.source);
        }
        Command::Glossary(args) => {
            let source_as_lang: Lang = args.langs.source.into();
            ensure!(
                source_as_lang != args.langs.target,
                "in a glossary dictionary source must be different from target."
            );

            args.langs.edition = args.langs.source;
            push_filter_key_lang(&mut args.options.filter, source_as_lang);
        }
        Command::GlossaryExtended(args) => {
            ensure!(
                args.langs.source != args.langs.target,
                "in a glossary dictionary source must be different from target."
            );
        }
        Command::Ipa(args) => {
            args.langs.edition = args.langs.target;
            push_filter_key_lang(&mut args.options.filter, args.langs.source);
        }
        Command::IpaMerged(args) => {
            args.langs.edition = Edition::All;
            args.langs.source = args.langs.target;
            push_filter_key_lang(&mut args.options.filter, args.langs.source);
        }
        Command::Download(args) => {
            args.langs.edition = args.langs.target;
        }
        Command::Iso => (),
    }

    Ok(())
}

fn run_command(cmd: &Command) -> Result<()> {
    match cmd {
        Command::Main(args) => {
            let pm = PathManager::new(DictionaryType::Main, args);
            // make_dict_main(args, &pm)
            make_dict_simple(DMain, args.options(), &pm)
        }
        Command::Glossary(args) => {
            let pm = PathManager::new(DictionaryType::Glossary, args);
            make_dict_simple(DGlossary, args.options(), &pm)
        }
        Command::GlossaryExtended(args) => {
            let pm = PathManager::new(DictionaryType::GlossaryExtended, args);
            make_dict_simple(DGlossaryExtended, &args.options, &pm)
        }
        Command::Ipa(args) => {
            let pm = PathManager::new(DictionaryType::Ipa, args);
            make_dict_simple(DIpa, &args.options, &pm)
        }
        Command::IpaMerged(args) => {
            let pm = PathManager::new(DictionaryType::IpaMerged, args);
            make_dict_simple(DIpaMerged, &args.options, &pm)
        }
        Command::Download(args) => {
            let pm = PathManager::new(DictionaryType::Main, args);
            let langs = args.langs();
            let edition_lang: EditionLang = langs.edition().try_into().unwrap();
            download_jsonl(
                edition_lang,
                langs.source(),
                &pm.path_jsonl_raw(),
                args.options.redownload,
            )
        }
        Command::Iso => {
            println!("{}", Lang::help_supported_isos_coloured());
            Ok(())
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse_cli();

    setup_tracing(cli.verbose);

    let span = tracing::info_span!("main");
    let _guard = span.enter();

    let mut cmd = cli.command;

    // Issue warnings and finish setting args, before debug printing.
    //
    // Done in a separate match for visibility. Everything that mutates args should go here.
    prepare_command(&mut cmd)?;

    tracing::debug!("{:#?}", cmd);

    run_command(&cmd)
}
