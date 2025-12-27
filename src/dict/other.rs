use crate::{
    Map, Set,
    cli::Options,
    dict::{Diagnostics, Dictionary, LabelledYomitanEntry, get_ipas, get_reading},
    lang::{EditionLang, Lang},
    models::{
        kaikki::WordEntry,
        yomitan::{
            DetailedDefinition, NTag, Node, PhoneticTranscription, TermBank, TermBankMeta,
            TermPhoneticTranscription, YomitanEntry, wrap,
        },
    },
    tags::find_short_pos,
};

#[derive(Debug, Clone, Copy)]
pub struct DGlossary;

#[derive(Debug, Clone, Copy)]
pub struct DGlossaryExtended;

#[derive(Debug, Clone, Copy)]
pub struct DIpa;

#[derive(Debug, Clone, Copy)]
pub struct DIpaMerged;

impl Dictionary for DGlossary {
    type I = Vec<YomitanEntry>;

    fn process(
        &self,
        edition: EditionLang,
        _source: Lang,
        target: Lang,
        entry: &WordEntry,
        irs: &mut Self::I,
    ) {
        process_glossary(edition, target, entry, irs);
    }

    fn to_yomitan(
        &self,
        _edition: EditionLang,
        _source: Lang,
        _target: Lang,
        _options: &Options,
        _diagnostics: &mut Diagnostics,
        irs: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![("term", irs)]
    }
}

impl Dictionary for DGlossaryExtended {
    type I = Vec<IGlossaryExtended>;

    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        target: Lang,
        entry: &WordEntry,
        irs: &mut Self::I,
    ) {
        process_glossary_extended(edition, source, target, entry, irs);
    }

    fn postprocess(&self, irs: &mut Self::I) {
        let mut map = Map::default();

        for (lemma, pos, edition, translations) in irs.drain(..) {
            map.entry(lemma)
                .or_insert_with(|| (pos, edition, Set::default()))
                .2
                .extend(translations);
        }

        irs.extend(map.into_iter().map(|(lemma, (pos, edition, set))| {
            (lemma, pos, edition, set.into_iter().collect::<Vec<_>>())
        }));
    }

    fn to_yomitan(
        &self,
        _edition: EditionLang,
        _source: Lang,
        _target: Lang,
        _options: &Options,
        _diagnostics: &mut Diagnostics,
        irs: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![("term", to_yomitan_glossary_extended(irs))]
    }
}

impl Dictionary for DIpa {
    type I = Vec<IIpa>;

    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        _target: Lang,
        entry: &WordEntry,
        irs: &mut Self::I,
    ) {
        process_ipa(edition, source, entry, irs);
    }

    fn to_yomitan(
        &self,
        _edition: EditionLang,
        _source: Lang,
        _target: Lang,
        _options: &Options,
        _diagnostics: &mut Diagnostics,
        irs: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![("term", to_yomitan_ipa(irs))]
    }
}

impl Dictionary for DIpaMerged {
    type I = Vec<IIpa>;

    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        _target: Lang,
        entry: &WordEntry,
        irs: &mut Self::I,
    ) {
        process_ipa(edition, source, entry, irs);
    }

    fn postprocess(&self, irs: &mut Self::I) {
        // Keep only unique entries
        *irs = Set::from_iter(irs.drain(..)).into_iter().collect();
        // Sorting is not needed ~ just for visibility
        irs.sort_by(|a, b| a.0.cmp(&b.0));
    }

    fn to_yomitan(
        &self,
        _edition: EditionLang,
        _source: Lang,
        _target: Lang,
        _options: &Options,
        _diagnostics: &mut Diagnostics,
        tidy: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![("term", to_yomitan_ipa(tidy))]
    }
}

