//! This file was generated and should not be edited directly.
//! The source code can be found at scripts/build.py

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum Lang {
    /// English
    #[default]
    En,
    /// Albanian
    Sq,
    /// Ancient Greek
    Grc,
    /// Arabic
    Ar,
    /// Assyrian Neo-Aramaic
    Aii,
    /// Bengali
    Bn,
    /// Chinese
    Zh,
    /// Czech
    Cs,
    /// Danish
    Da,
    /// Dutch
    Nl,
    /// Middle English
    Enm,
    /// Old English
    Ang,
    /// Esperanto
    Eo,
    /// Finnish
    Fi,
    /// French
    Fr,
    /// German
    De,
    /// Greek
    El,
    /// Gulf Arabic
    Afb,
    /// Hebrew
    He,
    /// Hindi
    Hi,
    /// Hungarian
    Hu,
    /// Indonesian
    Id,
    /// Irish
    Ga,
    /// Old Irish
    Sga,
    /// Italian
    It,
    /// Japanese
    Ja,
    /// Kannada
    Kn,
    /// Kazakh
    Kk,
    /// Khmer
    Km,
    /// Kurdish
    Ku,
    /// Korean
    Ko,
    /// Latin
    La,
    /// Latvian
    Lv,
    /// North Levantine Arabic
    Apc,
    /// Malay
    Ms,
    /// Marathi
    Mr,
    /// Mongolian
    Mn,
    /// Maltese
    Mt,
    /// Norwegian Bokmål
    Nb,
    /// Norwegian Nynorsk
    Nn,
    /// Persian
    Fa,
    /// Polish
    Pl,
    /// Portuguese
    Pt,
    /// Romanian
    Ro,
    /// Russian
    Ru,
    /// Serbo-Croatian
    Sh,
    /// Sicilian
    Scn,
    /// Slovene
    Sl,
    /// South Levantine Arabic
    Ajp,
    /// Spanish
    Es,
    /// Swedish
    Sv,
    /// Tagalog
    Tl,
    /// Telugu
    Te,
    /// Thai
    Th,
    /// Turkish
    Tr,
    /// Ukrainian
    Uk,
    /// Urdu
    Ur,
    /// Vietnamese
    Vi,
}

impl From<EditionLang> for Lang {
    fn from(e: EditionLang) -> Self {
        match e {
            EditionLang::Zh => Self::Zh,
            EditionLang::Cs => Self::Cs,
            EditionLang::Nl => Self::Nl,
            EditionLang::En => Self::En,
            EditionLang::Fr => Self::Fr,
            EditionLang::De => Self::De,
            EditionLang::El => Self::El,
            EditionLang::Id => Self::Id,
            EditionLang::It => Self::It,
            EditionLang::Ja => Self::Ja,
            EditionLang::Ku => Self::Ku,
            EditionLang::Ko => Self::Ko,
            EditionLang::Ms => Self::Ms,
            EditionLang::Pl => Self::Pl,
            EditionLang::Pt => Self::Pt,
            EditionLang::Ru => Self::Ru,
            EditionLang::Es => Self::Es,
            EditionLang::Th => Self::Th,
            EditionLang::Tr => Self::Tr,
            EditionLang::Vi => Self::Vi,
        }
    }
}

