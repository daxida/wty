use anyhow::{Ok, Result, bail};
use clap::{Parser, Subcommand};
use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::lang::Edition;
use crate::lang::{EditionLang, Lang};
use crate::models::WordEntry;

#[derive(Debug, Parser)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    // NOTE: the order in which this --verbose flag appears in subcommands help seems cursed.
    //
    /// Verbose output
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

    // contains these extra skip parameters
    #[command(flatten)]
    pub skip: ArgsSkip,
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

    /// Target language
    pub target: EditionLang,
}

/// Langs-like struct that validates edition for `source` and skips `edition`.
#[derive(Parser, Debug, Default)]
pub struct GlossaryLangs {
    /// Edition language
    #[arg(skip)]
    pub edition: EditionLang,

    /// Source language
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

    /// Only keep the first n jsonlines before filtering. -1 keeps all
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

    /// Write jsons with whitespace
    #[arg(short, long)]
    pub pretty: bool,

    /// Include experimental features
    #[arg(short, long)]
    pub experimental: bool,

    /// Change the root directory
    #[arg(long, default_value = "data")]
    pub root_dir: PathBuf,
}

/// Skip arguments. Only relevant for the main dictionary.
#[derive(Parser, Debug, Default)]
pub struct ArgsSkip {
    /// Skip filtering the jsonl
    #[arg(long = "skip-filtering", help_heading = "Skip")]
    pub filtering: bool,

    /// Skip running tidy (IR generation)
    #[arg(long = "skip-tidy", help_heading = "Skip")]
    pub tidy: bool,

    /// Skip running yomitan (mainly for testing)
    #[arg(long = "skip-yomitan", help_heading = "Skip")]
    pub yomitan: bool,
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

impl Cli {
    pub fn parse_cli() -> Self {
        Self::parse()
    }
}

impl ArgsOptions {
    // TODO: this won't work for GlossaryExtended. Although it makes little sense there...
    //
    /// Check if there are any (extra) filter parameters.
    ///
    /// It depends on the dictionary type, since some dictionaries may add the LangCode filter to
    /// self.filter at main init.
    pub const fn has_filter_params(&self) -> bool {
        (self.filter.len() > 1) || !self.reject.is_empty() || self.first != -1
    }
}

// Empty structs to implement the SimpleDictionary trait on.
#[derive(Debug, Clone, Copy)]
pub struct DGlossary;

#[derive(Debug, Clone, Copy)]
pub struct DGlossaryExtended;

#[derive(Debug, Clone, Copy)]
pub struct DIpa;

#[derive(Debug, Clone, Copy)]
pub struct DIpaMerged;

/// Enum used by `PathManager` to dispatch filetree operations (folder names etc.)
#[derive(Debug, Clone, Copy)]
pub enum DictionaryType {
    Main,
    Glossary,
    GlossaryExtended,
    Ipa,
    IpaMerged,
}

/// Used only for the temporary files folder (`dir_temp`).
impl fmt::Display for DictionaryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main => write!(f, "main"),
            Self::Glossary => write!(f, "glossary"),
            Self::GlossaryExtended => write!(f, "glossary-ext"), // should be just glossary
            Self::Ipa => write!(f, "ipa"),
            Self::IpaMerged => write!(f, "ipa-merged"),
        }
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

impl SimpleArgs for MainArgs {
    fn dict_name(&self) -> &str {
        &self.dict_name
    }
    fn langs(&self) -> &impl Langs {
        &self.langs
    }
    fn options(&self) -> &ArgsOptions {
        &self.options
    }
}

impl SimpleArgs for GlossaryArgs {
    fn dict_name(&self) -> &str {
        &self.dict_name
    }
    fn langs(&self) -> &impl Langs {
        &self.langs
    }
    fn options(&self) -> &ArgsOptions {
        &self.options
    }
}

