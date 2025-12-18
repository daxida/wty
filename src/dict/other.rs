use crate::{
    Diagnostics, Dictionary, LabelledYomitanEntry, Map, Set,
    cli::ArgsOptions,
    dict::{get_ipas, get_reading},
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
        make_yomitan_entries_glossary(edition, target, entry, irs);
    }

    fn to_yomitan(
        &self,
        _edition: EditionLang,
        _source: Lang,
        _target: Lang,
        _options: &ArgsOptions,
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
        make_ir_glossary_extended(edition, source, target, entry, irs);
    }

    fn postprocess(&self, irs: &mut Self::I) {
        let mut map = Map::default();

        for (lemma, pos, edition, translations) in irs.drain(..) {
            let entry = map
                .entry(lemma.clone())
                .or_insert_with(|| (pos.clone(), edition, Set::default()));

            for tr in translations {
                entry.2.insert(tr);
            }
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
        _options: &ArgsOptions,
        _diagnostics: &mut Diagnostics,
        irs: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![("term", make_yomitan_glossary_extended(irs))]
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
        make_ir_ipa(edition, source, entry, irs);
    }

    fn to_yomitan(
        &self,
        _edition: EditionLang,
        _source: Lang,
        _target: Lang,
        _options: &ArgsOptions,
        _diagnostics: &mut Diagnostics,
        irs: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![("term", make_yomitan_ipa(irs))]
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
        make_ir_ipa(edition, source, entry, irs);
    }

    fn postprocess(&self, irs: &mut Self::I) {
        // TODO: use dedup
        // Keep only unique entries
        let mut seen = Set::default();
        seen.extend(irs.drain(..));
        *irs = seen.into_iter().collect();
        // Sorting is not needed ~ just for visibility
        irs.sort_by(|a, b| a.0.cmp(&b.0));
    }

    fn to_yomitan(
        &self,
        _edition: EditionLang,
        _source: Lang,
        _target: Lang,
        _options: &ArgsOptions,
        _diagnostics: &mut Diagnostics,
        tidy: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![("term", make_yomitan_ipa(tidy))]
    }
}

fn make_yomitan_entries_glossary(
    source: EditionLang,
    target: Lang,
    word_entry: &WordEntry,
    irs: &mut Vec<YomitanEntry>,
) {
    // rg: process translations processtranslations
    let target_str = target.to_string();

    // The original was fetching translations from the Senses too, but those are documented nowhere
    // and there is not a single occurence in the testsuite.
    let mut translations: Map<Option<String>, Vec<String>> = Map::default();
    for translation in &word_entry.translations {
        if translation.lang_code != target_str || translation.word.is_empty() {
            continue;
        }

        let sense = if translation.sense.is_empty() {
            None
        } else {
            Some(translation.sense.clone())
        };

        let sense_translations = translations.entry(sense).or_default();
        sense_translations.push(translation.word.clone());
    }

    if translations.is_empty() {
        return;
    }

    let mut definitions = Vec::new();
    for (sense, translations) in translations {
        match sense {
            None => {
                for translation in translations {
                    definitions.push(DetailedDefinition::Text(translation));
                }
            }
            Some(sense) => {
                let mut structured_translations_content = Node::new_array();
                let structured_sense = wrap(NTag::Span, "", Node::Text(sense));
                structured_translations_content.push(structured_sense);
                let mut structured_translations_array = Node::new_array();
                for translation in translations {
                    structured_translations_array.push(wrap(NTag::Li, "", Node::Text(translation)));
                }
                structured_translations_content.push(wrap(
                    NTag::Ul,
                    "",
                    structured_translations_array,
                ));
                let structured_translations = DetailedDefinition::structured(wrap(
                    NTag::Div,
                    "",
                    structured_translations_content,
                ));
                definitions.push(structured_translations);
            }
        }
    }

    let reading =
        get_reading(source, target, word_entry).unwrap_or_else(|| word_entry.word.clone());
    let found_pos = match find_short_pos(&word_entry.pos) {
        Some(short_pos) => short_pos.to_string(),
        None => word_entry.pos.clone(),
    };
    let definition_tags = found_pos.clone();

    let ir = YomitanEntry::TermBank(TermBank(
        word_entry.word.clone(),
        reading,
        definition_tags,
        found_pos,
        definitions,
    ));
    irs.push(ir);
}