impl Lang {
    pub const fn help_supported_isos() -> &'static str {
        "Supported isos: sq | grc | ar | aii | bn | zh | cs | da | nl | en | enm | ang | eo | fi | fr | de | el | afb | he | hi | hu | id | ga | sga | it | ja | kn | kk | km | ku | ko | la | lv | apc | ms | mr | mn | mt | nb | nn | fa | pl | pt | ro | ru | sh | scn | sl | ajp | es | sv | tl | te | th | tr | uk | ur | vi"
    }

    pub const fn help_supported_isos_coloured() -> &'static str {
        "Supported isos: sq | grc | ar | aii | bn | [32mzh[0m | [32mcs[0m | da | [32mnl[0m | [32men[0m | enm | ang | eo | fi | [32mfr[0m | [32mde[0m | [32mel[0m | afb | he | hi | hu | [32mid[0m | ga | sga | [32mit[0m | [32mja[0m | kn | kk | km | [32mku[0m | [32mko[0m | la | lv | apc | [32mms[0m | mr | mn | mt | nb | nn | fa | [32mpl[0m | [32mpt[0m | ro | [32mru[0m | sh | scn | sl | ajp | [32mes[0m | sv | tl | te | [32mth[0m | [32mtr[0m | uk | ur | [32mvi[0m"
    }

    pub const fn help_supported_editions() -> &'static str {
        "Supported editions: zh | cs | nl | en | fr | de | el | id | it | ja | ku | ko | ms | pl | pt | ru | es | th | tr | vi"
    }

    pub const fn long(&self) -> &'static str {
        match self {
            Self::Sq => "Albanian",
            Self::Grc => "Ancient Greek",
            Self::Ar => "Arabic",
            Self::Aii => "Assyrian Neo-Aramaic",
            Self::Bn => "Bengali",
            Self::Zh => "Chinese",
            Self::Cs => "Czech",
            Self::Da => "Danish",
            Self::Nl => "Dutch",
            Self::En => "English",
            Self::Enm => "Middle English",
            Self::Ang => "Old English",
            Self::Eo => "Esperanto",
            Self::Fi => "Finnish",
            Self::Fr => "French",
            Self::De => "German",
            Self::El => "Greek",
            Self::Afb => "Gulf Arabic",
            Self::He => "Hebrew",
            Self::Hi => "Hindi",
            Self::Hu => "Hungarian",
            Self::Id => "Indonesian",
            Self::Ga => "Irish",
            Self::Sga => "Old Irish",
            Self::It => "Italian",
            Self::Ja => "Japanese",
            Self::Kn => "Kannada",
            Self::Kk => "Kazakh",
            Self::Km => "Khmer",
            Self::Ku => "Kurdish",
            Self::Ko => "Korean",
            Self::La => "Latin",
            Self::Lv => "Latvian",
            Self::Apc => "North Levantine Arabic",
            Self::Ms => "Malay",
            Self::Mr => "Marathi",
            Self::Mn => "Mongolian",
            Self::Mt => "Maltese",
            Self::Nb => "Norwegian Bokmål",
            Self::Nn => "Norwegian Nynorsk",
            Self::Fa => "Persian",
            Self::Pl => "Polish",
            Self::Pt => "Portuguese",
            Self::Ro => "Romanian",
            Self::Ru => "Russian",
            Self::Sh => "Serbo-Croatian",
            Self::Scn => "Sicilian",
            Self::Sl => "Slovene",
            Self::Ajp => "South Levantine Arabic",
            Self::Es => "Spanish",
            Self::Sv => "Swedish",
            Self::Tl => "Tagalog",
            Self::Te => "Telugu",
            Self::Th => "Thai",
            Self::Tr => "Turkish",
            Self::Uk => "Ukrainian",
            Self::Ur => "Urdu",
            Self::Vi => "Vietnamese",
        }
    }
}

impl std::str::FromStr for Lang {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sq" => Ok(Self::Sq),
            "grc" => Ok(Self::Grc),
            "ar" => Ok(Self::Ar),
            "aii" => Ok(Self::Aii),
            "bn" => Ok(Self::Bn),
            "zh" => Ok(Self::Zh),
            "cs" => Ok(Self::Cs),
            "da" => Ok(Self::Da),
            "nl" => Ok(Self::Nl),
            "en" => Ok(Self::En),
            "enm" => Ok(Self::Enm),
            "ang" => Ok(Self::Ang),
            "eo" => Ok(Self::Eo),
            "fi" => Ok(Self::Fi),
            "fr" => Ok(Self::Fr),
            "de" => Ok(Self::De),
            "el" => Ok(Self::El),
            "afb" => Ok(Self::Afb),
            "he" => Ok(Self::He),
            "hi" => Ok(Self::Hi),
            "hu" => Ok(Self::Hu),
            "id" => Ok(Self::Id),
            "ga" => Ok(Self::Ga),
            "sga" => Ok(Self::Sga),
            "it" => Ok(Self::It),
            "ja" => Ok(Self::Ja),
            "kn" => Ok(Self::Kn),
            "kk" => Ok(Self::Kk),
            "km" => Ok(Self::Km),
            "ku" => Ok(Self::Ku),
            "ko" => Ok(Self::Ko),
            "la" => Ok(Self::La),
            "lv" => Ok(Self::Lv),
            "apc" => Ok(Self::Apc),
            "ms" => Ok(Self::Ms),
            "mr" => Ok(Self::Mr),
            "mn" => Ok(Self::Mn),
            "mt" => Ok(Self::Mt),
            "nb" => Ok(Self::Nb),
            "nn" => Ok(Self::Nn),
            "fa" => Ok(Self::Fa),
            "pl" => Ok(Self::Pl),
            "pt" => Ok(Self::Pt),
            "ro" => Ok(Self::Ro),
            "ru" => Ok(Self::Ru),
            "sh" => Ok(Self::Sh),
            "scn" => Ok(Self::Scn),
            "sl" => Ok(Self::Sl),
            "ajp" => Ok(Self::Ajp),
            "es" => Ok(Self::Es),
            "sv" => Ok(Self::Sv),
            "tl" => Ok(Self::Tl),
            "te" => Ok(Self::Te),
            "th" => Ok(Self::Th),
            "tr" => Ok(Self::Tr),
            "uk" => Ok(Self::Uk),
            "ur" => Ok(Self::Ur),
            "vi" => Ok(Self::Vi),
            _ => Err(format!("unsupported iso code '{s}'\n{}", Self::help_supported_isos())),
        }
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let debug_str = format!("{self:?}");
        write!(f, "{}", debug_str.to_lowercase())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Edition {
    /// All editions
    All,
    /// An `EditionLang`
    EditionLang(EditionLang),
}