// rg: process translations processtranslations
fn process_glossary(
    source: EditionLang,
    target: Lang,
    word_entry: &WordEntry,
    irs: &mut Vec<YomitanEntry>,
) {
    let target_str = target.to_string();

    let mut translations: Map<&str, Vec<String>> = Map::default();
    for translation in word_entry.non_trivial_translations() {
        if translation.lang_code != target_str {
            continue;
        }

        translations
            .entry(&translation.sense)
            .or_default()
            .push(translation.word.clone());
    }

    if translations.is_empty() {
        return;
    }

    let mut definitions = Vec::new();
    for (sense, translations) in translations {
        if sense.is_empty() {
            for translation in translations {
                definitions.push(DetailedDefinition::Text(translation));
            }
            continue;
        }

        let mut sc_translations_content = Node::new_array();
        sc_translations_content.push(wrap(NTag::Span, "", Node::Text(sense.to_string())));
        sc_translations_content.push(wrap(
            NTag::Ul,
            "",
            Node::Array(
                translations
                    .into_iter()
                    .map(|translation| wrap(NTag::Li, "", Node::Text(translation)))
                    .collect(),
            ),
        ));
        let sc_translations =
            DetailedDefinition::structured(wrap(NTag::Div, "", sc_translations_content));
        definitions.push(sc_translations);
    }

    let reading =
        get_reading(source, target, word_entry).unwrap_or_else(|| word_entry.word.clone());
    let found_pos = match find_short_pos(&word_entry.pos) {
        Some(short_pos) => short_pos.to_string(),
        None => word_entry.pos.clone(),
    };

    irs.push(YomitanEntry::TermBank(TermBank(
        word_entry.word.clone(),
        reading,
        found_pos.clone(),
        found_pos,
        definitions,
    )));
}

type IGlossaryExtended = (String, String, EditionLang, Vec<String>);

fn process_glossary_extended(
    edition: EditionLang,
    source: Lang,
    target: Lang,
    word_entry: &WordEntry,
    irs: &mut Vec<IGlossaryExtended>,
) {
    let source_str = source.to_string();
    let target_str = target.to_string();

    let mut translations: Map<&str, (Vec<&str>, Vec<&str>)> = Map::default();
    for translation in word_entry.non_trivial_translations() {
        if translation.lang_code == target_str {
            translations
                .entry(&translation.sense)
                .or_default()
                .0
                .push(&translation.word);
        }

        if translation.lang_code == source_str {
            translations
                .entry(&translation.sense)
                .or_default()
                .1
                .push(&translation.word);
        }
    }

    // We only keep translations with matches in both languages (source and target)
    translations.retain(|_, (targets, sources)| !targets.is_empty() && !sources.is_empty());

    if translations.is_empty() {
        return;
    }

    let found_pos = match find_short_pos(&word_entry.pos) {
        Some(short_pos) => short_pos.to_string(),
        None => word_entry.pos.clone(),
    };

    // A "semi" cartesian product. See the test below.
    irs.extend(translations.iter().flat_map(|(_, (targets, sources))| {
        sources.iter().map(|lemma| {
            (
                lemma.to_string(),
                found_pos.clone(),
                edition,
                targets.iter().map(|def| def.to_string()).collect(),
            )
        })
    }));
}

fn to_yomitan_glossary_extended(irs: Vec<IGlossaryExtended>) -> Vec<YomitanEntry> {
    irs.into_iter()
        .map(|(lemma, found_pos, _, translations)| {
            YomitanEntry::TermBank(TermBank(
                lemma,
                String::new(),
                found_pos.clone(),
                found_pos,
                translations
                    .into_iter()
                    .map(DetailedDefinition::Text)
                    .collect(),
            ))
        })
        .collect()
}

type IIpa = (String, PhoneticTranscription);

fn process_ipa(edition: EditionLang, source: Lang, word_entry: &WordEntry, irs: &mut Vec<IIpa>) {
    let ipas = get_ipas(word_entry);

    if ipas.is_empty() {
        return;
    }

    let phonetic_transcription = PhoneticTranscription {
        reading: get_reading(edition, source, word_entry)
            .unwrap_or_else(|| word_entry.word.clone()),
        transcriptions: ipas,
    };

    irs.push((word_entry.word.clone(), phonetic_transcription));
}

