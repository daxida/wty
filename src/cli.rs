use anyhow::{Ok, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::lang::Edition;
use crate::lang::{EditionLang, Lang};
use crate::models::kaikki::WordEntry;

#[derive(Debug, Parser)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    // NOTE: the order in which this --verbose flag appears in subcommands help seems cursed.
    //
    /// Verbose output (set logging level to DEBUG)
    #[arg(long, short, global = true)]
    pub verbose: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Main dictionary. Uses target for the edition
    Main(MainArgs),

    /// Short dictionary made from translations. Uses source for the edition
    Glossary(GlossaryArgs),

    /// Short dictionary made from translations. Supports any language pair
    GlossaryExtended(GlossaryExtendedArgs),

    /// Phonetic transcription dictionary. Uses target for the edition
    Ipa(IpaArgs),

    /// Phonetic transcription dictionary. Uses all editions
    IpaMerged(IpaMergedArgs),

    /// Download a Kaikki jsonline
    Download(MainArgs),

    /// Show supported iso codes, with coloured editions
    Iso,
}

#[derive(Parser, Debug, Default)]
pub struct MainArgs {
    #[command(flatten)]
    pub langs: MainLangs,

    /// Dictionary name
    #[arg(default_value = "kty")]
    pub dict_name: String,

    #[command(flatten)]
    pub options: ArgsOptions,
}

#[derive(Parser, Debug, Default)]
pub struct GlossaryArgs {
    #[command(flatten)]
    pub langs: GlossaryLangs,

    /// Dictionary name
    #[arg(default_value = "kty")]
    pub dict_name: String,

    #[command(flatten)]
    pub options: ArgsOptions,
}

#[derive(Parser, Debug, Default)]
pub struct GlossaryExtendedArgs {
    #[command(flatten)]
    pub langs: GlossaryExtendedLangs,

    /// Dictionary name
    #[arg(default_value = "kty")]
    pub dict_name: String,

    #[command(flatten)]
    pub options: ArgsOptions,
}

#[derive(Parser, Debug, Default)]
pub struct IpaArgs {
    #[command(flatten)]
    pub langs: MainLangs,

    /// Dictionary name
    #[arg(default_value = "kty")]
    pub dict_name: String,

    #[command(flatten)]
    pub options: ArgsOptions,
}

#[derive(Parser, Debug, Default)]
pub struct IpaMergedArgs {
    #[command(flatten)]
    pub langs: IpaMergedLangs,

    /// Dictionary name
    #[arg(default_value = "kty")]
    pub dict_name: String,

    #[command(flatten)]
    pub options: ArgsOptions,
}

/// Langs-like struct that validates edition for `target` and skips `edition`.
#[derive(Parser, Debug, Default)]
pub struct MainLangs {
    /// Edition language
    #[arg(skip)]
    pub edition: EditionLang,

    /// Source language
    pub source: Lang,

    /// Target language (edition)
    pub target: EditionLang,
}

/// Langs-like struct that validates edition for `source` and skips `edition`.
#[derive(Parser, Debug, Default)]
pub struct GlossaryLangs {
    /// Edition language
    #[arg(skip)]
    pub edition: EditionLang,

    /// Source language (edition)
    pub source: EditionLang,

    /// Target language
    pub target: Lang,
}

/// Langs-like struct that validates edition for `edition`.
#[derive(Parser, Debug, Default)]
pub struct GlossaryExtendedLangs {
    /// Edition language
    pub edition: Edition,

    /// Source language
    pub source: Lang,

    /// Target language
    pub target: Lang,
}

/// Langs-like struct that only takes one language.
#[derive(Parser, Debug, Default)]
pub struct IpaMergedLangs {
    /// Edition language
    #[arg(skip)]
    pub edition: Edition,

    /// Source language
    #[arg(skip)]
    pub source: Lang,

    /// Target language
    pub target: Lang,
}

#[expect(clippy::struct_excessive_bools)]
#[derive(Parser, Debug, Default)]
pub struct ArgsOptions {
    // In the main dictionary, the filter file is always writen to disk, regardless of this.
    //
    // If save_temps is true, we assume that the user is debugging and does not need the zip.
    //
    /// Write temporary files to disk and skip zipping
    #[arg(long, short)]
    pub save_temps: bool,

    /// Redownload kaikki files
    #[arg(long, short)]
    pub redownload: bool,

    /// Only keep the first n filtered lines. -1 keeps all
    #[arg(long, default_value_t = -1)]
    pub first: i32,

    // This filtering is done at filter_jsonl
    //
    // Example:
    //   `--filter pos,adv`
    //
    // You can specify this option multiple times:
    //   `--filter pos,adv --filter word,foo`
    //
    /// Only keep entries matching certain key–value filters
    #[arg(long, value_parser = parse_tuple)]
    pub filter: Vec<(FilterKey, String)>,

    // This filtering is done at filter_jsonl
    //
    // Example:
    //   `--reject pos,adj`
    //
    // You can specify this option multiple times:
    //   `--reject pos,adj --reject word,foo`
    //
    /// Only keep entries not matching certain key–value filters
    #[arg(long, value_parser = parse_tuple)]
    pub reject: Vec<(FilterKey, String)>,

    /// Replace the jsonl with the filtered lines
    #[arg(long)]
    pub cache_filter: bool,

    /// Do not print anything to the console
    #[arg(long, short)]
    pub quiet: bool,