impl Edition {
    pub fn variants(&self) -> Vec<EditionLang> {
        match self {
            Self::All => vec![
                EditionLang::Zh,
                EditionLang::Cs,
                EditionLang::Nl,
                EditionLang::En,
                EditionLang::Fr,
                EditionLang::De,
                EditionLang::El,
                EditionLang::Id,
                EditionLang::It,
                EditionLang::Ja,
                EditionLang::Ku,
                EditionLang::Ko,
                EditionLang::Ms,
                EditionLang::Pl,
                EditionLang::Pt,
                EditionLang::Ru,
                EditionLang::Es,
                EditionLang::Th,
                EditionLang::Tr,
                EditionLang::Vi,
            ],
            Self::EditionLang(lang) => vec![*lang],
        }
    }
}

impl Default for Edition {
    fn default() -> Self {
        Self::EditionLang(EditionLang::default())
    }
}

impl std::str::FromStr for Edition {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "all" => Ok(Self::All),
            other => Ok(Self::EditionLang(other.parse::<EditionLang>()?)),
        }
    }
}

impl std::fmt::Display for Edition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "all"),
            Self::EditionLang(lang) => write!(f, "{lang}"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum EditionLang {
    /// English
    #[default]
    En,
    /// Chinese
    Zh,
    /// Czech
    Cs,
    /// Dutch
    Nl,
    /// French
    Fr,
    /// German
    De,
    /// Greek
    El,
    /// Indonesian
    Id,
    /// Italian
    It,
    /// Japanese
    Ja,
    /// Kurdish
    Ku,
    /// Korean
    Ko,
    /// Malay
    Ms,
    /// Polish
    Pl,
    /// Portuguese
    Pt,
    /// Russian
    Ru,
    /// Spanish
    Es,
    /// Thai
    Th,
    /// Turkish
    Tr,
    /// Vietnamese
    Vi,
}

impl std::convert::TryFrom<Lang> for EditionLang {
    type Error = &'static str;

    fn try_from(lang: Lang) -> Result<Self, Self::Error> {
        match lang {
            Lang::Zh => Ok(Self::Zh),
            Lang::Cs => Ok(Self::Cs),
            Lang::Nl => Ok(Self::Nl),
            Lang::En => Ok(Self::En),
            Lang::Fr => Ok(Self::Fr),
            Lang::De => Ok(Self::De),
            Lang::El => Ok(Self::El),
            Lang::Id => Ok(Self::Id),
            Lang::It => Ok(Self::It),
            Lang::Ja => Ok(Self::Ja),
            Lang::Ku => Ok(Self::Ku),
            Lang::Ko => Ok(Self::Ko),
            Lang::Ms => Ok(Self::Ms),
            Lang::Pl => Ok(Self::Pl),
            Lang::Pt => Ok(Self::Pt),
            Lang::Ru => Ok(Self::Ru),
            Lang::Es => Ok(Self::Es),
            Lang::Th => Ok(Self::Th),
            Lang::Tr => Ok(Self::Tr),
            Lang::Vi => Ok(Self::Vi),
            _ => Err("language has no edition"),
        }
    }
}

impl std::convert::TryFrom<Edition> for EditionLang {
    type Error = &'static str;

    fn try_from(edition: Edition) -> Result<Self, Self::Error> {
        match edition {
            Edition::EditionLang(lang) => Ok(lang),
            Edition::All => Err("cannot convert Edition::All to EditionLang"),
        }
    }
}

impl std::str::FromStr for EditionLang {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "zh" => Ok(Self::Zh),
            "cs" => Ok(Self::Cs),
            "nl" => Ok(Self::Nl),
            "en" => Ok(Self::En),
            "fr" => Ok(Self::Fr),
            "de" => Ok(Self::De),
            "el" => Ok(Self::El),
            "id" => Ok(Self::Id),
            "it" => Ok(Self::It),
            "ja" => Ok(Self::Ja),
            "ku" => Ok(Self::Ku),
            "ko" => Ok(Self::Ko),
            "ms" => Ok(Self::Ms),
            "pl" => Ok(Self::Pl),
            "pt" => Ok(Self::Pt),
            "ru" => Ok(Self::Ru),
            "es" => Ok(Self::Es),
            "th" => Ok(Self::Th),
            "tr" => Ok(Self::Tr),
            "vi" => Ok(Self::Vi),
            _ => Err(format!("invalid edition '{s}'")),
        }
    }
}

impl std::fmt::Display for EditionLang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let debug_str = format!("{:?}", Lang::from(*self));
        write!(f, "{}", debug_str.to_lowercase())
    }
}