impl SimpleArgs for GlossaryExtendedArgs {
    fn dict_name(&self) -> &str {
        &self.dict_name
    }
    fn langs(&self) -> &impl Langs {
        &self.langs
    }
    fn options(&self) -> &ArgsOptions {
        &self.options
    }
}

impl SimpleArgs for IpaArgs {
    fn dict_name(&self) -> &str {
        &self.dict_name
    }
    fn langs(&self) -> &impl Langs {
        &self.langs
    }
    fn options(&self) -> &ArgsOptions {
        &self.options
    }
}

impl SimpleArgs for IpaMergedArgs {
    fn dict_name(&self) -> &str {
        &self.dict_name
    }
    fn langs(&self) -> &impl Langs {
        &self.langs
    }
    fn options(&self) -> &ArgsOptions {
        &self.options
    }
}

/// Helper struct to manage paths.
//
// It could have done directly with args, but tracking dict_ty is quite tricky. Also, this makes
// the intent of every call to either args or pm (PathManager) clearer. And better autocomplete!
#[derive(Debug)]
pub struct PathManager {
    dict_name: String,
    dict_ty: DictionaryType,

    edition: Edition,
    source: Lang,
    target: Lang,

    root_dir: PathBuf,
    save_temps: bool,
    experimental: bool,
}

impl PathManager {
    pub fn new(dict_ty: DictionaryType, args: &impl SimpleArgs) -> Self {
        Self {
            dict_name: args.dict_name().to_string(),
            dict_ty,
            edition: args.langs().edition(),
            source: args.langs().source(),
            target: args.langs().target(),
            root_dir: args.options().root_dir.clone(),
            save_temps: args.options().save_temps,
            experimental: args.options().experimental,
        }
    }

    // Seems a bit hacky to get it from the PathManager...
    pub const fn langs(&self) -> (Edition, Lang, Lang) {
        (self.edition, self.source, self.target)
    }

    /// Example: `data/kaikki`
    fn dir_kaik(&self) -> PathBuf {
        self.root_dir.join("kaikki")
    }
    /// Example: `data/dict/el/el`
    fn dir_dict(&self) -> PathBuf {
        self.root_dir.join("dict").join(match self.dict_ty {
            // For merged dictionaries, use the edition (displays as "all")
            DictionaryType::IpaMerged => format!("{}/{}", self.target, self.edition),
            _ => format!("{}/{}", self.source, self.target),
        })
    }
    /// Depends on the type of dictionary being made.
    ///
    /// Example: `data/dict/el/el/temp-main`
    /// Example: `data/dict/el/el/temp-glossary`
    fn dir_temp(&self) -> PathBuf {
        // Maybe remove the "temp-" altogether?
        self.dir_dict().join(format!("temp-{}", self.dict_ty))
    }
    /// Example: `data/dict/el/el/temp/tidy`
    pub fn dir_tidy(&self) -> PathBuf {
        self.dir_temp().join("tidy")
    }