    /// Write jsons with whitespace
    #[arg(short, long)]
    pub pretty: bool,

    /// Skip converting to yomitan (to speed up testing)
    #[arg(long)]
    pub skip_yomitan: bool,

    /// Include experimental features
    #[arg(short, long)]
    pub experimental: bool,

    /// Change the root directory
    #[arg(long, default_value = "data")]
    pub root_dir: PathBuf,
}

fn parse_tuple(s: &str) -> Result<(FilterKey, String), String> {
    let parts: Vec<_> = s.split(',').map(|x| x.trim().to_string()).collect();
    if parts.len() != 2 {
        return Err("expected two comma-separated values".into());
    }
    let filter_key = FilterKey::try_from(parts[0].as_str()).map_err(|e| e.to_string())?;
    core::result::Result::Ok((filter_key, parts[1].clone()))
}

#[derive(Debug, Clone)]
pub enum FilterKey {
    LangCode,
    Word,
    Pos,
}

impl FilterKey {
    pub fn field_value<'a>(&self, entry: &'a WordEntry) -> &'a str {
        match self {
            Self::LangCode => &entry.lang_code,
            Self::Word => &entry.word,
            Self::Pos => &entry.pos,
        }
    }

    fn try_from(s: &str) -> Result<Self> {
        match s {
            "lang_code" => Ok(Self::LangCode),
            "word" => Ok(Self::Word),
            "pos" => Ok(Self::Pos),
            other => bail!("unknown filter key '{other}'. Choose between: lang_code | word | pos",),
        }
    }
}

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
            anyhow::ensure!(
                source_as_lang != args.langs.target,
                "in a glossary dictionary source must be different from target."
            );

            args.langs.edition = args.langs.source;
            push_filter_key_lang(&mut args.options.filter, source_as_lang);
        }
        Command::GlossaryExtended(args) => {
            anyhow::ensure!(
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

impl Cli {
    pub fn parse_cli() -> Result<Self> {
        let mut cli = Self::parse();
        prepare_command(&mut cli.command)?;
        Ok(cli)
    }
}

/// Helper trait to support CLI edition validation, while treating them as equal later on.
pub trait Langs {
    fn edition(&self) -> Edition;
    fn source(&self) -> Lang;
    fn target(&self) -> Lang;
}

impl Langs for MainLangs {
    fn edition(&self) -> Edition {
        Edition::EditionLang(self.edition)
    }
    fn source(&self) -> Lang {
        self.source
    }
    fn target(&self) -> Lang {
        self.target.into()
    }
}

impl Langs for GlossaryLangs {
    fn edition(&self) -> Edition {
        Edition::EditionLang(self.edition)
    }
    fn source(&self) -> Lang {
        self.source.into()
    }
    fn target(&self) -> Lang {
        self.target
    }
}

impl Langs for GlossaryExtendedLangs {
    fn edition(&self) -> Edition {
        self.edition
    }
    fn source(&self) -> Lang {
        self.source
    }
    fn target(&self) -> Lang {
        self.target
    }
}

// IpaLangs reuses MainLangs

impl Langs for IpaMergedLangs {
    fn edition(&self) -> Edition {
        self.edition
    }
    fn source(&self) -> Lang {
        self.source
    }
    fn target(&self) -> Lang {
        self.target
    }
}

pub trait SimpleArgs {
    fn dict_name(&self) -> &str;
    fn langs(&self) -> &impl Langs;
    fn options(&self) -> &ArgsOptions;
}

/// Implement the SimpleArgs trait.
macro_rules! simple_args {
    ($($ty:ty),* $(,)?) => {
        $( impl SimpleArgs for $ty {
            fn dict_name(&self) -> &str { &self.dict_name }
            fn langs(&self) -> &impl Langs { &self.langs }
            fn options(&self) -> &ArgsOptions { &self.options }
        } )*
    };
}

simple_args!(MainArgs);
simple_args!(GlossaryArgs);
simple_args!(GlossaryExtendedArgs);
simple_args!(IpaArgs);
simple_args!(IpaMergedArgs);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_commands() {
        assert!(Cli::try_parse_from(["kty", "main", "el", "en"]).is_ok());
        assert!(Cli::try_parse_from(["kty", "glossary", "el", "en"]).is_ok());
    }

    #[test]
    fn main_needs_target_edition() {
        assert!(Cli::try_parse_from(["kty", "main", "grc", "el"]).is_ok());
        assert!(Cli::try_parse_from(["kty", "main", "el", "grc"]).is_err());
    }

    #[test]
    fn glossary_needs_source_edition() {
        assert!(Cli::try_parse_from(["kty", "glossary", "grc", "el"]).is_err());
        assert!(Cli::try_parse_from(["kty", "glossary", "el", "grc"]).is_ok());
    }

    #[test]
    fn glossary_can_not_be_monolingual() {
        let res = Cli::try_parse_from(["kty", "glossary", "el", "el"]);
        let mut cli = res.unwrap(); // The parsing should be ok
        assert!(prepare_command(&mut cli.command).is_err())
    }

    #[test]
    fn filter_flag() {
        assert!(MainArgs::try_parse_from(["_pname", "el", "el", "--filter", "foo,bar"]).is_err());
        assert!(MainArgs::try_parse_from(["_pname", "el", "el", "--filter", "word,hello"]).is_ok());
        assert!(MainArgs::try_parse_from(["_pname", "el", "el", "--reject", "pos,name"]).is_ok());
    }
}
