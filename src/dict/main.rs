use std::{fs::File, io::BufWriter, sync::LazyLock};

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::{
    Map, Set,
    cli::Options,
    dict::{
        Diagnostics, Dictionary, Intermediate, LabelledYomitanEntry,
        locale::localize_examples_string,
    },
    lang::{EditionLang, Lang},
    models::{
        kaikki::{Example, Form, HeadTemplate, Pos, Sense, Tag, WordEntry},
        yomitan::{
            BacklinkContent, DetailedDefinition, GenericNode, Ipa, NTag, Node, NodeData, TermBank,
            YomitanEntry, wrap,
        },
    },
    path::PathManager,
    tags::{
        REDUNDANT_FORM_TAGS, find_short_pos, find_tag_in_bank, merge_person_tags,
        remove_redundant_tags, sort_tags, sort_tags_by_similar,
    },
    utils::{link_kaikki, link_wiktionary, pretty_println_at_path},
};

#[derive(Debug, Clone, Copy)]
pub struct DMain;

impl Intermediate for Tidy {
    fn len(&self) -> usize {
        self.len()
    }

    fn write(&self, pm: &PathManager, options: &Options) -> Result<()> {
        self.write(options, pm)
    }
}

impl Dictionary for DMain {
    type I = Tidy;

    fn preprocess(
        &self,
        edition: EditionLang,
        source: Lang,
        _: Lang,
        word_entry: &mut WordEntry,
        options: &Options,
        irs: &mut Self::I,
    ) {
        preprocess_main(edition, source, options, word_entry, irs);
    }

    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        _: Lang,
        word_entry: &WordEntry,
        irs: &mut Self::I,
    ) {
        process_main(edition, source, word_entry, irs);
    }

    fn postprocess(&self, irs: &mut Self::I) {
        postprocess_forms(&mut irs.form_map);
    }

    fn found_ir_message(&self, irs: &Self::I) {
        // A bit hacky to have it here
        let n_lemmas = irs.lemma_map.len();
        let n_forms = irs.form_map.len();
        let n_forms_inflection = irs.form_map.len_inflection();
        let n_forms_extracted = irs.form_map.len_extracted();
        let n_forms_alt_of = irs.form_map.len_alt_of();
        debug_assert_eq!(
            n_forms,
            n_forms_inflection + n_forms_extracted + n_forms_alt_of,
            "mismatch in form counts"
        );
        let n_entries = n_lemmas + n_forms;
        println!(
            "Found {n_entries} entries: {n_lemmas} lemmas, {n_forms} forms \
({n_forms_inflection} inflections, {n_forms_extracted} extracted, {n_forms_alt_of} alt_of)"
        );
    }

    fn write_ir(&self) -> bool {
        true
    }

    fn to_yomitan(
        &self,
        edition: EditionLang,
        source: Lang,
        _: Lang,
        options: &Options,
        diagnostics: &mut Diagnostics,
        irs: Self::I,
    ) -> Vec<LabelledYomitanEntry> {
        vec![
            (
                "lemma",
                to_yomitan_lemmas(edition, options, irs.lemma_map, diagnostics),
            ),
            ("form", to_yomitan_forms(source, irs.form_map)),
        ]
    }

    fn write_diagnostics(&self, pm: &PathManager, diagnostics: &Diagnostics) -> Result<()> {
        diagnostics.write(pm)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct LemmaKey {
    lemma: String,
    reading: String,
    pos: Pos,
}

#[derive(Debug, Default)]
struct LemmaMap(Map<LemmaKey, Vec<LemmaInfo>>);

// We only serialize for debugging in the testsuite, so having this tmp nested is easy to write and
// has no overhead when building the dictionary without --save-temps. This way, we avoid storing
// nested structures that are less performant (both for cache locality, and number of lookups).
impl Serialize for LemmaMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut nested: Map<&str, Map<&str, Map<&str, &Vec<LemmaInfo>>>> = Map::default();

        for (key, infos) in &self.0 {
            nested
                .entry(&key.lemma)
                .or_default()
                .entry(&key.reading)
                .or_default()
                .insert(&key.pos, infos);
        }

        nested.serialize(serializer)
    }
}

impl LemmaMap {
    fn len(&self) -> usize {
        self.0.values().map(Vec::len).sum()
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct FormKey {
    uninflected: String,
    inflected: String,
    pos: Pos,
}

#[derive(Debug, Default)]
struct FormMap(Map<FormKey, (FormSource, Vec<String>)>);

// We only serialize for debugging in the testsuite, so having this tmp nested is easy to write and
// has no overhead when building the dictionary without --save-temps. This way, we avoid storing
// nested structures that are less performant (both for cache locality, and number of lookups).
impl Serialize for FormMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut nested: Map<&str, Map<&str, Map<&str, &(FormSource, Vec<String>)>>> =
            Map::default();

        for (key, infos) in &self.0 {
            nested
                .entry(&key.uninflected)
                .or_default()
                .entry(&key.inflected)
                .or_default()
                .insert(&key.pos, infos);
        }

        nested.serialize(serializer)
    }
}