fn to_yomitan_ipa(irs: Vec<IIpa>) -> Vec<YomitanEntry> {
    irs.into_iter()
        .map(|(lemma, transcription)| {
            YomitanEntry::TermBankMeta(TermBankMeta::TermPhoneticTranscription(
                TermPhoneticTranscription(lemma, "ipa".to_string(), transcription),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::models::kaikki::{Sound, Translation};

    impl Translation {
        fn new(lang_code: &str, sense: &str, word: &str) -> Self {
            Self {
                lang_code: lang_code.into(),
                sense: sense.into(),
                word: word.into(),
            }
        }
    }

    // cf. https://en.wiktionary.org/wiki/Gibraltar
    // {
    //     English (sense):       "British overseas territory"
    //     Albanian (sh):         ["Gjibraltar", "Gjibraltari"]
    //     Greek, Ancient (grc):  ["Ἡράκλειαι στῆλαι", "Κάλπη"]
    // }
    //
    //     source                            target (what we search)
    // >>> ["Gjibraltar", "Gjibraltari"]  <> "Ἡράκλειαι στῆλαι"
    // >>> ["Gjibraltar", "Gjibraltari"]  <> "Κάλπη"
    #[test]
    fn process_glossary_extended_basic() {
        let dict = DGlossaryExtended;
        let mut word_entry = WordEntry::default();
        word_entry.translations = vec![
            Translation::new("grc", "British overseas territory", "Ἡράκλειαι στῆλαι"),
            Translation::new("grc", "British overseas territory", "Ἡράκλειαι στῆλαι"),
            Translation::new("grc", "British overseas territory", "Κάλπη"),
            Translation::new("sh", "British overseas territory", "Gibraltar"),
            Translation::new("sh", "British overseas territory", "Gjibraltari"),
            Translation::new("sh", "Different sense", "Foo"),
        ];

        let mut irs = Vec::new();
        let (edition, source, target) = (EditionLang::En, Lang::Grc, Lang::Sh);
        dict.process(edition, source, target, &word_entry, &mut irs);

        // Empty translations should not change anything
        let word_entry = WordEntry::default();
        dict.process(edition, source, target, &word_entry, &mut irs);

        assert_eq!(irs.len(), 3);

        let (lemma1, _, _, defs1) = &irs[0];
        let (lemma2, _, _, defs2) = &irs[1];
        let (lemma3, _, _, defs3) = &irs[2];

        assert_eq!(lemma1, "Ἡράκλειαι στῆλαι");
        assert_eq!(lemma2, "Ἡράκλειαι στῆλαι");
        assert_eq!(lemma3, "Κάλπη");

        let expected = vec!["Gibraltar".to_string(), "Gjibraltari".to_string()];
        assert_eq!(defs1, &expected);
        assert_eq!(defs2, &expected);
        assert_eq!(defs3, &expected);

        dict.postprocess(&mut irs);
        assert_eq!(irs.len(), 2);

        let options = Options::default();
        let mut diagnostics = Diagnostics::default();
        let yomitan_labelled_entries =
            dict.to_yomitan(edition, source, target, &options, &mut diagnostics, irs);
        assert_eq!(yomitan_labelled_entries[0].1.len(), 2);
    }

    impl Sound {
        fn new(ipa: &str) -> Self {
            Self {
                ipa: ipa.into(),
                ..Default::default()
            }
        }
    }

    #[test]
    fn process_ipa_merged_basic() {
        let dict = DIpaMerged;
        let mut word_entry = WordEntry::default();
        word_entry.sounds = vec![Sound::new("ipa1"), Sound::new("ipa1"), Sound::new("ipa2")];

        let mut irs = Vec::new();
        let (edition, source, target) = (EditionLang::En, Lang::Grc, Lang::Sh);
        dict.process(edition, source, target, &word_entry, &mut irs);

        assert_eq!(irs.len(), 1);

        let transcriptions = &irs[0].1.transcriptions;
        assert_eq!(transcriptions.len(), 2);

        assert_eq!(&transcriptions[0].ipa, "ipa1");
        assert_eq!(&transcriptions[1].ipa, "ipa2");

        dict.postprocess(&mut irs);
        assert_eq!(irs[0].1.transcriptions.len(), 2);

        let options = Options::default();
        let mut diagnostics = Diagnostics::default();
        let yomitan_labelled_entries =
            dict.to_yomitan(edition, source, target, &options, &mut diagnostics, irs);
        assert_eq!(yomitan_labelled_entries[0].1.len(), 1);
    }
}