type IGlossaryExtended = (String, String, EditionLang, Vec<String>);

// Should consume the WordEntry really
fn make_ir_glossary_extended(
    edition: EditionLang,
    source: Lang,
    target: Lang,
    word_entry: &WordEntry,
    irs: &mut Vec<IGlossaryExtended>,
) {
    let target_str = target.to_string();
    let source_str = source.to_string();

    // Compared to glossary, we don't care about the Senses content themselves but the translation
    // must at least match the same sense.

    let mut translations: Map<String, (Vec<String>, Vec<String>)> = Map::default();
    for translation in &word_entry.translations {
        if translation.word.is_empty() {
            continue;
        }

        if translation.lang_code == target_str {
            let sense_translations = translations.entry(translation.sense.clone()).or_default();
            sense_translations.0.push(translation.word.clone());
        }

        if translation.lang_code == source_str {
            let sense_translations = translations.entry(translation.sense.clone()).or_default();
            sense_translations.1.push(translation.word.clone());
        }
    }

    // We only keep translations with matches in both languages
    // Ex. {"male artisan": (["mjeshtër"], ["τεχνίτης"])} (en-sq-grc)
    translations.retain(|_, (targets, sources)| !targets.is_empty() && !sources.is_empty());

    if translations.is_empty() {
        return;
    }

    let found_pos = match find_short_pos(&word_entry.pos) {
        Some(short_pos) => short_pos.to_string(),
        None => word_entry.pos.clone(),
    };

    let mut translations_product = Vec::new();

    for (_sense, translations) in translations {
        // A "semi" cartesian product:
        // {"British overseas territory": (["Gjibraltar", "Gjibraltari"], ["Ἡράκλειαι στῆλαι", "Κάλπη"])}
        //     source                            target (what we search)
        // >>> ["Gjibraltar", "Gjibraltari"]  <> "Ἡράκλειαι στῆλαι"
        // >>> ["Gjibraltar", "Gjibraltari"]  <> "Κάλπη"

        for lemma in translations.1 {
            let mut definitions = Vec::new();
            for translation in &translations.0 {
                definitions.push(translation.to_string());
            }
            let entry = (lemma, found_pos.clone(), edition, definitions);
            translations_product.push(entry);
        }
    }

    irs.extend(translations_product);
}

fn make_yomitan_glossary_extended(irs: Vec<IGlossaryExtended>) -> Vec<YomitanEntry> {
    irs.into_iter()
        .map(|ir| {
            let (lemma, found_pos, _, translations) = ir;

            let mut definitions = Vec::new();
            for translation in &translations {
                definitions.push(DetailedDefinition::Text(translation.to_string()));
            }

            YomitanEntry::TermBank(TermBank(
                lemma,
                String::new(),
                found_pos.clone(),
                found_pos,
                definitions,
            ))
        })
        .collect()
}

type IIpa = (String, PhoneticTranscription);

fn make_ir_ipa(edition: EditionLang, source: Lang, word_entry: &WordEntry, irs: &mut Vec<IIpa>) {
    let ipas = get_ipas(word_entry);

    if ipas.is_empty() {
        return;
    }

    let phonetic_transcription = PhoneticTranscription {
        reading: get_reading(edition, source, word_entry)
            .unwrap_or_else(|| word_entry.word.clone()),
        transcriptions: ipas,
    };

    let ir: IIpa = (word_entry.word.clone(), phonetic_transcription);
    irs.push(ir);
}

fn make_yomitan_ipa(irs: Vec<IIpa>) -> Vec<YomitanEntry> {
    irs.into_iter()
        .map(|ir| {
            let (lemma, phonetic_transcription) = ir;
            YomitanEntry::TermBankMeta(TermBankMeta::TermPhoneticTranscription(
                TermPhoneticTranscription(lemma, "ipa".to_string(), phonetic_transcription),
            ))
        })
        .collect()
}