impl FormMap {
    /// Iterates over: uninflected, inflected, pos, source, tags
    fn flat_iter(&self) -> impl Iterator<Item = (&str, &str, &str, &FormSource, &Vec<String>)> {
        self.0.iter().map(|(key, (source, tags))| {
            (
                key.uninflected.as_str(),
                key.inflected.as_str(),
                key.pos.as_str(),
                source,
                tags,
            )
        })
    }

    /// Iterates over: uninflected, inflected, pos, source, tags
    fn flat_iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (&str, &str, &str, &mut FormSource, &mut Vec<String>)> {
        self.0.iter_mut().map(|(key, (source, tags))| {
            (
                key.uninflected.as_str(),
                key.inflected.as_str(),
                key.pos.as_str(),
                source,
                tags,
            )
        })
    }

    fn len(&self) -> usize {
        self.flat_iter().count()
    }

    fn len_of(&self, source: FormSource) -> usize {
        self.flat_iter()
            .filter(|(_, _, _, src, _)| **src == source)
            .count()
    }

    fn len_extracted(&self) -> usize {
        self.len_of(FormSource::Extracted)
    }

    fn len_inflection(&self) -> usize {
        self.len_of(FormSource::Inflection)
    }

    fn len_alt_of(&self) -> usize {
        self.len_of(FormSource::AltOf)
    }
}

/// Enum used exclusively for debugging. This information doesn't appear on the dictionary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum FormSource {
    /// Form extracted from `word_entry.forms`
    Extracted,
    /// Form added via gloss analysis ("is inflection of...")
    Inflection,
    /// Alternative forms
    AltOf,
}

// NOTE: the less we have here the better. For example, the links could be entirely moved to the
// yomitan side of things. It all depends on what we may or may not consider useful for debugging.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct LemmaInfo {
    gloss_tree: GlossTree,

    #[serde(skip_serializing_if = "Option::is_none")]
    etymology_text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    head_info_text: Option<String>,

    #[serde(rename = "wlink")]
    link_wiktionary: String,

    #[serde(rename = "klink")]
    link_kaikki: String,
}

type GlossTree = Map<String, GlossInfo>;

// ... its really SenseInfo but oh well
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct GlossInfo {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<Tag>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    topics: Vec<Tag>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    examples: Vec<Example>,

    #[serde(skip_serializing_if = "Map::is_empty")]
    children: GlossTree,
}

/// Intermediate representation of the main dictionary.
#[derive(Debug, Default)]
pub struct Tidy {
    lemma_map: LemmaMap, // 56
    form_map: FormMap,   // 56
}

impl Tidy {
    fn len(&self) -> usize {
        self.lemma_map.len() + self.form_map.len()
    }

    // This is usually called at the end, so it could just move the arguments...
    fn insert_lemma(&mut self, lemma: &str, reading: &str, pos: &str, entry: LemmaInfo) {
        debug_assert!(!entry.gloss_tree.is_empty());

        let key = LemmaKey {
            lemma: lemma.into(),
            reading: reading.into(),
            pos: pos.into(),
        };

        self.lemma_map.0.entry(key).or_default().push(entry);
    }

    fn insert_form(
        &mut self,
        uninflected: &str,
        inflected: &str,
        pos: &str,
        source: FormSource,
        tags: Vec<Tag>,
    ) {
        debug_assert_ne!(uninflected, inflected);
        debug_assert!(!tags.is_empty());

        let key = FormKey {
            uninflected: uninflected.into(),
            inflected: inflected.into(),
            pos: pos.into(),
        };

        self.form_map
            .0
            .entry(key)
            .or_insert_with(|| (source, Vec::new()))
            .1
            .extend(tags);
    }

    // NOTE: we write stuff even if irs.attribute is empty
    #[tracing::instrument(skip_all)]
    fn write(&self, options: &Options, pm: &PathManager) -> Result<()> {
        let opath = pm.path_lemmas();
        let file = File::create(&opath)?;
        let writer = BufWriter::new(file);

        if options.pretty {
            serde_json::to_writer_pretty(writer, &self.lemma_map)?;
        } else {
            serde_json::to_writer(writer, &self.lemma_map)?;
        }
        if !options.quiet {
            pretty_println_at_path("Wrote tidy lemmas", &opath);
        }

        let opath = pm.path_forms();
        let file = File::create(&opath)?;
        let writer = BufWriter::new(file);

        if options.pretty {
            serde_json::to_writer_pretty(writer, &self.form_map)?;
        } else {
            serde_json::to_writer(writer, &self.form_map)?;
        }
        if !options.quiet {
            pretty_println_at_path("Wrote tidy forms", &opath);
        }

        Ok(())
    }
}

fn postprocess_forms(form_map: &mut FormMap) {
    for (_, _, _, _, tags) in form_map.flat_iter_mut() {
        // Keep only unique tags and remove tags subsets
        remove_redundant_tags(tags);

        // Merge person tags
        merge_person_tags(tags);

        // Sort inner words
        for tag in tags.iter_mut() {
            let mut words: Vec<&str> = tag.split(' ').collect();
            sort_tags(&mut words);
            *tag = words.join(" ");
        }

        sort_tags_by_similar(tags);
    }
}