    pub fn setup_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.dir_kaik())?;
        fs::create_dir_all(self.dir_dict())?;

        if self.save_temps {
            fs::create_dir_all(self.dir_tidy())?; // not needed for glossary
            fs::create_dir_all(self.dir_temp_dict())?;
        }

        Ok(())
    }

    /// Different in English and non-English editions. The English download is already filtered.
    ///
    /// Example (el):    `data/kaikki/el-extract.jsonl`
    /// Example (en-en): `data/kaikki/en-en-extract.jsonl`
    /// Example (de-en): `data/kaikki/de-en-extract.jsonl`
    pub fn path_jsonl_raw(&self) -> PathBuf {
        self.dir_kaik().join(match self.edition {
            Edition::All => panic!(), // this can't happen in DictionaryType::Main
            Edition::EditionLang(EditionLang::En) => {
                format!("{}-{}-extract.jsonl", self.source, self.target)
            }
            Edition::EditionLang(_) => format!("{}-extract.jsonl", self.edition),
        })
    }

    /// `data/kaikki/source-target.jsonl`
    ///
    /// Source and target are passed as arguments because some dictionaries may require a different
    /// combination in their input. F.e., the el-en glossary is made out of el-el-extract.jsonl
    ///
    /// Example (en-el): `data/kaikki/en-el-extract.jsonl`
    pub fn path_jsonl(&self, source: Lang, target: Lang) -> PathBuf {
        self.dir_kaik()
            .join(format!("{source}-{target}-extract.jsonl"))
    }

    /// `data/dict/source/target/temp/tidy/source-target-lemmas.json`
    ///
    /// Example: `data/dict/el/el/temp/tidy/el-el-lemmas.json`
    pub fn path_lemmas(&self) -> PathBuf {
        self.dir_tidy()
            .join(format!("{}-{}-lemmas.json", self.source, self.target))
    }

    /// `data/dict/source/target/temp/tidy/source-target-forms.json`
    ///
    /// Example: `data/dict/el/el/temp/tidy/el-el-forms.json`
    pub fn path_forms(&self) -> PathBuf {
        self.dir_tidy()
            .join(format!("{}-{}-forms.json", self.source, self.target))
    }

    /// Temporary working directory path used before zipping the dictionary.
    ///
    /// Example: `data/dict/el/el/temp/dict`
    pub fn dir_temp_dict(&self) -> PathBuf {
        self.dir_temp().join("dict")
    }

    // Should not go here, but since it uses dict_ty...
    // It exists so the dictionary index is in sync with PathManager::path_dict
    //
    /// Depends on the dictionary type (main, glossary etc.)
    ///
    /// Example: `dictionary_name-el-en`
    /// Example: `dictionary_name-el-en-gloss`
    pub fn dict_name_expanded(&self) -> String {
        let mut expanded = match self.dict_ty {
            DictionaryType::Main => format!("{}-{}-{}", self.dict_name, self.source, self.target),
            DictionaryType::Glossary => {
                format!("{}-{}-{}-gloss", self.dict_name, self.source, self.target)
            }
            DictionaryType::GlossaryExtended => {
                format!(
                    "{}-{}-{}-{}-gloss",
                    self.dict_name, self.edition, self.source, self.target
                )
            }
            DictionaryType::Ipa => {
                format!("{}-{}-{}-ipa", self.dict_name, self.source, self.target)
            }
            DictionaryType::IpaMerged => format!("{}-{}-ipa", self.dict_name, self.target),
        };

        if self.experimental {
            expanded.push_str("-exp");
        }

        expanded
    }

    /// Depends on the dictionary type (main, glossary etc.)
    ///
    /// Example: `data/dict/el/en/dictionary_name-el-en.zip`
    /// Example: `data/dict/el/en/dictionary_name-el-en-gloss.zip`
    pub fn path_dict(&self) -> PathBuf {
        self.dir_dict()
            .join(format!("{}.zip", self.dict_name_expanded()))
    }

    /// Example: `data/dict/el/el/temp/diagnostics`
    pub fn dir_diagnostics(&self) -> PathBuf {
        self.dir_temp().join("diagnostics")
    }
}

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

    // #[test]
    // fn glossary_needs_source_edition() {
    //     assert!(Cli::try_parse_from(["kty", "glossary", "grc", "el"]).is_err());
    //     assert!(Cli::try_parse_from(["kty", "glossary", "el", "grc"]).is_ok());
    // }
    //
    // #[test]
    // fn glossary_can_not_be_monolingual() {
    //     assert!(Cli::try_parse_from(["kty", "glossary", "el", "el"]).is_err());
    // }

    #[test]
    fn filter_flag() {
        assert!(MainArgs::try_parse_from(["_pname", "el", "el", "--filter", "foo,bar"]).is_err());
        assert!(MainArgs::try_parse_from(["_pname", "el", "el", "--filter", "word,hello"]).is_ok());
        assert!(MainArgs::try_parse_from(["_pname", "el", "el", "--reject", "pos,name"]).is_ok());
    }
}
