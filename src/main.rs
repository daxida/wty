use anyhow::{Result, ensure};

use kty::cli::{
    Cli, Command, DGlossary, DGlossaryExtended, DIpa, DIpaMerged, DictionaryType, FilterKey,
    PathManager,
};
use kty::lang::{Edition, Lang};
use kty::{make_dict_main, make_simple_dict, setup_tracing};

fn push_filter_key_lang(filter: &mut Vec<(FilterKey, String)>, lang: Lang) {
    filter.push((FilterKey::LangCode, lang.to_string()));
}

fn prepare_command(cli: &mut Cli) -> Result<()> {
    match cli.command {
        Command::Main(ref mut args) => {
            if !args.options.save_temps && (args.skip.tidy || args.skip.yomitan) {
                // The code might still work if we had files from a previous run
                tracing::warn!("save_temps is disabled while tidy/yomitan is skipped");
            }

            args.langs.edition = args.langs.target;
            push_filter_key_lang(&mut args.options.filter, args.langs.source);
        }
        Command::Glossary(ref mut args) => {
            let source_as_lang: Lang = args.langs.source.into();
            ensure!(
                source_as_lang != args.langs.target,
                "in a glossary dictionary source must be different from target."
            );

            args.langs.edition = args.langs.source;
            push_filter_key_lang(&mut args.options.filter, source_as_lang);
        }
        Command::GlossaryExtended(ref args) => {
            ensure!(
                args.langs.source != args.langs.target,
                "in a glossary dictionary source must be different from target."
            );
        }
        Command::Ipa(ref mut args) => {
            args.langs.edition = args.langs.target;
            push_filter_key_lang(&mut args.options.filter, args.langs.source);
        }
        Command::IpaMerged(ref mut args) => {
            args.langs.edition = Edition::All;
            args.langs.source = args.langs.target;
            push_filter_key_lang(&mut args.options.filter, args.langs.source);
        }
        Command::Iso => (),
    }

    Ok(())
}

fn main() -> Result<()> {
    let mut cli = Cli::parse_cli();

    setup_tracing(cli.verbose);

    let span = tracing::info_span!("main");
    let _guard = span.enter();

    // Issue warnings and finish setting args, before debug printing.
    //
    // Done in a separate match for visibility. Everything that mutates args should go here.
    prepare_command(&mut cli)?;

    tracing::debug!("{:#?}", cli.command);

    match &cli.command {
        Command::Main(args) => {
            let pm = PathManager::new(DictionaryType::Main, args);
            make_dict_main(args, &pm)
        }
        Command::Glossary(args) => {
            let pm = PathManager::new(DictionaryType::Glossary, args);
            make_simple_dict(DGlossary, &args.options, &pm)
        }
        Command::GlossaryExtended(args) => {
            let pm = PathManager::new(DictionaryType::GlossaryExtended, args);
            make_simple_dict(DGlossaryExtended, &args.options, &pm)
        }
        Command::Ipa(args) => {
            let pm = PathManager::new(DictionaryType::Ipa, args);
            make_simple_dict(DIpa, &args.options, &pm)
        }
        Command::IpaMerged(args) => {
            let pm = PathManager::new(DictionaryType::IpaMerged, args);
            make_simple_dict(DIpaMerged, &args.options, &pm)
        }
        Command::Iso => {
            println!("{}", Lang::help_supported_isos_coloured());
            Ok(())
        }
    }
}