fn process_main(edition: EditionLang, source: Lang, word_entry: &WordEntry, irs: &mut Tidy) {
    process_forms(edition, source, word_entry, irs);

    process_alt_forms(word_entry, irs);

    // Don't push a lemma if the word_entry has no glosses (f.e. if it is an inflection etc.)
    if word_entry.contains_no_gloss() {
        process_no_gloss(edition, word_entry, irs);
        return;
    }

    if let Some(raw_sense_entry) = process_word_entry(edition, source, word_entry) {
        let reading =
            get_reading(edition, source, word_entry).unwrap_or_else(|| word_entry.word.clone());
        irs.insert_lemma(&word_entry.word, &reading, &word_entry.pos, raw_sense_entry);
    }
}

// Everything that mutates word_entry
fn preprocess_main(
    edition: EditionLang,
    source: Lang,
    options: &Options,
    word_entry: &mut WordEntry,
    irs: &mut Tidy,
) {
    // WARN: mutates word_entry::senses::sense::tags
    match edition {
        EditionLang::En => {
            // The original fetched them from head_templates but it is better not to touch that
            // and we can do the same by looking at the tags of the canonical form.
            if let Some(cform) = word_entry.canonical_form() {
                let cform_tags: Vec<_> = cform.tags.clone();
                for sense in &mut word_entry.senses {
                    for tag in &cform_tags {
                        if tag != "canonical" && !sense.tags.contains(tag) {
                            sense.tags.push(tag.into());
                        }
                    }
                }
            }
        }
        EditionLang::El => {
            // Fetch gender from a matching form
            let gender_tags = ["masculine", "feminine", "neuter"];
            for form in &word_entry.forms {
                if form.form == word_entry.word {
                    for sense in &mut word_entry.senses {
                        for tag in &form.tags {
                            if gender_tags.contains(&tag.as_str()) && !sense.tags.contains(tag) {
                                sense.tags.push(tag.into());
                            }
                        }
                    }
                }
            }
        }
        EditionLang::Ru => {
            // Propagate word_entry.tags to sense.tags
            for sense in &mut word_entry.senses {
                for tag in &word_entry.tags {
                    if !sense.tags.contains(tag) {
                        sense.tags.push(tag.into());
                    }
                }
            }
        }
        _ => (),
    }

    // WARN: mutates word_entry::senses
    //
    // What if the current word is an inflection but *also* has an inflection table?
    // https://el.wiktionary.org/wiki/ψηφίσας
    //
    // That is, imagine participle A comes from verb B, but A is treated as an adjective, so
    // it has a declension table. If we are not careful, every word C in the table that is a form
    // of A will not appear in the dictionary!
    //
    // It does not happen in English, but bear with this fake example:
    // * C = runnings < A = running < B = run
    // then, by saying that A is just a form of B, we will remove the sense, and the entry won't be
    // added to lemmas because there are no senses at all. All the information in the declension
    // table saying C < A will yield no results. Effectively, hovering over C in yomitan will show
    // nothing. Not ideal.
    //
    // There are two choices, make C point to B, or keep A as a non-lemma. We opt for the latter,
    // checking that there are no trivial forms (C) in WordEntry. Only then we can safely delete
    // the sense.
    //
    // Note that deleting senses is a good decision overall: it reduces clutter and forces the
    // redirect. One just has to be careful about when to do it
    //
    let old_senses = std::mem::take(&mut word_entry.senses);
    let mut senses_without_inflections = Vec::new();
    for sense in old_senses {
        if is_inflection_sense(edition, &sense)
            && (!options.experimental || word_entry.non_trivial_forms().next().is_none())
        {
            handle_inflection_sense(edition, source, word_entry, &sense, irs);
        } else {
            senses_without_inflections.push(sense);
        }
    }
    word_entry.senses = senses_without_inflections;

    // WARN: mutates word_entry::senses::glosses
    //
    // rg: full stop
    // https://github.com/yomidevs/yomitan/issues/2232
    // Add an empty whitespace at the end... and it works!
    if options.experimental {
        static TRAILING_PUNCT_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\p{P}$").unwrap());
        for sense in &mut word_entry.senses {
            for gloss in &mut sense.glosses {
                if !TRAILING_PUNCT_RE.is_match(gloss) {
                    gloss.push(' ');
                }
            }
        }
    }
}

/// Add Extracted forms. That is, forms from `word_entry.forms`.
fn process_forms(edition: EditionLang, source: Lang, word_entry: &WordEntry, irs: &mut Tidy) {
    for form in word_entry.non_trivial_forms() {
        let filtered_tags: Vec<_> = form
            .tags
            .iter()
            .map(String::as_str)
            .filter(|tag| !REDUNDANT_FORM_TAGS.contains(tag))
            .collect();
        if filtered_tags.is_empty() {
            continue;
        }

        if should_break_at_finish_forms(edition, source, form) {
            break;
        }

        irs.insert_form(
            &word_entry.word,
            &form.form,
            &word_entry.pos,
            FormSource::Extracted,
            vec![filtered_tags.join(" ")],
        );
    }
}

