use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::{
    cli::{Langs, SimpleArgs},
    lang::{Edition, EditionLang, Lang},
};

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
            Self::GlossaryExtended => write!(f, "glossary-ext"),
            Self::Ipa => write!(f, "ipa"),
            Self::IpaMerged => write!(f, "ipa-merged"),
        }
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
    pub fn dir_kaik(&self) -> PathBuf {
        self.root_dir.join("kaikki")
    }
    /// Directory for all dictionaries.
    ///
    /// Example: `data/dict`
    pub fn dir_dicts(&self) -> PathBuf {
        self.root_dir.join("dict")
    }
    /// Example: `data/dict/el/el`
    fn dir_dict(&self) -> PathBuf {
        self.dir_dicts().join(match self.dict_ty {
            // For merged dictionaries, use the edition (displays as "all")
            // TODO: this should be the opposite
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

    pub fn setup_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(self.dir_kaik())?;
        fs::create_dir_all(self.dir_dict())?;

        if self.save_temps {
            fs::create_dir_all(self.dir_tidy())?; // not needed for glossary
            fs::create_dir_all(self.dir_temp_dict())?;
        }

        Ok(())
    }

    // Only used by CMD::download
    //
    /// Cf. `paths_jsonl` documentation
    pub fn path_jsonl(&self, edition: EditionLang, lang: Lang) -> PathBuf {
        self.aliases(edition, lang).last().unwrap().into()
    }

    /// Cf. `paths_jsonl` documentation
    fn aliases(&self, edition: EditionLang, lang: Lang) -> Vec<PathBuf> {
        match edition {
            EditionLang::En => {
                vec![self.dir_kaik().join(format!("{lang}-en-extract.jsonl"))]
            }
            _ => {
                vec![
                    self.dir_kaik()
                        .join(format!("{lang}-{edition}-extract.jsonl")),
                    self.dir_kaik().join(format!("{edition}-extract.jsonl")),
                ]
            }
        }
    }

    /// Return a vector of edition languages and aliases to the raw jsonl.
    ///
    /// Aliases are done for both legacy reasons (cf. the testsuite), and the differences in
    /// filenames between the English edition and the rest.
    ///
    /// The order of the paths is from specific (alias(es)) to general (download name)
    ///
    /// For example, for the main dictionary:
    /// * If <source> = En, <target> = Zh
    ///   [en-zh-extract.jsonl], actual, name of the (filtered) download
    ///   [en-extract.jsonl],    does not exist
    /// * If <source> = Zh, <target> = En
    ///   [zh-en-extract.jsonl]  alias (specific), used in tests
    ///   [zh-extract.jsonl]     actual (general), name of the (unfiltered) download
    ///
    /// Example (zh-en): [`data/kaikki/zh-en-extract.jsonl`]
    /// Example (en-en): [`data/kaikki/en-en-extract.jsonl`]
    /// Example (en-zh): [`data/kaikki/en-zh-extract.jsonl`, `data/kaikki/zh-extract.jsonl`]
    ///
    /// Note that the English downloads are already filtered. That is:
    /// * [`data/kaikki/zh-en-extract.jsonl`]
    ///   is guaranteed to only have Chinese words with glosses in English
    /// but for:
    /// * [`data/kaikki/en-zh-extract.jsonl`]
    ///   there is no such guarantee (it is an alias, *not* downloaded)
    pub fn paths_jsonl(&self) -> Vec<(EditionLang, Vec<PathBuf>)> {
        let (edition, source, _) = self.langs();

        use DictionaryType::*;
        match self.dict_ty {
            // All editions, other_lang is not used when filtering
            GlossaryExtended | IpaMerged => edition
                .variants()
                .into_iter()
                .map(|edl| (edl, self.aliases(edl, edl.into())))
                .collect(),
            // One edition, other_lang is used when filtering
            Main | Ipa => {
                let edl = edition.try_into().unwrap();
                vec![(edl, self.aliases(edl, source))]
            }
            // One edition, other_lang is not used when filtering
            Glossary => {
                let edl = edition.try_into().unwrap();
                vec![(edl, self.aliases(edl, edl.into()))]
            }
        }
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
    use crate::cli::{
        GlossaryArgs, GlossaryExtendedArgs, GlossaryExtendedLangs, GlossaryLangs, MainArgs,
        MainLangs,
    };

    use super::*;

    macro_rules! exp {
        ($edition:expr, [$($p:expr),*]) => {
            vec![(
                $edition, vec![$(std::path::PathBuf::from($p)),*],
            )]
        };
    }

    type E = Vec<(EditionLang, Vec<PathBuf>)>;

    #[test]
    fn paths_main() {
        fn go(source: Lang, target: EditionLang, expected: E) {
            let args = MainArgs {
                langs: MainLangs {
                    edition: target,
                    source,
                    target,
                },
                ..Default::default()
            };
            let pm = PathManager::new(DictionaryType::Main, &args);
            let paths = pm.paths_jsonl();
            assert_eq!(paths, expected);
        }

        // main zh en
        go(
            Lang::Zh,        // source
            EditionLang::En, // target
            exp!(EditionLang::En, ["kaikki/zh-en-extract.jsonl"]),
        );

        go(
            Lang::En,
            EditionLang::Zh,
            exp!(
                EditionLang::Zh,
                ["kaikki/en-zh-extract.jsonl", "kaikki/zh-extract.jsonl"]
            ),
        );

        go(
            Lang::En,
            EditionLang::En,
            exp!(EditionLang::En, ["kaikki/en-en-extract.jsonl"]),
        );
    }

    #[test]
    fn paths_glossary() {
        fn go(source: EditionLang, target: Lang, expected: E) {
            let args = GlossaryArgs {
                langs: GlossaryLangs {
                    edition: source,
                    source,
                    target,
                },
                ..Default::default()
            };
            let pm = PathManager::new(DictionaryType::Glossary, &args);
            let paths = pm.paths_jsonl();
            assert_eq!(paths, expected);
        }

        go(
            EditionLang::Zh,
            Lang::En,
            exp!(
                EditionLang::Zh,
                ["kaikki/zh-zh-extract.jsonl", "kaikki/zh-extract.jsonl"]
            ),
        );

        go(
            EditionLang::En,
            Lang::Zh,
            exp!(EditionLang::En, ["kaikki/en-en-extract.jsonl"]),
        );
    }

    #[test]
    fn paths_glossary_extended() {
        let args = GlossaryExtendedArgs {
            langs: GlossaryExtendedLangs {
                edition: Edition::All,
                // These two are irrelevant
                source: Lang::Tok,
                target: Lang::Scn,
            },
            ..Default::default()
        };
        let pm = PathManager::new(DictionaryType::GlossaryExtended, &args);
        let paths = pm.paths_jsonl();

        assert!(
            paths.contains(
                exp!(
                    EditionLang::Zh,
                    ["kaikki/zh-zh-extract.jsonl", "kaikki/zh-extract.jsonl"]
                )
                .first()
                .unwrap()
            )
        );
        assert!(
            paths.contains(
                exp!(
                    EditionLang::Nl,
                    ["kaikki/nl-nl-extract.jsonl", "kaikki/nl-extract.jsonl"]
                )
                .first()
                .unwrap()
            )
        );
    }
}
