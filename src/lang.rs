/// This file was generated and should not be edited directly.
/// The source code can be found at scripts/build.py
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

impl Lang {
    pub const fn is_supported_iso_help_message() -> &'static str {
        "Supported isos: sq | grc | ar | aii | bn | zh | cs | da | nl | en | enm | ang | eo | fi | fr | de | el | afb | he | hi | hu | id | ga | sga | it | ja | kn | kk | km | ku | ko | la | lv | apc | ms | mr | mn | mt | nb | nn | fa | pl | pt | ro | ru | sh | scn | sl | ajp | es | sv | tl | te | th | tr | uk | ur | vi"
    }

    pub const fn has_edition(&self) -> bool {
        matches!(self, Self::Zh | Self::Cs | Self::Nl | Self::En | Self::Fr | Self::De | Self::El | Self::Id | Self::It | Self::Ja | Self::Ku | Self::Ko | Self::Ms | Self::Pl | Self::Pt | Self::Ru | Self::Es | Self::Th | Self::Tr | Self::Vi)
    }

    pub const fn has_edition_help_message() -> &'static str {
        "Valid editions: zh | cs | nl | en | fr | de | el | id | it | ja | ku | ko | ms | pl | pt | ru | es | th | tr | vi"
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
            _ => Err(format!("unsupported iso code '{s}'\n{}", Self::is_supported_iso_help_message())),
        }
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format!("{self:?}").to_lowercase())
    }
}