// Finnish from the English edition crashes with out-of-memory.
// There are simply too many forms, so we prune the less used (possessive).
//
// https://uusikielemme.fi/finnish-grammar/possessive-suffixes-possessiivisuffiksit#one
fn should_break_at_finish_forms(edition: EditionLang, source: Lang, form: &Form) -> bool {
    if matches!((edition, source), (EditionLang::En, Lang::Fi)) {
        // HACK: 1. For tables that parse the title
        // https://kaikki.org/dictionary/Finnish/meaning/p/p%C3%A4/p%C3%A4%C3%A4.html
        if form.form == "See the possessive forms below." {
            return true;
        }
        // HACK: 2. For tables that don't parse the title
        // https://kaikki.org/dictionary/Finnish/meaning/i/is/iso.html
        // https://github.com/tatuylonen/wiktextract/issues/1565
        if form.form == "Rare. Only used with substantive adjectives." {
            return true;
        }
    }
    false
}

/// Add `AltOf` forms. That is, alternative forms.
fn process_alt_forms(word_entry: &WordEntry, irs: &mut Tidy) {
    let base_tags = vec!["alt-of".to_string()];

    for alt_form in &word_entry.alt_of {
        irs.insert_form(
            &word_entry.word,
            &alt_form.word,
            &word_entry.pos,
            FormSource::AltOf,
            base_tags.clone(),
        );
    }

    for sense in &word_entry.senses {
        let mut sense_tags = sense.tags.clone();
        sense_tags.extend(base_tags.clone());

        for alt_form in &sense.alt_of {
            irs.insert_form(
                &word_entry.word,
                &alt_form.word,
                &word_entry.pos,
                FormSource::AltOf,
                sense_tags.clone(),
            );
        }
    }
}

/// Process "no-gloss" word entries for alternative ways of adding lemmas/forms.
fn process_no_gloss(edition: EditionLang, word_entry: &WordEntry, irs: &mut Tidy) {
    match edition {
        // Unfortunately we are in the same A from B, B from C situation discussed in
        // preprocess_word_entry. There is no easy solution for adding the lemma back because at
        // this point the gloss has been deleted. Maybe reconsider the original approach of
        // deleting glosses, and mark them somehow as "inflection-only".
        //
        // At any rate, this will still add useful redirections.
        EditionLang::El => {
            if word_entry.is_participle()
                && let Some(form_of) = word_entry.form_of.first()
            {
                irs.insert_form(
                    &form_of.word,
                    &word_entry.word,
                    &word_entry.pos,
                    FormSource::Inflection,
                    vec![format!("redirected from {}", word_entry.word)],
                );
            }
        }
        _ => (),
    }
}

// There are potentially more than one, but yomitan doesn't really support it
pub fn get_reading(edition: EditionLang, source: Lang, word_entry: &WordEntry) -> Option<String> {
    match (edition, source) {
        (EditionLang::En, Lang::Ja) => get_japanese_reading(word_entry),
        (EditionLang::En, Lang::Fa) => word_entry.romanization_form().map(|f| f.form.clone()),
        (EditionLang::Ja, _) => word_entry.transliteration_form().map(|f| f.form.clone()),
        (EditionLang::En | EditionLang::Zh, Lang::Zh) => word_entry.pinyin().map(String::from),
        _ => get_canonical_word(source, word_entry),
    }
}

/// The canonical word may contain extra diacritics.
///
/// For most languages, this is equal to word, but for, let's say, Latin, there may be a
/// difference (cf. <https://en.wiktionary.org/wiki/fama>, where `word_entry.word` is fama, but
/// this will return fāma).
fn get_canonical_word(source: Lang, word_entry: &WordEntry) -> Option<String> {
    match source {
        Lang::La | Lang::Ru | Lang::Grc => word_entry.canonical_form().map(|f| f.form.to_string()),
        _ => None,
    }
}

// Does not support multiple readings
fn get_japanese_reading(word_entry: &WordEntry) -> Option<String> {
    // The original parses head_templates directly (which probably deserves a PR to
    // wiktextract), although imo pronunciation templates should have been better.
    // There is no pronunciation template info in en-wiktextract, and while I think that
    // information ends up in sounds, it is not always reliable. For example:
    // https://en.wiktionary.org/wiki/お腹が空いた
    // has a pronunciation template:
    // {{ja-pron|おなか が すいた}}
    // but no "other" sounds, which is where pronunciations are usually stored.

    // Ideally we would just do this:
    // for sound in &word_entry.sounds {
    //     if !sound.other.is_empty() {
    //         return &sound.other;
    //     }
    // }

    // I really don't want to touch templates so instead, replace the ruby
    if let Some(cform) = word_entry.canonical_form()
        && !cform.ruby.is_empty()
    {
        // https://github.com/tatuylonen/wiktextract/issues/1484
        // let mut cform_lemma = cform.form.clone();
        // if cform_lemma != word_entry.word {
        //     warn!(
        //         "Canonical form: '{cform_lemma}' != word: '{}'\n{}\n{}\n\n",
        //         word_entry.word,
        //         link_wiktionary(args, &word_entry.word),
        //         link_kaikki(args, &word_entry.word),
        //     );
        // } else {
        //     warn!(
        //         "Equal for word: '{}'\n{}\n{}\n\n",
        //         word_entry.word,
        //         link_wiktionary(args, &word_entry.word),
        //         link_kaikki(args, &word_entry.word),
        //     );
        // }

        // This should be cform.form, but it's not parsed properly:
        // https://github.com/tatuylonen/wiktextract/issues/1484
        let mut cform_lemma = word_entry.word.clone();
        let mut cursor = 0;
        for (base, reading) in &cform.ruby {
            if let Some(pos) = cform_lemma[cursor..].find(base) {
                let start = cursor + pos;
                let end = start + base.len();
                cform_lemma.replace_range(start..end, reading);
                cursor = start + reading.len();
            } else {
                tracing::warn!("Kanji '{}' not found in '{}'", base, cform_lemma);
                return None;
            }
        }
        return Some(cform_lemma);
    }

    None
}

// rg: handleline handle_line
fn process_word_entry(
    edition: EditionLang,
    source: Lang,
    word_entry: &WordEntry,
) -> Option<LemmaInfo> {
    let gloss_tree = get_gloss_tree(word_entry);

    if gloss_tree.is_empty() {
        // Happens if word_entry has no glosses. Common if the edition supports notices for page
        // stubs, orthography variants etc., cf. https://de.wiktionary.org/wiki/caritativ
        return None;
    }

    let etymology_text = word_entry
        .etymology_texts()
        .map(|etymology_text| etymology_text.join("\n"));

    Some(LemmaInfo {
        gloss_tree,
        etymology_text,
        head_info_text: get_head_info(&word_entry.head_templates)
            .map(std::string::ToString::to_string),
        link_wiktionary: link_wiktionary(edition, source, &word_entry.word),
        link_kaikki: link_kaikki(edition, source, &word_entry.word),
    })
}

// default version getphonetictranscription
pub fn get_ipas(word_entry: &WordEntry) -> Vec<Ipa> {
    let ipas_iter = word_entry.sounds.iter().filter_map(|sound| {
        if sound.ipa.is_empty() {
            return None;
        }
        let ipa = sound.ipa.clone();
        let mut tags = sound.tags.clone();
        if !sound.note.is_empty() {
            tags.push(sound.note.clone());
        }
        Some(Ipa { ipa, tags })
    });

    // rg: saveIpaResult - Group by ipa
    let mut ipas_grouped: Vec<Ipa> = Vec::new();
    for ipa in ipas_iter {
        if let Some(existing) = ipas_grouped.iter_mut().find(|e| e.ipa == ipa.ipa) {
            for tag in ipa.tags {
                if !existing.tags.contains(&tag) {
                    existing.tags.push(tag);
                }
            }
        } else {
            ipas_grouped.push(ipa);
        }
    }

    ipas_grouped
}

static PARENS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(.+?\)").unwrap());

// rg: getheadinfo
fn get_head_info(head_templates: &[HeadTemplate]) -> Option<&str> {
    head_templates.iter().find_map(|head_template| {
        if PARENS_RE.is_match(&head_template.expansion) {
            Some(head_template.expansion.as_str())
        } else {
            None
        }
    })
}

fn get_gloss_tree(entry: &WordEntry) -> GlossTree {
    let mut gloss_tree = GlossTree::default();

    for sense in &entry.senses {
        // rg: examplefiltering
        // bunch of example filtering: skip

        let mut filtered_examples: Vec<_> = sense
            .examples
            .iter()
            .filter(|ex| !ex.text.is_empty() && ex.text.chars().count() <= 120) // equal to JS length
            .cloned()
            .collect();
        // Place examples with translations first
        filtered_examples.sort_by_key(|ex| ex.translation.is_empty());

        insert_glosses(
            &mut gloss_tree,
            &sense.glosses,
            &sense.tags,
            &sense.topics,
            &filtered_examples,
        );
    }

    gloss_tree
}

/// Recursive helper to deal with nested glosses
fn insert_glosses(
    gloss_tree: &mut GlossTree,
    glosses: &[String],
    tags: &[Tag],
    topics: &[Tag],
    examples: &[Example],
) {
    let Some(head) = glosses.get(0) else {
        return;
    };

    let tail = &glosses[1..];

    // get or insert node with only tags at this level
    let node = gloss_tree.entry(head.clone()).or_insert_with(|| GlossInfo {
        tags: tags.to_vec(),
        topics: topics.to_vec(),
        ..Default::default()
    });

    // intersect tags if node already exists
    if !node.tags.is_empty() {
        node.tags = tags
            .iter()
            .filter(|&t| node.tags.contains(t))
            .cloned()
            .collect();
    }

    // assign examples to the last level
    if tail.is_empty() {
        node.examples = examples.to_vec();
        return;
    }

    insert_glosses(&mut node.children, tail, tags, topics, examples);
}

static DE_INFLECTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(.*)des (?:Verbs|Adjektivs|Substantivs|Demonstrativpronomens|Possessivpronomens|Pronomens) (.*)$"
    ).unwrap()
});

// rg: isinflectiongloss
fn is_inflection_sense(edition: EditionLang, sense: &Sense) -> bool {
    match edition {
        EditionLang::De => sense
            .glosses
            .iter()
            .any(|gloss| DE_INFLECTION_RE.is_match(gloss)),
        EditionLang::El => {
            !sense.form_of.is_empty() && sense.glosses.iter().any(|gloss| gloss.contains("του"))
        }
        EditionLang::En => {
            sense.glosses.iter().any(|gloss| {
                if gloss.contains("inflection of") {
                    return true;
                }

                for form in &sense.form_of {
                    if form.word.is_empty() {
                        continue;
                    }
                    // We are looking for "... of {word}$" or "... of {word} (text)$"
                    //
                    // Cf.
                    // ... imperative of iki
                    // ... perfective of возни́кнуть (vozníknutʹ)
                    // But no
                    // ... agent noun of fahren; driver (person)
                    let subs = format!("of {}", form.word);
                    if gloss.ends_with(&subs)
                        || (gloss.contains(&format!("{subs} (")) && gloss.ends_with(')'))
                    {
                        return true;
                    }
                }

                false
            })
        }
        EditionLang::Fr => {
            !sense.form_of.is_empty()
                && sense
                    .glosses
                    .iter()
                    .any(|gloss| gloss.contains("personne du"))
        }
        _ => false,
    }
}

const TAGS_RETAINED_EL: [&str; 9] = [
    "masculine",
    "feminine",
    "neuter",
    "singular",
    "plural",
    "nominative",
    "accusative",
    "genitive",
    "vocative",
];

fn handle_inflection_sense(
    edition: EditionLang,
    source: Lang,
    word_entry: &WordEntry,
    sense: &Sense,
    irs: &mut Tidy,
) {
    debug_assert!(!sense.glosses.is_empty()); // we checked @ is_inflection_sense

    match edition {
        EditionLang::De => {
            if let Some(caps) = DE_INFLECTION_RE.captures(&sense.glosses[0])
                && let (Some(inflection_tags), Some(uninflected)) = (caps.get(1), caps.get(2))
            {
                let inflection_tags = inflection_tags.as_str().trim();

                if !inflection_tags.is_empty() {
                    irs.insert_form(
                        uninflected.as_str(),
                        &word_entry.word,
                        &word_entry.pos,
                        FormSource::Inflection,
                        vec![inflection_tags.to_string()],
                    );
                }
            }
        }
        EditionLang::El => {
            let allowed_tags: Vec<_> = sense
                .tags
                .iter()
                .filter(|tag| TAGS_RETAINED_EL.contains(&tag.as_str()))
                .map(String::from)
                .collect();
            let inflection_tags: Vec<_> = if allowed_tags.is_empty() {
                // very rare
                vec![format!("redirected from {}", word_entry.word)]
            } else {
                allowed_tags
            };
            for form in &sense.form_of {
                irs.insert_form(
                    &form.word,
                    &word_entry.word,
                    &word_entry.pos,
                    FormSource::Inflection,
                    // Unfortunate clone. Most sense.form_of only contain one form...
                    inflection_tags.clone(),
                );
            }
        }
        EditionLang::En => handle_inflection_sense_en(source, word_entry, sense, irs),
        EditionLang::Fr => {
            let allowed_tags: Vec<_> = sense
                .tags
                .iter()
                .filter(|tag| *tag != "form-of")
                .map(String::from)
                .collect();
            let inflection_tags: Vec<_> = if allowed_tags.is_empty() {
                vec![format!("redirected from {}", word_entry.word)]
            } else {
                allowed_tags
            };
            for form in &sense.form_of {
                irs.insert_form(
                    &form.word,
                    &word_entry.word,
                    &word_entry.pos,
                    FormSource::Inflection,
                    inflection_tags.clone(),
                );
            }
        }
        _ => unreachable!("Unhandled lang that implements is_inflection_sense"),
    }
}

fn handle_inflection_sense_en(source: Lang, word_entry: &WordEntry, sense: &Sense, irs: &mut Tidy) {
    let uninflected = match sense.form_of.as_slice() {
        [alt_form] => &alt_form.word,
        _ => return,
    };

    // Not sure if this is better (cf. ru-en) over word_entry.word but it is what was done in
    // the original, so lets not change that for the moment.
    let inflected =
        get_canonical_word(source, word_entry).unwrap_or_else(|| word_entry.word.clone());

    if inflected == *uninflected {
        return;
    }

    let mut inflections = Set::default();
    let of_uninflected = format!("of {uninflected}");
    for gloss in &sense.glosses {
        let cleaned = gloss
            .replace("inflection of ", "")
            .replace(&of_uninflected, "")
            .replace(&*uninflected, "")
            .replace(':', "");

        let inflection = PARENS_RE.replace_all(&cleaned, "").trim().to_string();

        if !inflection.is_empty() {
            inflections.insert(inflection);
        }
    }

    for inflection in inflections {
        irs.insert_form(
            &uninflected,
            &inflected,
            &word_entry.pos,
            FormSource::Inflection,
            vec![inflection],
        );
    }
}

fn normalize_orthography(source: Lang, word: &str) -> String {
    match source {
        Lang::Grc | Lang::La | Lang::Ru => {
            // Normalize to NFD and drop combining accents
            word.nfd()
                .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
                .collect()
        }
        _ => word.to_string(),
    }
}

#[tracing::instrument(skip_all)]
fn to_yomitan_lemmas(
    edition: EditionLang,
    options: &Options,
    lemma_map: LemmaMap,
    diagnostics: &mut Diagnostics,
) -> Vec<YomitanEntry> {
    let mut yomitan_entries = Vec::new();

    for (key, infos) in lemma_map.0 {
        let LemmaKey {
            lemma,
            reading,
            pos,
        } = key;

        yomitan_entries.extend(infos.into_iter().map(|info| {
            to_yomitan_lemma(edition, options, &lemma, &reading, &pos, info, diagnostics)
        }));
    }

    yomitan_entries
}

// TODO: consume info
fn to_yomitan_lemma(
    edition: EditionLang,
    options: &Options,
    lemma: &str,
    reading: &str,
    pos: &Pos, // should be &str
    info: LemmaInfo,
    diagnostics: &mut Diagnostics,
) -> YomitanEntry {
    let found_pos = match find_short_pos(pos) {
        Some(short_pos) => short_pos.to_string(),
        None => pos.clone(),
    };

    let yomitan_reading = if *reading == *lemma { "" } else { reading };

    let common_short_tags_found =
        get_found_tags(options, lemma, pos, &info.gloss_tree, diagnostics);

    let mut detailed_definition_content = Node::new_array();

    if info.etymology_text.is_some() || info.head_info_text.is_some() {
        detailed_definition_content.push(structured_preamble(
            info.etymology_text,
            info.head_info_text,
        ));
    }

    detailed_definition_content.push(structured_glosses(
        edition,
        info.gloss_tree,
        &common_short_tags_found,
    ));

    detailed_definition_content.push(structured_backlink(info.link_wiktionary, info.link_kaikki));

    YomitanEntry::TermBank(TermBank(
        lemma.to_string(),
        yomitan_reading.to_string(),
        common_short_tags_found.join(" "),
        found_pos,
        vec![DetailedDefinition::structured(detailed_definition_content)],
    ))
}

fn get_found_tags(
    options: &Options,
    lemma: &str,
    pos: &Pos,
    gloss_tree: &GlossTree,
    diagnostics: &mut Diagnostics,
) -> Vec<Tag> {
    // Common tags to all glosses (this is an English edition reasoning really...)
    // it should also support tags at the WordEntry level
    let common_tags_iter = gloss_tree
        .values()
        .map(|g| Set::from_iter(g.tags.iter().cloned()))
        .reduce(|acc, set| acc.intersection(&set).cloned().collect::<Set<Tag>>())
        .unwrap() // a non-empty gloss_tree has at least one gloss
        .into_iter();

    // rg: processtags process_tags
    let mut common_short_tags_found: Vec<Tag> = Vec::new();

    // we add pos (at index 0) for this search!
    for tag in std::iter::once(pos.to_string()).chain(common_tags_iter) {
        match find_tag_in_bank(&tag) {
            None => {
                // try modified tag: skip
                if options.save_temps {
                    diagnostics.increment_rejected_tag(tag, lemma.to_string());
                }
            }
            Some(res) => {
                if options.save_temps {
                    diagnostics.increment_accepted_tag(tag, lemma.to_string());
                }
                common_short_tags_found.push(res.short_tag);
            }
        }
    }

    common_short_tags_found
}

fn build_details_entry(ty: &str, content: String) -> Node {
    wrap(
        NTag::Details,
        &format!("details-entry-{ty}"),
        Node::Array(vec![
            wrap(NTag::Summary, "summary-entry", Node::Text(ty.into())),
            wrap(NTag::Div, &format!("{ty}-content"), Node::Text(content)),
        ]),
    )
}

fn structured_preamble(etymology_text: Option<String>, head_info_text: Option<String>) -> Node {
    let mut preamble_content = Node::new_array();
    if let Some(head_info_text) = head_info_text {
        preamble_content.push(build_details_entry("Grammar", head_info_text));
    }
    if let Some(etymology_text) = etymology_text {
        preamble_content.push(build_details_entry("Etymology", etymology_text));
    }

    wrap(
        NTag::Div,
        "",
        wrap(NTag::Div, "preamble", preamble_content).into_array_node(),
    )
}

fn structured_backlink(wlink: String, klink: String) -> Node {
    wrap(
        NTag::Div,
        "backlink",
        Node::Array(vec![
            Node::Backlink(BacklinkContent::new(wlink, "Wiktionary")),
            Node::Text(" | ".into()), // JMdict uses this separator
            Node::Backlink(BacklinkContent::new(klink, "Kaikki")),
        ]),
    )
}

fn structured_glosses(
    edition: EditionLang,
    gloss_tree: GlossTree,
    common_short_tags_found: &[Tag],
) -> Node {
    wrap(
        NTag::Ol,
        "glosses",
        Node::Array(
            gloss_tree
                .into_iter()
                .map(|gloss_pair| {
                    wrap(
                        NTag::Li,
                        "",
                        Node::Array(structured_glosses_go(
                            edition,
                            &GlossTree::from_iter([gloss_pair]),
                            common_short_tags_found,
                            0,
                        )),
                    )
                })
                .collect(),
        ),
    )
}

// Recursive helper ~ should return Node for consistency
fn structured_glosses_go(
    edition: EditionLang,
    gloss_tree: &GlossTree,
    common_short_tags_found: &[Tag],
    level: usize,
) -> Vec<Node> {
    let html_tag = if level == 0 { NTag::Div } else { NTag::Li };
    let mut nested = Vec::new();

    for (gloss, gloss_info) in gloss_tree {
        // Tags/topics that are not common to all glosses (i.e. specific to this gloss)
        let minimal_tags: Vec<_> = gloss_info
            .tags
            .iter()
            .chain(gloss_info.topics.iter())
            .filter(|&tag| !common_short_tags_found.contains(tag))
            .cloned()
            .collect();

        let mut level_content = Node::new_array();

        if let Some(structured_tags) = structured_tags(&minimal_tags, common_short_tags_found) {
            level_content.push(structured_tags);
        }

        level_content.push(Node::Text(gloss.into()));

        if !gloss_info.examples.is_empty() {
            level_content.push(structured_examples(edition, &gloss_info.examples));
        }

        nested.push(wrap(html_tag, "", level_content));

        if gloss_info.children.is_empty() {
            continue;
        }

        // We dont want tags from the parent appearing again in the children
        let mut new_common_short_tags_found = minimal_tags;
        new_common_short_tags_found.extend_from_slice(common_short_tags_found);

        nested.push(wrap(
            NTag::Ul,
            "",
            Node::Array(structured_glosses_go(
                edition,
                &gloss_info.children,
                &new_common_short_tags_found,
                level + 1,
            )),
        ));
    }

    nested
}

fn structured_tags(tags: &[Tag], common_short_tags_found: &[Tag]) -> Option<Node> {
    let structured_tags_content: Vec<_> = tags
        .iter()
        .filter_map(|tag| {
            if let Some(tag_info) = find_tag_in_bank(tag)
                && !common_short_tags_found.contains(&tag_info.short_tag)
            {
                Some(
                    GenericNode {
                        tag: NTag::Span,
                        title: Some(tag_info.long_tag),
                        data: Some(NodeData::from_iter([
                            ("content", "tag"),
                            ("category", &tag_info.category),
                        ])),
                        content: Node::Text(tag_info.short_tag),
                    }
                    .into_node(),
                )
            } else {
                None
            }
        })
        .collect();

    if structured_tags_content.is_empty() {
        None
    } else {
        Some(wrap(
            NTag::Div,
            "tags",
            Node::Array(structured_tags_content),
        ))
    }
}

fn structured_examples(edition: EditionLang, examples: &[Example]) -> Node {
    debug_assert!(!examples.is_empty());

    let mut structured_examples_content = wrap(
        NTag::Summary,
        "summary-entry",
        Node::Text(localize_examples_string(edition, examples.len())),
    )
    .into_array_node();

    for example in examples {
        let mut structured_example_content = wrap(
            NTag::Div,
            "example-sentence-a",
            Node::Text(example.text.clone()),
        )
        .into_array_node();
        if !example.translation.is_empty() {
            structured_example_content.push(wrap(
                NTag::Div,
                "example-sentence-b",
                Node::Text(example.translation.clone()),
            ));
        }
        if !example.reference.is_empty() {
            let reference = example
                .reference
                .strip_suffix(':')
                .unwrap_or(&example.reference)
                .to_string();
            structured_example_content.push(wrap(
                NTag::Div,
                "example-sentence-c",
                Node::Text(reference),
            ));
        }
        structured_examples_content.push(wrap(
            NTag::Div,
            "extra-info",
            wrap(NTag::Div, "example-sentence", structured_example_content),
        ));
    }

    wrap(
        NTag::Details,
        "details-entry-examples",
        structured_examples_content,
    )
}

#[tracing::instrument(skip_all)]
fn to_yomitan_forms(source: Lang, form_map: FormMap) -> Vec<YomitanEntry> {
    form_map
        .flat_iter()
        .map(|(uninflected, inflected, _, _, tags)| {
            // There needs to be DetailedDefinition per tag because yomitan reads
            // multiple tags in a single Inflection as a causal inflection chain.
            let deinflection_definitions: Vec<_> = tags
                .iter()
                .map(|tag| {
                    DetailedDefinition::Inflection((uninflected.to_string(), vec![tag.to_string()]))
                })
                .collect();

            let normalized_inflected = normalize_orthography(source, inflected);
            let reading = if normalized_inflected == *inflected {
                ""
            } else {
                inflected
            };

            YomitanEntry::TermBank(TermBank(
                normalized_inflected,
                reading.into(),
                "non-lemma".into(),
                String::new(),
                deinflection_definitions,
            ))
        })
        .collect()
}
