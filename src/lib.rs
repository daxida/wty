pub mod cli;
pub mod download;
pub mod lang;
pub mod locale;
pub mod models;
pub mod tags;
pub mod utils;

use anyhow::{Context, Ok, Result, bail, ensure};
use fxhash::FxBuildHasher;
use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
#[allow(unused)]
use tracing::{Level, debug, error, info, span, trace, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use unicode_normalization::UnicodeNormalization;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::cli::{ArgsOptions, MainArgs, MainLangs, PathManager};
#[cfg(feature = "html")]
use crate::download::html::download_jsonl;
use crate::lang::{EditionLang, Lang};
use crate::locale::get_locale_examples_string;
use crate::models::kaikki::{Example, Form, HeadTemplate, Pos, Sense, Tag, WordEntry};
use crate::models::yomitan::{
    BacklinkContent, DetailedDefinition, GenericNode, Ipa, NTag, Node, NodeData,
    PhoneticTranscription, TermBank, TermBankMeta, TermPhoneticTranscription, YomitanEntry, wrap,
};
use crate::tags::{
    BLACKLISTED_TAGS, IDENTITY_TAGS, REDUNDANT_TAGS, find_pos, find_tag_in_bank,
    get_tag_bank_as_tag_info, merge_person_tags, remove_redundant_tags, sort_tags,
    sort_tags_by_similar,
};
use crate::utils::{CHECK_C, SKIP_C, pretty_print_at_path, pretty_println_at_path};

pub type Map<K, V> = IndexMap<K, V, FxBuildHasher>; // Preserve insertion order
pub type Set<K> = IndexSet<K, FxBuildHasher>;

// Schema of the main dictionary.
//
// ```text
// +--------------------+-----------------------------------------------+--------------------------------------+
// | Step               | En edition                                    | Non-En edition                       |
// |                    | (target = en)                                 |                                      |
// +--------------------+-----------------------------------------------+--------------------------------------+
// | Download           | <source>-<target>-extract.jsonl               | <target>-extract.jsonl               |
// +--------------------+-----------------------------------------------+--------------------------------------+
// | Filter             | unchanged (already filtered)                  | <source>-<target>-extract.jsonl      |
// |                    |                                               |                                      |
// |                    | if filter params are provided:                |                                      |
// |                    | → <source>-<target>-extract.tmp.jsonl         |                                      |
// |                    |-----------------------------------------------+--------------------------------------|
// |                    | The filtered file path (either .jsonl or .tmp.jsonl) is passed to Tidy.              |
// +--------------------+-----------------------------------------------+--------------------------------------+
// | Tidy (common)      | output to temp/tidy or kept in memory                                                |
// +--------------------+--------------------------------------------------------------------------------------+
// | Yomitan (common)   | output to temp/dict or directly to .zip                                              |
// +--------------------+--------------------------------------------------------------------------------------+
// ```

const CONSOLE_PRINT_INTERVAL: i32 = 10000;

/// Filter by source language iso and other input-given key-value pairs.
///
/// For the English edition, it is a bit tricky. The downloaded jsonl is already filtered. We
/// skip filtering again, unless we are given extra filter parameters.
///
/// In that case, we write a new filtered tmp.jsonl file, and we return its path, different from
/// the default <source>-<target>.extract.jsonl, to be passed to the tidy function.
#[tracing::instrument(skip_all, fields(source = %source))]
fn filter_jsonl(
    edition: EditionLang,
    source: Lang,
    options: &ArgsOptions,
    path_jsonl_raw: &Path,
    path_jsonl: PathBuf,
) -> Result<PathBuf> {
    // English edition already gives them filtered.
    // Yet don't skip if we have filter arguments (forced filtering).
    if matches!(edition, EditionLang::En) && !options.has_filter_params() {
        println!("{SKIP_C} Skipping filtering: English edition detected, with no extra filters");
        return Ok(path_jsonl);
    }

    // rust default is 8 * (1 << 10) := 8KB
    let capacity = 256 * (1 << 10);

    let reader_path = path_jsonl_raw;
    let reader_file = File::open(reader_path)?;
    let mut reader = BufReader::with_capacity(capacity, reader_file);

    let writer_path = match edition {
        EditionLang::En => path_jsonl.with_extension("tmp.jsonl"),
        _ => path_jsonl,
    };
    debug_assert_ne!(reader_path, writer_path);
    let writer_file = File::create(&writer_path)?;
    let mut writer = BufWriter::with_capacity(capacity, writer_file);
    debug!("Filtering: {reader_path:?} > {writer_path:?}",);

    let mut line_count = 1;
    let mut extracted_lines_counter = 0;
    let mut printed_progress = false;

    let mut line = String::with_capacity(1 << 10);

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break; // EOF
        }

        line_count += 1;

        let word_entry: WordEntry =
            serde_json::from_str(&line).with_context(|| "Error decoding JSON @ filter")?;

        if line_count % CONSOLE_PRINT_INTERVAL == 0 {
            printed_progress = true;
            print!("Processed {line_count} lines...\r");
            std::io::stdout().flush()?;
        }

        if line_count == options.first {
            break;
        }

        if options
            .reject
            .iter()
            .any(|(k, v)| k.field_value(&word_entry) == v)
        {
            continue;
        }

        if !options
            .filter
            .iter()
            .all(|(k, v)| k.field_value(&word_entry) == v)
        {
            continue;
        }

        extracted_lines_counter += 1;
        writer.write_all(line.as_bytes())?;
    }

    if printed_progress {
        println!();
    }

    writer.flush()?;

    if fs::metadata(&writer_path)?.len() == 0 {
        fs::remove_file(&writer_path)?;
        bail!("no valid entries for these filters. Exiting.");
    }

    pretty_println_at_path(
        &format!("{CHECK_C} Filtered {extracted_lines_counter} lines out of {line_count}"),
        &writer_path,
    );

    Ok(writer_path)
}

// Tidy: internal types

type LemmaMap = Map<
    String, // lemma
    Map<
        String, // reading
        Map<
            Pos, // pos
            Map<
                String,        // etymology number
                RawSenseEntry, // ipa, gloss_tree etc.
            >,
        >,
    >,
>;

// Note that the order is inverted when converted to a Yomitan entry.
//
// I assume it was done this way to simplify the FormMap visualization.
//
// Example entry in FormMap:
//
// "uninflected": {
//   "inflected": {
//     "verb": [
//       "inflection",
//       [
//         "masculine"
//       ]
//     ]
//   }
// }
//
// Matching YomitanEntry:
//
// [
//   "inflected",       <- lemma, what we search in the dictionary
//   "",
//   "non-lemma",
//   "",
//   0,
//   [
//     [
//       "uninflected", <- form, where we are redirected
//       [
//         "masculine"
//       ]
//     ]
//   ],
//   0,
//   ""
// ]
type FormMap = Map<
    String, // uninflected ~ form
    Map<
        String, // inflected ~ lemma
        Map<
            Pos, // pos
            // Vec<String>, // inflections (tags really)
            (FormSource, Vec<String>), // (source, inflections (tags really))
        >,
    >,
>;

/// Iterates over: uninflected, inflected, pos, source, tags
fn flat_iter_forms(
    form_map: &FormMap,
) -> impl Iterator<Item = (&String, &String, &Pos, &FormSource, &Vec<String>)> {
    form_map.iter().flat_map(|(uninfl, infl_map)| {
        infl_map.iter().flat_map(move |(infl, pos_map)| {
            pos_map
                .iter()
                .map(move |(pos, (source, tags))| (uninfl, infl, pos, source, tags))
        })
    })
}

/// Iterates over: uninflected, inflected, pos, source, tags
fn flat_iter_forms_mut(
    form_map: &mut FormMap,
) -> impl Iterator<Item = (&String, &String, &Pos, &mut FormSource, &mut Vec<String>)> {
    form_map.iter_mut().flat_map(|(uninfl, infl_map)| {
        infl_map.iter_mut().flat_map(move |(infl, pos_map)| {
            pos_map
                .iter_mut()
                .map(move |(pos, (source, tags))| (uninfl, infl, pos, source, tags))
        })
    })
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

fn lemma_map_len(lemma_map: &LemmaMap) -> usize {
    lemma_map
        .values()
        .flat_map(|reading_map| reading_map.values())
        .flat_map(|pos_map| pos_map.values())
        .map(Map::len)
        .sum()
}

fn form_map_len(form_map: &FormMap) -> usize {
    flat_iter_forms(form_map).count()
}

fn form_map_len_of_source(form_map: &FormMap, source: FormSource) -> usize {
    flat_iter_forms(form_map)
        .filter(|(_, _, _, src, _)| **src == source)
        .count()
}

fn form_map_len_extracted(form_map: &FormMap) -> usize {
    form_map_len_of_source(form_map, FormSource::Extracted)
}

fn form_map_len_inflection(form_map: &FormMap) -> usize {
    form_map_len_of_source(form_map, FormSource::Inflection)
}

fn form_map_len_alt_of(form_map: &FormMap) -> usize {
    form_map_len_of_source(form_map, FormSource::AltOf)
}

// Lemmainfo in the original
//
// NOTE: the less we have here the better. For example, the links could be entirely moved to the
// yomitan side of things. It all depends on what we may or may not consider useful for debugging.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct RawSenseEntry {
    #[serde(rename = "glossTree")]
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
    examples: Vec<Example>,

    #[serde(skip_serializing_if = "Map::is_empty")]
    children: GlossTree,
}

/// Intermediate representation: used for snapshots and debugging.
#[derive(Debug, Default)]
struct Tidy {
    lemma_map: LemmaMap,
    form_map: FormMap,
}

impl Tidy {
    // This is usually called at the end, so it could just move the arguments...
    fn insert_lemma_entry(&mut self, lemma: &str, reading: &str, pos: &str, entry: RawSenseEntry) {
        let etym_map = self
            .lemma_map
            .entry(lemma.to_string())
            .or_default()
            .entry(reading.to_string())
            .or_default()
            .entry(pos.to_string())
            .or_default();

        let next_etymology_number = etym_map.len().to_string();

        etym_map.insert(next_etymology_number, entry);
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
        let entry = self
            .form_map
            .entry(uninflected.to_string())
            .or_default()
            .entry(inflected.to_string())
            .or_default()
            .entry(pos.to_string())
            .or_insert_with(|| (source, Vec::new()));

        entry.1.extend(tags);
    }
}

fn postprocess_forms(form_map: &mut FormMap) {
    for (_, _, _, _, tags) in flat_iter_forms_mut(form_map) {
        // Keep only unique tags
        let mut seen = IndexSet::new();
        seen.extend(tags.drain(..));
        *tags = seen.into_iter().collect();

        // Merge person tags and sort
        *tags = merge_person_tags(tags);
        sort_tags_by_similar(tags);
        remove_redundant_tags(tags);
    }
}

#[tracing::instrument(skip_all)]
fn tidy(
    langs: &MainLangs,
    options: &ArgsOptions,
    pm: &PathManager,
    path_jsonl: &Path,
) -> Result<Tidy> {
    if !path_jsonl.exists() {
        bail!("{:?} does not exist @ tidy", path_jsonl.display())
    }

    debug!("Reading jsonlines @ {}", path_jsonl.display());

    let ret = tidy_run(langs, options, path_jsonl)?;

    let n_lemmas = lemma_map_len(&ret.lemma_map);
    let n_forms = form_map_len(&ret.form_map);
    let n_forms_inflection = form_map_len_inflection(&ret.form_map);
    let n_forms_extracted = form_map_len_extracted(&ret.form_map);
    let n_forms_alt_of = form_map_len_alt_of(&ret.form_map);
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

    if options.save_temps {
        debug!("Writing Tidy result to disk");
        write_tidy(options, pm, &ret)?;
    }

    Ok(ret)
}

static PARENS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(.+?\)").unwrap());

#[tracing::instrument(skip_all)]
fn tidy_run(langs: &MainLangs, options: &ArgsOptions, reader_path: &Path) -> Result<Tidy> {
    let (edition, source, target) = (langs.edition, langs.source, langs.target);
    debug_assert_eq!(edition, target);

    let mut ret = Tidy::default();

    let reader_file = File::open(reader_path)?;
    let mut reader = BufReader::new(reader_file);
    let mut line = String::with_capacity(1 << 10);

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break; // EOF
        }

        let mut word_entry: WordEntry =
            serde_json::from_str(&line).with_context(|| "Error decoding JSON @ tidy")?;

        // rg searchword
        // debug (with only relevant, as in, deserialized, information)
        // if matches!(edition, EditionLang::Ja) && word_entry.word == "立命" {
        //     warn!("{:?}", langs);
        //     warn!("{}", get_link_kaikki(edition, source, &word_entry.word));
        //     warn!("{}", serde_json::to_string_pretty(&word_entry)?);
        // }

        // Everything that mutates word_entry
        preprocess_word_entry(source, target, options, &mut word_entry, &mut ret);

        process_forms(&word_entry, &mut ret);

        process_alt_forms(&word_entry, &mut ret);

        // Don't push a lemma if the word_entry has no glosses (f.e. if it is an inflection etc.)
        if contains_no_gloss(&word_entry) {
            process_no_gloss(target, &word_entry, &mut ret);
            continue;
        }

        // rg insertlemma handleline
        let reading = get_reading(edition, source, &word_entry);
        if let Some(raw_sense_entry) = process_word_entry(edition, source, &word_entry) {
            debug_assert!(!raw_sense_entry.gloss_tree.is_empty());
            ret.insert_lemma_entry(&word_entry.word, &reading, &word_entry.pos, raw_sense_entry);
        }
    }

    postprocess_forms(&mut ret.form_map);

    Ok(ret)
}

// Everything that mutates word_entry
fn preprocess_word_entry(
    source: Lang,
    target: EditionLang,
    options: &ArgsOptions,
    word_entry: &mut WordEntry,
    ret: &mut Tidy,
) {
    // WARN: mutates word_entry::pos
    //
    // The whole point being displaying a better tag.
    //
    // https://github.com/tatuylonen/wiktextract/pull/1489
    // if word_entry.pos == "verb" && word_entry.tags.iter().any(|t| t == "participle") {
    //     word_entry.pos = "participle".to_string();
    // }

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

    // WARN: mutates word_entry::senses::sense::tags
    //
    // [en]
    // the original fetched them from head_templates but it is better not to touch that
    // and we can do the same by looking at the tags of the canonical form.
    if matches!(target, EditionLang::En) {
        let tag_matches = [
            "perfective",
            "imperfective",
            "masculine",
            "feminine",
            "neuter",
            "inanimate",
            "animate",
        ];

        if let Some(cform) = get_canonical_form(word_entry) {
            let cform_tags: Vec<_> = cform.tags.clone();
            for sense in &mut word_entry.senses {
                for tag in &cform_tags {
                    if tag_matches.contains(&tag.as_str()) && !sense.tags.contains(tag) {
                        sense.tags.push(tag.into());
                    }
                }
            }
        }
    }

    // WARN: mutates word_entry::senses::sense::tags
    //
    // [ru]
    // This is a good idea that should probably go to every language where it makes sense.
    // Below there is a "safest" version for Greek (where the tags that we propagate are narrowed).
    if matches!(target, EditionLang::Ru) {
        for sense in &mut word_entry.senses {
            for tag in &word_entry.tags {
                if !sense.tags.contains(tag) {
                    sense.tags.push(tag.into());
                }
            }
        }
    }

    // WARN: mutates word_entry::senses::sense::tags
    //
    // [el] Fetch gender from a matching form
    if matches!(target, EditionLang::El) {
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
        if is_inflection_gloss(target, word_entry, &sense)
            && (!options.experimental || get_non_trivial_forms(word_entry).next().is_none())
        {
            handle_inflection_gloss(source, target, word_entry, &sense, ret);
        } else {
            senses_without_inflections.push(sense);
        }
    }
    word_entry.senses = senses_without_inflections;
}

fn get_non_trivial_forms(word_entry: &WordEntry) -> impl Iterator<Item = &Form> {
    word_entry.forms.iter().filter(move |form| {
        if form.form == word_entry.word {
            return false;
        }

        // blacklisted forms (happens at least in English)
        // Usually it has the meaning of "empty cell" in an inflection table
        if form.form == "-" {
            return false;
        }

        // https://github.com/tatuylonen/wiktextract/issues/1494
        // this should fix it, but it is hacky
        // * wait until the editor's answer: https://en.wiktionary.org/wiki/User_talk:Saltmarsh
        //   in case they fix the template and this is not needed.
        // if matches!(args.source, Lang::El) {
        //     if form.form == "ο" || form.form == "η" {
        //         return false;
        //     }
        // }

        // blacklisted tags (happens at least in Russian: romanization)
        let is_blacklisted = form
            .tags
            .iter()
            .any(|tag| BLACKLISTED_TAGS.contains(&tag.as_str()));
        let is_identity = form
            .tags
            .iter()
            .all(|tag| IDENTITY_TAGS.contains(&tag.as_str()));
        if is_blacklisted || is_identity {
            return false;
        }

        true
    })
}

/// Add Extracted forms. That is, forms from `word_entry.forms`.
fn process_forms(word_entry: &WordEntry, ret: &mut Tidy) {
    for form in get_non_trivial_forms(word_entry) {
        let mut filtered_tags: Vec<Tag> = form
            .tags
            .clone()
            .into_iter()
            .filter(|tag| !REDUNDANT_TAGS.contains(&tag.as_str()))
            .collect();
        if filtered_tags.is_empty() {
            continue;
        }

        sort_tags(&mut filtered_tags);

        ret.insert_form(
            &word_entry.word,
            &form.form,
            &word_entry.pos,
            FormSource::Extracted,
            vec![filtered_tags.join(" ")],
        );
    }
}

/// Add `AltOf` forms. That is, alternative forms.
fn process_alt_forms(word_entry: &WordEntry, ret: &mut Tidy) {
    let base_tags = vec!["alt-of".to_string()];

    for alt_form in &word_entry.alt_of {
        ret.insert_form(
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
            ret.insert_form(
                &word_entry.word,
                &alt_form.word,
                &word_entry.pos,
                FormSource::AltOf,
                sense_tags.clone(),
            );
        }
    }
}

/// Check if a `word_entry` contains no glosses.
///
/// Happens if there are no senses, or if there is a single sense with the "no-gloss" tag.
fn contains_no_gloss(word_entry: &WordEntry) -> bool {
    match word_entry.senses.as_slice() {
        [] => true,
        [sense] => sense.tags.iter().any(|tag| tag == "no-gloss"),
        _ => false,
    }
}

/// Process "no-gloss" word entries for alternative ways of adding lemmas/forms.
fn process_no_gloss(target: EditionLang, word_entry: &WordEntry, ret: &mut Tidy) {
    match target {
        // Unfortunately we are in the same A from B, B from C situation discussed in
        // preprocess_word_entry. There is no easy solution for adding the lemma back because at
        // this point the gloss has been deleted. Maybe reconsider the original approach of
        // deleting glosses, and mark them somehow as "inflection-only".
        //
        // At any rate, this will still add useful redirections.
        EditionLang::El => {
            // This is how Kaikki stores participles (μετοχές). Cf. preprocess_word_entry
            if word_entry.pos == "verb"
                && word_entry.tags.iter().any(|t| t == "participle")
                && let Some(form_of) = word_entry.form_of.first()
            {
                ret.insert_form(
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

// There are potentially more than one, but I haven't seen that happen
fn get_reading(edition: EditionLang, source: Lang, word_entry: &WordEntry) -> String {
    match (edition, source) {
        (EditionLang::Ja, _) => match get_transliteration_form(word_entry) {
            Some(form) => form.form.clone(),
            None => word_entry.word.clone(),
        },
        (EditionLang::En, Lang::Ja) => get_japanese_reading(word_entry),
        (EditionLang::En, Lang::Fa) => match get_romanization_form(word_entry) {
            Some(form) => form.form.clone(),
            None => word_entry.word.clone(),
        },
        _ => get_canonical_word(source, word_entry).to_string(),
    }
}

/// The canonical word may contain extra diacritics.
///
/// For most languages, this is equal to word, but for, let's say, Latin, there may be a
/// difference (cf. <https://en.wiktionary.org/wiki/fama>, where `word_entry.word` is fama, but
/// this will return fāma).
fn get_canonical_word(source: Lang, word_entry: &WordEntry) -> &str {
    match source {
        Lang::La | Lang::Ru | Lang::Grc => match get_canonical_form(word_entry) {
            Some(cform) => &cform.form,
            None => &word_entry.word,
        },
        _ => &word_entry.word,
    }
}

/// Return all non-empty forms that contain all given tags.
fn get_tagged_forms<'a>(
    word_entry: &'a WordEntry,
    tags: &[&str],
) -> impl Iterator<Item = &'a Form> {
    word_entry.forms.iter().filter(|form| {
        !form.form.is_empty() && tags.iter().all(|tag| form.tags.iter().any(|t| t == tag))
    })
}

/// Return the first non-empty form with the `canonical` tag.
fn get_canonical_form(word_entry: &WordEntry) -> Option<&Form> {
    get_tagged_forms(word_entry, &["canonical"]).next()
}

/// Return the first non-empty form with the `romanization` tag.
fn get_romanization_form(word_entry: &WordEntry) -> Option<&Form> {
    get_tagged_forms(word_entry, &["romanization"]).next()
}

/// Return the first non-empty form with the `transliteration` tag.
fn get_transliteration_form(word_entry: &WordEntry) -> Option<&Form> {
    get_tagged_forms(word_entry, &["transliteration"]).next()
}

// Does not support multiple readings
fn get_japanese_reading(word_entry: &WordEntry) -> String {
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
    if let Some(cform) = get_canonical_form(word_entry)
        && !cform.ruby.is_empty()
    {
        // https://github.com/tatuylonen/wiktextract/issues/1484
        // let mut cform_lemma = cform.form.clone();
        // if cform_lemma != word_entry.word {
        //     warn!(
        //         "Canonical form: '{cform_lemma}' != word: '{}'\n{}\n{}\n\n",
        //         word_entry.word,
        //         get_link_wiktionary(args, &word_entry.word),
        //         get_link_kaikki(args, &word_entry.word),
        //     );
        // } else {
        //     warn!(
        //         "Equal for word: '{}'\n{}\n{}\n\n",
        //         word_entry.word,
        //         get_link_wiktionary(args, &word_entry.word),
        //         get_link_kaikki(args, &word_entry.word),
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
                warn!("Kanji '{}' not found in '{}'", base, cform_lemma);
                return word_entry.word.clone();
            }
        }
        return cform_lemma;
    }

    word_entry.word.clone()
}

// rg: handleline handle_line
fn process_word_entry(
    edition: EditionLang,
    source: Lang,
    word_entry: &WordEntry,
) -> Option<RawSenseEntry> {
    // Reconvert to Option ~ a bit dumb, could deserialize it as Option, but we use defaults
    // at most WordEntry attributes so I think it's better to be consistent
    let etymology_text = if word_entry.etymology_text.is_empty() {
        None
    } else {
        Some(word_entry.etymology_text.clone())
    };

    let gloss_tree = get_gloss_tree(word_entry);
    if gloss_tree.is_empty() {
        return None;
    }

    Some(RawSenseEntry {
        gloss_tree,
        etymology_text,
        head_info_text: get_head_info(&word_entry.head_templates),
        link_wiktionary: get_link_wiktionary(edition, source, &word_entry.word),
        link_kaikki: get_link_kaikki(edition, source, &word_entry.word),
    })
}

// Useful for debugging too
fn get_link_wiktionary(edition: EditionLang, source: Lang, word: &str) -> String {
    format!(
        "https://{}.wiktionary.org/wiki/{}#{}",
        edition,
        word,
        source.long()
    )
}

// Same debug but for kaikki
fn get_link_kaikki(edition: EditionLang, source: Lang, word: &str) -> String {
    let chars: Vec<_> = word.chars().collect();
    let first = chars[0]; // word can't be empty
    let first_two = if chars.len() < 2 {
        word.to_string()
    } else {
        chars[0..2].iter().collect::<String>()
    };
    // 楽しい >> 楽/楽し/楽しい
    // 伸す >> 伸/伸す/伸す (when word.chars().count() < 2)
    // up >> u/up/up (word.len() is irrelevant, only char count matters)
    let search_query = format!("{first}/{first_two}/{word}");
    let dictionary = match edition {
        EditionLang::En => "dictionary".to_string(),
        other => format!("{other}wiktionary"),
    };
    let localized_source = match edition {
        EditionLang::En | EditionLang::El => source.long(),
        // https://github.com/tatuylonen/wiktextract/issues/1497
        _ => "All%20languages%20combined",
    };
    let unescaped_url =
        format!("https://kaikki.org/{dictionary}/{localized_source}/meaning/{search_query}.html");
    unescaped_url.replace(' ', "%20")
}

// default version getphonetictranscription
fn get_ipas(word_entry: &WordEntry) -> Vec<Ipa> {
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

// rg: getheadinfo
fn get_head_info(head_templates: &[HeadTemplate]) -> Option<String> {
    // WARN: cant do lookbehinds in rust!
    for head_template in head_templates {
        let expansion = &head_template.expansion;
        if !expansion.is_empty() && PARENS_RE.is_match(expansion) {
            return Some(expansion.clone());
        }
    }
    None
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
        // Stable sort: examples with translations first
        filtered_examples.sort_by_key(|ex| ex.translation.is_empty());

        insert_glosses(
            &mut gloss_tree,
            &sense.glosses,
            &sense.tags,
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
    examples: &[Example],
) {
    if glosses.is_empty() {
        return;
    }

    let head = &glosses[0];
    let tail = &glosses[1..];

    // get or insert node with only tags at this level
    let node = gloss_tree.entry(head.clone()).or_insert_with(|| GlossInfo {
        tags: tags.to_vec(),
        examples: vec![],
        children: GlossTree::default(),
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

    insert_glosses(&mut node.children, tail, tags, examples);
}

// rg: isinflection
// Should be sense again...
//
// We pass the wordentry too in case the discrimination needs more info
fn is_inflection_gloss(target: EditionLang, _word_entry: &WordEntry, sense: &Sense) -> bool {
    match target {
        EditionLang::De => {
            static RE_INFLECTION_DE: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r" des (Verbs|Adjektivs|Substantivs|Demonstrativpronomens|Possessivpronomens|Pronomens)").unwrap()
            });
            sense
                .glosses
                .iter()
                .any(|gloss| RE_INFLECTION_DE.is_match(gloss))
        }
        EditionLang::El => {
            !sense.form_of.is_empty() && sense.glosses.iter().any(|gloss| gloss.contains("του"))
        }
        EditionLang::En => {
            if sense
                .glosses
                .iter()
                .any(|gloss| gloss.contains("inflection of"))
            {
                return true;
            }
            for form in &sense.form_of {
                if !form.word.is_empty() {
                    // if $ is how JS escapes chars this is awfully wrong...
                    // also, this escape is not the original custom function used in the
                    // original...
                    let pattern = format!(r"of {}($| \(.+?\)$)", regex::escape(&form.word));
                    let re = Regex::new(&pattern).unwrap();
                    if sense.glosses.iter().any(|gloss| re.is_match(gloss)) {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

static DE_INFLECTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(.*)des (?:Verbs|Adjektivs|Substantivs|Demonstrativpronomens|Possessivpronomens|Pronomens) (.*)$"
    ).unwrap()
});

fn handle_inflection_gloss(
    source: Lang,
    target: EditionLang,
    word_entry: &WordEntry,
    sense: &Sense,
    ret: &mut Tidy,
) {
    match target {
        EditionLang::El => {
            const VALID_TAGS: [&str; 9] = [
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

            let allowed_tags: Vec<_> = sense
                .tags
                .iter()
                .filter(|tag| VALID_TAGS.contains(&tag.as_str()))
                .map(std::string::ToString::to_string)
                .collect();
            let inflection_tags: Vec<_> = if allowed_tags.is_empty() {
                vec![format!("redirected from {}", word_entry.word)]
            } else {
                allowed_tags
            };
            for form in &sense.form_of {
                ret.insert_form(
                    &form.word,
                    &word_entry.word,
                    &word_entry.pos,
                    FormSource::Inflection,
                    inflection_tags.clone(),
                );
            }
        }
        EditionLang::En => handle_inflection_gloss_en(source, word_entry, sense, ret),
        EditionLang::De => {
            if sense.glosses.is_empty() {
                return;
            }

            if let Some(caps) = DE_INFLECTION_RE.captures(&sense.glosses[0])
                && let (Some(inflection_tags), Some(uninflected)) = (caps.get(1), caps.get(2))
            {
                let inflection_tags = inflection_tags.as_str().trim();

                if !inflection_tags.is_empty() {
                    ret.insert_form(
                        uninflected.as_str(),
                        &word_entry.word,
                        &word_entry.pos,
                        FormSource::Inflection,
                        vec![inflection_tags.to_string()],
                    );
                }
            }
        }
        _ => (),
    }
}

static EN_LEMMA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"of ([^\s]+)\s*(\(.+?\))?$").unwrap());
static EN_INSIDE_PARENS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\(.+?\)$").unwrap());

// this is awful
//
// tested in the es-en suite
fn handle_inflection_gloss_en(source: Lang, word_entry: &WordEntry, sense: &Sense, ret: &mut Tidy) {
    if sense.glosses.is_empty() {
        return;
    }

    // Split glosses by ##
    let gloss_pieces: Vec<String> = sense
        .glosses
        .iter()
        .flat_map(|gloss| {
            gloss
                .split("##")
                .map(str::trim)
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect();

    let mut lemmas = IndexSet::new();
    let mut inflections = IndexSet::new();

    for mut inflection in gloss_pieces {
        if let Some(caps) = EN_LEMMA_RE.captures(&inflection)
            && let Some(lemma) = caps.get(1)
        {
            lemmas.insert(lemma.as_str().replace(':', "").trim().to_string());
        }

        let lemma = match lemmas.len() {
            0 => continue,
            1 => lemmas.iter().next().unwrap(),
            // If multiple lemmas → ambiguous → stop
            _ => return,
        };

        // Clean up inflection text
        inflection = inflection
            .replace("inflection of ", "")
            .replace(&format!("of {lemma}"), "")
            .replace(lemma, "")
            .replace(':', "")
            .to_string();
        // Remove parenthesized content at the end
        inflection = EN_INSIDE_PARENS_RE
            .replace_all(&inflection, "")
            .trim()
            .to_string();

        if !inflection.is_empty() {
            inflections.insert(inflection);
        }
    }

    let Some(uninflected) = lemmas.iter().next() else {
        return;
    };

    // Not sure if this is better (cf. ru-en) over word_entry.word but it is what was done in
    // the original, so lets not change that for the moment.
    let inflected = get_canonical_word(source, word_entry);

    if inflected == uninflected {
        return;
    }

    for inflection in inflections {
        ret.insert_form(
            uninflected,
            inflected,
            &word_entry.pos,
            FormSource::Inflection,
            vec![inflection],
        );
    }
}

// NOTE: we write stuff even if ret.attribute is empty
//
/// Write a Tidy struct to disk.
///
/// This is effectively a snapshot of our tidy intermediate representation.
#[tracing::instrument(skip_all)]
fn write_tidy(options: &ArgsOptions, pm: &PathManager, ret: &Tidy) -> Result<()> {
    let opath = pm.path_lemmas();
    let file = File::create(&opath)?;
    let writer = BufWriter::new(file);

    if options.pretty {
        serde_json::to_writer_pretty(writer, &ret.lemma_map)?;
    } else {
        serde_json::to_writer(writer, &ret.lemma_map)?;
    }
    pretty_println_at_path("Wrote tidy lemmas", &opath);

    // Forms are written by chunks in the original (cf. mapChunks). Not sure if needed.
    // If I even change that, do NOT hardcode the forms number (i.e. the 0 in ...forms-0.json)
    let opath = pm.path_forms();
    let file = File::create(&opath)?;
    let writer = BufWriter::new(file);

    if options.pretty {
        serde_json::to_writer_pretty(writer, &ret.form_map)?;
    } else {
        serde_json::to_writer(writer, &ret.form_map)?;
    }
    pretty_println_at_path("Wrote tidy forms", &opath);

    Ok(())
}

fn get_index(dict_name: &str, source: Lang, target: Lang) -> String {
    let current_date = chrono::Utc::now().format("%Y-%m-%d");
    format!(
        r#"{{
  "title": "{dict_name}",
  "format": 3,
  "revision": "{current_date}",
  "sequenced": true,
  "author": "Kaikki-to-Yomitan contributors",
  "url": "https://github.com/yomidevs/kaikki-to-yomitan",
  "description": "Dictionaries for various language pairs generated from Wiktionary data, via Kaikki and Kaikki-to-Yomitan.",
  "attribution": "https://kaikki.org/",
  "sourceLanguage": "{source}",
  "targetLanguage": "{target}"
}}"#
    )
}

// Do not distinguish between Tag and Pos (String) to make it more ergonomic.

type Key = String; // A tag or pos
type Word = String; // A word
// Vec of words in which the tag/pos was encountered
type CounterValue = Vec<Word>;
type Counter = Map<Key, CounterValue>;

// For debugging purposes
#[derive(Debug, Default)]
struct Diagnostics {
    /// Tags found in bank
    accepted_tags: Counter,
    /// Tags not found in bank
    rejected_tags: Counter,

    /// POS found in bank
    accepted_pos: Counter,
    /// POS not found in bank
    rejected_pos: Counter,
}

impl Diagnostics {
    fn new() -> Self {
        Self::default()
    }

    fn is_empty(&self) -> bool {
        self.accepted_tags.is_empty()
            && self.rejected_tags.is_empty()
            && self.accepted_pos.is_empty()
            && self.rejected_pos.is_empty()
    }

    fn increment(map: &mut Counter, key: Key, word: Word) {
        map.entry(key).or_default().push(word);
    }

    fn increment_accepted_tag(&mut self, tag: Key, word: Word) {
        Self::increment(&mut self.accepted_tags, tag, word);
    }

    fn increment_rejected_tag(&mut self, tag: Key, word: Word) {
        Self::increment(&mut self.rejected_tags, tag, word);
    }

    fn increment_accepted_pos(&mut self, pos: Key, word: Word) {
        Self::increment(&mut self.accepted_pos, pos, word);
    }

    fn increment_rejected_pos(&mut self, pos: Key, word: Word) {
        Self::increment(&mut self.rejected_pos, pos, word);
    }
}

// For el/en/fr this is trivial ~ Needs lang script for the rest
fn normalize_orthography(source: Lang, term: &str) -> String {
    match source {
        Lang::Grc | Lang::La | Lang::Ru => {
            // Normalize to NFD and drop combining accents
            term.nfd()
                .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
                .collect()
        }
        _ => term.to_string(),
    }
}

// NOTE: do NOT use the json! macro as it does not preserve insertion order

// rg: yomitango yomitan_go
#[tracing::instrument(skip_all)]
fn make_yomitan_lemmas(
    langs: &MainLangs,
    options: &ArgsOptions,
    lemma_map: LemmaMap,
    diagnostics: &mut Diagnostics,
) -> Vec<YomitanEntry> {
    let mut yomitan_entries = Vec::new();

    for (lemma, readings) in lemma_map {
        for (reading, pos_word) in readings {
            for (pos, etyms) in pos_word {
                for (_etym_number, info) in etyms {
                    let yomitan_entry = make_yomitan_lemma(
                        langs,
                        options,
                        &lemma,
                        &reading,
                        &pos,
                        info,
                        diagnostics,
                    );
                    yomitan_entries.push(yomitan_entry);
                }
            }
        }
    }

    yomitan_entries
}

// TODO: consume info
fn make_yomitan_lemma(
    langs: &MainLangs,
    options: &ArgsOptions,
    lemma: &str,
    reading: &str,
    pos: &Pos, // should be &str
    info: RawSenseEntry,
    diagnostics: &mut Diagnostics,
) -> YomitanEntry {
    // rg: findpartofspeech findpos
    let found_pos: String = if let Some(short_pos) = find_pos(pos) {
        if options.save_temps {
            diagnostics.increment_accepted_pos(pos.to_string(), lemma.to_string());
        }
        short_pos
    } else {
        if options.save_temps {
            diagnostics.increment_rejected_pos(pos.to_string(), lemma.to_string());
        }
        pos
    }
    .to_string();

    let yomitan_reading = if *reading == *lemma { "" } else { reading };

    let common_short_tags_recognized =
        get_recognized_tags(options, lemma, pos, &info.gloss_tree, diagnostics);
    let definition_tags = common_short_tags_recognized.join(" ");

    let mut detailed_definition_content = Node::new_array();

    // rg: etymologytext / head_info_text headinfo
    if info.etymology_text.is_some() || info.head_info_text.is_some() {
        let structured_preamble =
            get_structured_preamble(info.etymology_text.as_ref(), info.head_info_text.as_ref());
        detailed_definition_content.push(structured_preamble);
    }

    let structured_glosses = get_structured_glosses(
        langs.target.into(),
        &info.gloss_tree,
        &common_short_tags_recognized,
    );
    detailed_definition_content.push(structured_glosses);

    let backlink = get_structured_backlink(&info.link_wiktionary, &info.link_kaikki, options);
    detailed_definition_content.push(backlink);

    let detailed_definition = DetailedDefinition::structured(detailed_definition_content);

    YomitanEntry::TermBank(TermBank(
        lemma.to_string(),
        yomitan_reading.to_string(),
        definition_tags,
        found_pos,
        0,
        vec![detailed_definition],
        0,
        String::new(),
    ))
}

fn get_recognized_tags(
    options: &ArgsOptions,
    lemma: &str,
    pos: &Pos,
    gloss_tree: &GlossTree,
    diagnostics: &mut Diagnostics,
) -> Vec<Tag> {
    // common tags to all glosses (this is an English edition reasoning really...)
    let common_tags: Vec<Tag> = gloss_tree
        .values()
        .map(|g| IndexSet::from_iter(g.tags.iter().cloned()))
        .reduce(|acc, set| acc.intersection(&set).cloned().collect::<IndexSet<Tag>>())
        .unwrap_or_default() // in case of no glosses
        .into_iter()
        .collect();

    // rg: processtags process_tags
    let mut common_short_tags_recognized: Vec<Tag> = Vec::new();
    // we add pos (at index 0) for this search!
    for tag in std::iter::once(pos).chain(common_tags.iter()) {
        match find_tag_in_bank(tag) {
            None => {
                // try modified tag: skip
                if tag != pos && options.save_temps {
                    diagnostics.increment_rejected_tag(tag.to_string(), lemma.to_string());
                }
            }
            Some(res) => {
                if tag != pos && options.save_temps {
                    diagnostics.increment_accepted_tag(tag.to_string(), lemma.to_string());
                }
                common_short_tags_recognized.push(res.short_tag);
            }
        }
    }
    // Some filtering here: skip
    common_short_tags_recognized
}

fn build_details_entry(ty: &str, content: &str) -> Node {
    let mut summary = wrap(NTag::Summary, "summary-entry", Node::Text(ty.into())).into_array_node();
    let div = wrap(
        NTag::Div,
        &format!("{ty}-content"),
        Node::Text(content.into()),
    );
    summary.push(div);
    wrap(NTag::Details, &format!("details-entry-{ty}"), summary)
}

fn get_structured_preamble(
    etymology_text: Option<&String>,
    head_info_text: Option<&String>,
) -> Node {
    let mut preamble_content = Node::new_array();
    if let Some(head_info_text) = &head_info_text {
        let detail = build_details_entry("Grammar", head_info_text);
        preamble_content.push(detail);
    }
    if let Some(etymology_text) = &etymology_text {
        let detail = build_details_entry("Etymology", etymology_text);
        preamble_content.push(detail);
    }
    let preamble = wrap(NTag::Div, "preamble", preamble_content);

    wrap(NTag::Div, "", preamble.into_array_node())
}

#[allow(unused_variables)]
fn get_structured_backlink(wlink: &str, klink: &str, options: &ArgsOptions) -> Node {
    let mut links = Node::new_array();

    links.push(Node::Backlink(BacklinkContent::new(wlink, "Wiktionary")));

    if options.experimental {
        links.push(Node::Text(" | ".into())); // JMdict does this
        links.push(Node::Backlink(BacklinkContent::new(klink, "Kaikki")));
    }

    wrap(NTag::Div, "backlink", links)
}

// should return Node for consistency
fn get_structured_glosses(
    target: Lang,
    gloss_tree: &GlossTree,
    common_short_tags_recognized: &[Tag],
) -> Node {
    let mut sense_content = Vec::new();
    for (gloss, gloss_info) in gloss_tree {
        let synthetic_branch = GlossTree::from_iter([(gloss.clone(), gloss_info.clone())]);
        let nested_gloss =
            get_structured_glosses_go(target, &synthetic_branch, common_short_tags_recognized, 0);
        let structured_gloss = wrap(NTag::Li, "", Node::Array(nested_gloss));
        sense_content.push(structured_gloss);
    }
    wrap(NTag::Ol, "glosses", Node::Array(sense_content))
}

// Recursive helper
// should return Node for consistency
fn get_structured_glosses_go(
    target: Lang,
    gloss_tree: &GlossTree,
    common_short_tags_recognized: &[Tag],
    level: usize,
) -> Vec<Node> {
    let html_tag = if level == 0 { NTag::Div } else { NTag::Li };
    let mut nested = Vec::new();

    for (gloss, gloss_info) in gloss_tree {
        let level_tags = gloss_info.tags.clone();

        // processglosstags: skip
        let minimal_tags: Vec<_> = level_tags
            .into_iter()
            .filter(|tag| !common_short_tags_recognized.contains(tag))
            .collect();

        let mut level_content = Node::new_array();

        if let Some(structured_tags) =
            get_structured_tags(&minimal_tags, common_short_tags_recognized)
        {
            level_content.push(structured_tags);
        }

        let gloss_content = Node::Text(gloss.into());
        level_content.push(gloss_content);

        if let Some(structured_examples) = get_structured_examples(target, &gloss_info.examples) {
            level_content.push(structured_examples);
        }

        let level_structured = wrap(html_tag, "", level_content);
        nested.push(level_structured);

        if !gloss_info.children.is_empty() {
            // we dont want tags from the parent appearing again in the children
            let mut new_common_short_tags_recognized = common_short_tags_recognized.to_vec();
            new_common_short_tags_recognized.extend(minimal_tags);

            let child_defs = get_structured_glosses_go(
                target,
                &gloss_info.children,
                &new_common_short_tags_recognized,
                level + 1,
            );
            let structured_child_defs = wrap(NTag::Ul, "", Node::Array(child_defs));
            nested.push(structured_child_defs);
        }
    }

    nested
}

fn get_structured_tags(tags: &[Tag], common_short_tags_recognized: &[Tag]) -> Option<Node> {
    let mut structured_tags_content = Vec::new();

    for tag in tags {
        let Some(full_tag) = find_tag_in_bank(tag) else {
            continue;
        };

        // minimaltags
        // HACK: the conversion to short tag is done differently in the original
        let short_tag = full_tag.short_tag;

        if common_short_tags_recognized.contains(&short_tag) {
            // We dont want "masculine" appear twice...
            continue;
        }

        let structured_tag_content = GenericNode {
            tag: NTag::Span,
            title: Some(full_tag.long_tag),
            data: Some(NodeData::from_iter([
                ("content", "tag"),
                ("category", &full_tag.category),
            ])),
            content: Node::Text(short_tag),
        }
        .into_node();

        structured_tags_content.push(structured_tag_content);
    }

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

fn get_structured_examples(target: Lang, examples: &[Example]) -> Option<Node> {
    if examples.is_empty() {
        return None;
    }

    let mut structured_examples_content = wrap(
        NTag::Summary,
        "summary-entry",
        Node::Text(get_locale_examples_string(&target, examples.len())),
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
            let structured_translation_content = wrap(
                NTag::Div,
                "example-sentence-b",
                Node::Text(example.translation.clone()),
            );
            structured_example_content.push(structured_translation_content);
        }
        let structured_example_content_wrap = wrap(
            NTag::Div,
            "extra-info",
            wrap(NTag::Div, "example-sentence", structured_example_content),
        );
        structured_examples_content.push(structured_example_content_wrap);
    }

    Some(wrap(
        NTag::Details,
        "details-entry-examples",
        structured_examples_content,
    ))
}

#[tracing::instrument(skip_all)]
fn make_yomitan_forms(source: Lang, form_map: FormMap) -> Vec<YomitanEntry> {
    let mut yomitan_entries = Vec::new();

    for (uninflected, inflected, _pos, _source, tags) in flat_iter_forms(&form_map) {
        // There was some hypotheses lingo here in the original that I didn't fully understand
        // and it didn't seem to do anything for the testsuite...

        // NOTE: There needs to be DetailedDefinition per tag because yomitan reads multiple tags
        // in a single Inflection as a causal inflection chain.
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

        let yomitan_entry = YomitanEntry::TermBank(TermBank(
            normalized_inflected,
            reading.into(),
            "non-lemma".into(),
            String::new(),
            0,
            deinflection_definitions,
            0,
            String::new(),
        ));

        yomitan_entries.push(yomitan_entry);
    }

    yomitan_entries
}

fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);
    serde_json::from_reader(reader)
        .with_context(|| format!("Failed to parse JSON @ {}", path.display()))
}

// Return default in case of path not found
//
// was used for tag_banks/pos that are language-specific but we use English everywhere so...
//
// fn load_json_optional<T>(path: &Path) -> Result<T>
// where
//     T: DeserializeOwned + Default + Sized,
// {
//     if path.exists() {
//         load_json(path)
//     } else {
//         debug!("Optional file not found @ {}", path.display());
//         Ok(T::default())
//     }
// }

/// Write index, entries (term banks), styles.css and tag banks.
#[tracing::instrument(skip_all)]
fn make_yomitan(
    langs: &MainLangs,
    options: &ArgsOptions,
    pm: &PathManager,
    diagnostics: &mut Diagnostics,
    tidy_cache: Option<Tidy>,
) -> Result<()> {
    println!("Making yomitan dict...");

    let (lemma_map, form_map) = if let Some(ret) = tidy_cache {
        debug!("Skip loading Tidy result: passing it instead");
        (ret.lemma_map, ret.form_map)
    } else {
        debug!("Loading Tidy result from disk");
        let lemmas_path = pm.path_lemmas();
        let forms_path = pm.path_forms();

        ensure!(
            lemmas_path.exists() && forms_path.exists(),
            "can not proceed with make_yomitan. Are you running --skip-tidy without --save-temps?"
        );

        let lemma_map: LemmaMap = load_json(&lemmas_path)?;
        let form_map: FormMap = load_json(&forms_path)?;
        (lemma_map, form_map)
    };

    let yomitan_entries = make_yomitan_lemmas(langs, options, lemma_map, diagnostics);
    let yomitan_forms = make_yomitan_forms(langs.source, form_map);
    let labelled_entries = [("lemma", yomitan_entries), ("form", yomitan_forms)];

    write_yomitan(
        langs.source,
        langs.target.into(),
        options,
        pm,
        &labelled_entries,
    )
}

const STYLES_CSS: &[u8] = include_bytes!("../assets/styles.css");
const STYLES_CSS_EXPERIMENTAL: &[u8] = include_bytes!("../assets/styles_experimental.css");

/// Write lemma / form / whatever banks to either disk or zip.
///
/// If `save_temps` is true, we assume that the user is debugging and does not need the zip.
fn write_yomitan(
    source: Lang,
    target: Lang,
    options: &ArgsOptions,
    pm: &PathManager,
    labelled_entries: &[(&str, Vec<YomitanEntry>)],
) -> Result<()> {
    let mut bank_index = 0;

    if options.save_temps {
        let out_dir = pm.dir_temp_dict();
        fs::create_dir_all(&out_dir)?;
        for (entry_ty, entries) in labelled_entries {
            write_banks(
                options.pretty,
                entries,
                &mut bank_index,
                entry_ty,
                &out_dir,
                BankSink::Disk,
            )?;
        }

        pretty_println_at_path(&format!("{CHECK_C} Wrote temp data"), &out_dir);
    } else {
        let writer_path = pm.path_dict();
        let writer_file = File::create(&writer_path)?;
        let mut zip = ZipWriter::new(writer_file);
        let zip_options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        // Zip index.json
        let index_string = get_index(&pm.dict_name_expanded(), source, target);
        zip.start_file("index.json", zip_options)?;
        zip.write_all(index_string.as_bytes())?;

        // Copy paste styles.css
        zip.start_file("styles.css", zip_options)?;

        if options.experimental {
            zip.write_all(STYLES_CSS_EXPERIMENTAL)?;
        } else {
            zip.write_all(STYLES_CSS)?;
        }

        // Copy paste tag_bank.json
        let tag_bank = get_tag_bank_as_tag_info();
        let tag_bank_bytes = serde_json::to_vec_pretty(&tag_bank)?;
        zip.start_file("tag_bank_1.json", zip_options)?; // it needs to end in _1
        zip.write_all(&tag_bank_bytes)?;

        for (entry_ty, entries) in labelled_entries {
            write_banks(
                options.pretty,
                entries,
                &mut bank_index,
                entry_ty,
                &writer_path,
                BankSink::Zip(&mut zip, zip_options),
            )?;
        }

        zip.finish()?;

        pretty_println_at_path(&format!("{CHECK_C} Wrote yomitan dict"), &writer_path);
    }

    Ok(())
}

enum BankSink<'a> {
    Disk,
    Zip(&'a mut ZipWriter<File>, SimpleFileOptions),
}

/// Writes `yomitan_entries` in batches to `out_sink` (either disk or a zip).
#[tracing::instrument(skip_all)]
fn write_banks(
    pretty: bool,
    yomitan_entries: &[YomitanEntry],
    bank_index: &mut usize,
    entry_ty: &str,
    out_dir: &Path,
    mut out_sink: BankSink,
) -> Result<()> {
    let bank_size = 25_000;
    let total_entries = yomitan_entries.len();
    let total_bank_num = total_entries.div_ceil(bank_size);

    let mut bank_num = 0;
    let mut start = 0;

    while start < total_entries {
        *bank_index += 1;
        bank_num += 1;

        let end = (start + bank_size).min(total_entries);
        let bank = &yomitan_entries[start..end];
        let upto = end - start;

        // NOTE: should be passed as argument?
        // NOTE: this assumes that once a type is passed, all the remaining entries are of same type
        //
        // SAFETY:
        // * if end = start + bank_size, then end > start (bank_size > 0)
        // * if end = total_entries    , then end > start (while condition)
        // In both cases end > start, so there is always a bank.first();
        let bank_name_prefix = match bank.first().unwrap() {
            YomitanEntry::TermBank(_) => "term_bank",
            YomitanEntry::TermBankMeta(_) => "term_meta_bank",
        };

        let bank_name = format!("{bank_name_prefix}_{bank_index}.json");
        let file_path = out_dir.join(&bank_name);

        let json_bytes = if pretty {
            serde_json::to_vec_pretty(&bank)?
        } else {
            serde_json::to_vec(&bank)?
        };

        match out_sink {
            BankSink::Disk => {
                let mut file = File::create(&file_path)?;
                file.write_all(&json_bytes)?;
            }
            BankSink::Zip(ref mut zip, zip_options) => {
                zip.start_file(&bank_name, zip_options)?;
                zip.write_all(&json_bytes)?;
            }
        }

        if bank_num > 1 {
            print!("\r\x1b[K");
        }
        pretty_print_at_path(
            &format!("Wrote yomitan {entry_ty} bank {bank_num}/{total_bank_num} ({upto} entries)"),
            &file_path,
        );
        std::io::stdout().flush()?;

        start = end;
    }

    if bank_num == total_bank_num {
        println!();
    }

    Ok(())
}

impl SimpleDictionary for DGlossary {
    type I = YomitanEntry; // default: when we don't want tidy

    fn process(
        &self,
        edition: EditionLang,
        _source: Lang,
        target: Lang,
        entry: &WordEntry,
    ) -> Vec<Self::I> {
        make_yomitan_entries_glossary(edition, target, entry)
    }

    fn paths_jsonl_raw(&self, pm: &PathManager) -> Vec<(EditionLang, PathBuf)> {
        let (edition, source, _) = pm.langs();
        let edition_lang = edition.try_into().unwrap(); // edition is never Edition::All
        vec![(edition_lang, pm.path_jsonl(source, source))]
    }

    fn to_yomitan(&self, tidy: Self::I) -> YomitanEntry {
        tidy
    }
}

impl SimpleDictionary for DGlossaryExtended {
    type I = IGlossaryExtended;

    fn paths_jsonl_raw(&self, pm: &PathManager) -> Vec<(EditionLang, PathBuf)> {
        let (edition, _, _) = pm.langs();
        let mut paths = Vec::new();
        for edition_lang in edition.variants() {
            let lang = edition_lang.into();
            paths.push((edition_lang, pm.path_jsonl(lang, lang)));
        }
        paths
    }

    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        target: Lang,
        entry: &WordEntry,
    ) -> Vec<Self::I> {
        make_ir_glossary_extended(edition, source, target, entry)
    }

    fn write_temp(&self) -> bool {
        true
    }

    fn compact(&self, tidies: &mut Vec<Self::I>) {
        let mut map = Map::default();

        for (lemma, pos, edition, translations) in tidies.drain(..) {
            let entry = map
                .entry(lemma.clone())
                .or_insert_with(|| (pos.clone(), edition, Set::default()));

            for tr in translations {
                entry.2.insert(tr);
            }
        }

        tidies.extend(map.into_iter().map(|(lemma, (pos, edition, set))| {
            (lemma, pos, edition, set.into_iter().collect::<Vec<_>>())
        }));
    }

    fn to_yomitan(&self, tidy: Self::I) -> YomitanEntry {
        make_yomitan_glossary_extended(tidy)
    }
}

impl SimpleDictionary for DIpa {
    type I = IIpa;

    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        _target: Lang,
        entry: &WordEntry,
    ) -> Vec<Self::I> {
        make_ir_ipa(edition, source, entry)
    }

    fn paths_jsonl_raw(&self, pm: &PathManager) -> Vec<(EditionLang, PathBuf)> {
        let (edition, source, target) = pm.langs();
        let edition_lang = edition.try_into().unwrap(); // edition is never Edition::All
        vec![(edition_lang, pm.path_jsonl(source, target))]
    }

    fn to_yomitan(&self, tidy: Self::I) -> YomitanEntry {
        make_yomitan_ipa(tidy)
    }
}

impl SimpleDictionary for DIpaMerged {
    type I = IIpa;

    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        _target: Lang,
        entry: &WordEntry,
    ) -> Vec<Self::I> {
        make_ir_ipa(edition, source, entry)
    }

    fn paths_jsonl_raw(&self, pm: &PathManager) -> Vec<(EditionLang, PathBuf)> {
        let (edition, _, _) = pm.langs();
        let mut paths = Vec::new();
        for edition_lang in edition.variants() {
            let lang = edition_lang.into();
            paths.push((edition_lang, pm.path_jsonl(lang, lang)));
        }
        paths
    }

    fn compact(&self, irs: &mut Vec<Self::I>) {
        // Keep only unique entries
        let mut seen = IndexSet::new();
        seen.extend(irs.drain(..));
        *irs = seen.into_iter().collect();
        // Sorting is not needed ~ just for visibility
        irs.sort_by(|a, b| a.0.cmp(&b.0));
    }

    fn to_yomitan(&self, tidy: Self::I) -> YomitanEntry {
        make_yomitan_ipa(tidy)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DGlossary;

#[derive(Debug, Clone, Copy)]
pub struct DGlossaryExtended;

#[derive(Debug, Clone, Copy)]
pub struct DIpa;

#[derive(Debug, Clone, Copy)]
pub struct DIpaMerged;

// Ideally this should support Main at some point
//
// If this ends up having too much overhead for dictionaries that do not use Self::I, consider this
// other trait:
//
// trait SimpleDictionary {
//     fn paths_jsonl_raw(&self, pm: &PathManager) -> Vec<(EditionLang, PathBuf)>;
//     fn process(&self, source: Lang, target: Lang, entry: &WordEntry) -> Vec<YomitanEntry>;
// }
//
// and rewrite write_simple_dict to instead just store YomitanEntries.
//
/// Trait to abstract the process of writing a dictionary.
pub trait SimpleDictionary {
    /// Intermediate representation. Used for postprocessing (merge, etc.) and debugging via snapshots.
    ///
    /// It can be set to `YomitanEntry` if we don't want to do anything fancy.
    type I: Serialize;

    /// Vector of paths to jsonl raw dumps.
    ///
    /// Most dictionaries only use a single path. For instance, Glossary will only use the `source`
    /// edition.
    fn paths_jsonl_raw(&self, pm: &PathManager) -> Vec<(EditionLang, PathBuf)>;

    /// How to transform a `WordEntry` into intermediate representation.
    ///
    /// Most dictionaries only make *at most one* `Self::I` from a `WordEntry`.
    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        target: Lang,
        word_entry: &WordEntry,
    ) -> Vec<Self::I>;

    /// Whether to write or not the Vec<Self::I> to disk
    fn write_temp(&self) -> bool {
        false
    }

    /// How to compact the intermediate representation.
    ///
    /// This can be implemented, for instance, to merge entries from different editions.
    ///
    /// If left unimplemented, it does nothing.
    #[allow(unused_variables)]
    fn compact(&self, irs: &mut Vec<Self::I>) {}

    // Does not have access to WordEntry!
    fn to_yomitan(&self, ir: Self::I) -> YomitanEntry;
}

pub fn make_simple_dict<D: SimpleDictionary>(
    dict: D,
    options: &ArgsOptions,
    pm: &PathManager,
) -> Result<()> {
    let (_, source, target) = pm.langs();

    pm.setup_dirs()?;

    // rust default is 8 * (1 << 10) := 8KB
    let capacity = 256 * (1 << 10);
    let mut line = String::with_capacity(1 << 10);
    let mut entries = Vec::new();

    for (edition, path_jsonl_raw) in dict.paths_jsonl_raw(pm) {
        #[cfg(feature = "html")]
        {
            download_jsonl(edition, source, &path_jsonl_raw, options.redownload)?;
        }

        let reader_path = path_jsonl_raw;
        let reader_file = File::open(&reader_path)?;
        let mut reader = BufReader::with_capacity(capacity, reader_file);

        let mut line_count = 1;
        let mut printed_progress = false;

        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break; // EOF
            }

            line_count += 1;

            let word_entry: WordEntry =
                serde_json::from_str(&line).with_context(|| "Error decoding JSON @ filter")?;

            if line_count % CONSOLE_PRINT_INTERVAL == 0 {
                printed_progress = true;
                print!("Processed {line_count} lines...\r");
                std::io::stdout().flush()?;
            }

            if line_count == options.first {
                break;
            }

            if !options
                .filter
                .iter()
                .all(|(k, v)| k.field_value(&word_entry) == v)
            {
                continue;
            }

            let entry = dict.process(edition, source, target, &word_entry);
            entries.extend(entry);
        }

        if printed_progress {
            println!();
        }
    }

    println!("Found {} entries", entries.len());
    if entries.is_empty() {
        // Compared to filter_jsonl, this is not an error since it does not prevent any code that
        // comes after (there is nothing after! this function does *everything*).
        // warn!("no valid entries for these filters. Exiting.");
        return Ok(());
    }

    dict.compact(&mut entries);
    println!("Compacted down to {} entries", entries.len());

    // dump ir if some flag is passed, save_temps I guess
    if options.save_temps && dict.write_temp() {
        let writer_path = pm.dir_tidy().join("tidy.jsonl");
        let writer_file = File::create(&writer_path)?;
        let writer = BufWriter::new(&writer_file);
        if options.pretty {
            serde_json::to_writer_pretty(writer, &entries)?;
        } else {
            serde_json::to_writer(writer, &entries)?;
        }
        pretty_print_at_path("Wrote tidy", &writer_path);
    }

    let yomitan_entries: Vec<YomitanEntry> = entries
        .into_iter()
        .map(|entry| dict.to_yomitan(entry))
        .collect();

    let labelled_entries = [("term", yomitan_entries)];
    write_yomitan(source, target, options, pm, &labelled_entries)?;

    Ok(())
}

fn make_yomitan_entries_glossary(
    source: EditionLang,
    target: Lang,
    word_entry: &WordEntry,
) -> Vec<YomitanEntry> {
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
        return vec![];
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

    let reading = get_reading(source, target, word_entry);
    let found_pos = match find_pos(&word_entry.pos) {
        Some(short_pos) => short_pos.to_string(),
        None => word_entry.pos.clone(),
    };
    let definition_tags = found_pos.clone();

    vec![YomitanEntry::TermBank(TermBank(
        word_entry.word.clone(),
        reading,
        definition_tags,
        found_pos,
        0,
        definitions,
        0,
        String::new(),
    ))]
}

type IGlossaryExtended = (String, String, EditionLang, Vec<String>);

// Should consume the WordEntry really
fn make_ir_glossary_extended(
    edition: EditionLang,
    source: Lang,
    target: Lang,
    word_entry: &WordEntry,
) -> Vec<IGlossaryExtended> {
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
        return vec![];
    }

    let found_pos = match find_pos(&word_entry.pos) {
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

    translations_product
}

fn make_yomitan_glossary_extended(ir: IGlossaryExtended) -> YomitanEntry {
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
        0,
        definitions,
        0,
        String::new(),
    ))
}

type IIpa = (String, PhoneticTranscription);

fn make_ir_ipa(edition: EditionLang, source: Lang, word_entry: &WordEntry) -> Vec<IIpa> {
    let ipas = get_ipas(word_entry);

    if ipas.is_empty() {
        return vec![];
    }

    let phonetic_transcription = PhoneticTranscription {
        reading: get_reading(edition, source, word_entry),
        transcriptions: ipas,
    };

    vec![(word_entry.word.clone(), phonetic_transcription)]
}

fn make_yomitan_ipa(ir: IIpa) -> YomitanEntry {
    let (lemma, phonetic_transcription) = ir;
    YomitanEntry::TermBankMeta(TermBankMeta::TermPhoneticTranscription(
        TermPhoneticTranscription(lemma, "ipa".to_string(), phonetic_transcription),
    ))
}

fn write_diagnostics(pm: &PathManager, diagnostics: &Diagnostics) -> Result<()> {
    if diagnostics.is_empty() {
        return Ok(());
    }

    let dir_diagnostics = pm.dir_diagnostics();
    fs::create_dir_all(&dir_diagnostics)?;

    write_sorted_json(
        &dir_diagnostics,
        "pos.json",
        &diagnostics.accepted_pos,
        &diagnostics.rejected_pos,
    )?;
    write_sorted_json(
        &dir_diagnostics,
        "tags.json",
        &diagnostics.accepted_tags,
        &diagnostics.rejected_tags,
    )?;

    Ok(())
}

// hacky: takes advantage of insertion order
fn convert_and_sort_indexmap(map: &Counter) -> IndexMap<String, (usize, Word)> {
    // Display first word
    let mut entries: Vec<_> = map
        .iter()
        .filter_map(|(key, words)| {
            words
                .first()
                .cloned()
                .map(|first_word| (key.clone(), (words.len(), first_word)))
        })
        .collect();

    entries.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    let mut sorted = IndexMap::with_capacity(entries.len());
    for (key, value) in entries {
        sorted.insert(key, value);
    }

    sorted
}

fn write_sorted_json(
    dir_diagnostics: &Path,
    name: &str,
    accepted: &Counter,
    rejected: &Counter,
) -> Result<()> {
    if accepted.is_empty() && rejected.is_empty() {
        return Ok(());
    }

    let accepted_sorted = convert_and_sort_indexmap(accepted);
    let rejected_sorted = convert_and_sort_indexmap(rejected);
    let json: IndexMap<&'static str, _> =
        IndexMap::from_iter([("rejected", rejected_sorted), ("accepted", accepted_sorted)]);

    let content = serde_json::to_string_pretty(&json)?;
    fs::write(dir_diagnostics.join(name), content)?;
    Ok(())
}

pub fn make_dict_main(args: &MainArgs, pm: &PathManager) -> Result<()> {
    debug_assert_eq!(args.langs.edition, args.langs.target);

    pm.setup_dirs()?;

    let path_jsonl_raw = pm.path_jsonl_raw();

    #[cfg(feature = "html")]
    {
        download_jsonl(
            args.langs.edition,
            args.langs.source,
            &path_jsonl_raw,
            args.options.redownload,
        )?;
    }

    let mut path_jsonl = pm.path_jsonl(args.langs.source, args.langs.target.into());
    if !args.skip.filtering {
        path_jsonl = filter_jsonl(
            args.langs.edition,
            args.langs.source,
            &args.options,
            &path_jsonl_raw,
            path_jsonl,
        )?;
    }

    let tidy_cache = if args.skip.tidy {
        None
    } else {
        let ret = tidy(&args.langs, &args.options, pm, &path_jsonl)?;
        Some(ret)
    };

    let mut diagnostics = Diagnostics::new();
    if !args.skip.yomitan {
        make_yomitan(&args.langs, &args.options, pm, &mut diagnostics, tidy_cache)?;
    }

    write_diagnostics(pm, &diagnostics)?;

    Ok(())
}

pub fn setup_tracing(verbose: bool) {
    // tracing_subscriber::fmt::init();
    // Same defaults as the above, without timestamps

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if verbose {
            // Only we are set to debug. ureq and other libs stay the same.
            EnvFilter::new(format!("{}=debug", env!("CARGO_PKG_NAME")))
        } else {
            EnvFilter::new("warn")
        }
    });

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        // .without_time()
        .with_target(true)
        .with_level(true)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cli::{DictionaryType, GlossaryArgs, GlossaryLangs};

    use anyhow::{Ok, Result, bail, ensure};

    use std::fs;
    use std::path::{Path, PathBuf};

    /// Clean empty folders under folder "root" recursively.
    fn cleanup(root: &Path) -> bool {
        let entries = fs::read_dir(root).unwrap();

        let mut is_empty = true;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let child_empty = cleanup(&path);
                if child_empty {
                    fs::remove_dir(&path).unwrap();
                } else {
                    is_empty = false;
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
            {
                panic!("zip found in tests");
            } else {
                is_empty = false;
            }
        }

        is_empty
    }

    fn fixture_options(fixture_dir: &Path) -> ArgsOptions {
        ArgsOptions {
            save_temps: true,
            pretty: true,
            experimental: false,
            root_dir: fixture_dir.to_path_buf(),
            ..Default::default()
        }
    }

    // Implementing SimpleArgs is not enough, since in Main we use the fact that target is an
    // EditionLang extensively.
    fn fixture_main_args(
        edition: EditionLang,
        source: Lang,
        target: EditionLang,
        fixture_dir: &Path,
    ) -> MainArgs {
        MainArgs {
            langs: MainLangs {
                edition,
                source,
                target,
            },
            options: fixture_options(fixture_dir),
            ..Default::default()
        }
    }

    fn fixture_glossary_args(
        edition: EditionLang,
        source: EditionLang,
        target: Lang,
        fixture_dir: &Path,
    ) -> GlossaryArgs {
        GlossaryArgs {
            langs: GlossaryLangs {
                edition,
                source,
                target,
            },
            options: fixture_options(fixture_dir),
            ..Default::default()
        }
    }

    // test via snapshots and commits like the original
    #[test]
    fn snapshot() {
        setup_tracing(false);

        let fixture_dir = PathBuf::from("tests");
        // have to hardcode this since we have not initialized args
        let fixture_input_dir = fixture_dir.join("kaikki");

        // Nuke the output dir to prevent pollution
        // It has the disadvantage of massive diffs if we failfast.
        //
        // let fixture_output_dir = fixture_dir.join("dict");
        // Don't crash if there is no output dir. It may happen if we nuke it manually
        // let _ = fs::remove_dir_all(fixture_output_dir);

        // iterdir and search for source-target-extract.jsonl files
        let mut cases = Vec::new();
        let mut langs_in_testsuite = Vec::new();

        for entry in fs::read_dir(&fixture_input_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if let Some(fname) = path.file_name().and_then(|f| f.to_str())
                && let Some(base) = fname.strip_suffix("-extract.jsonl")
                && let Some((source, target)) = base.split_once('-')
            {
                let src = source.parse::<Lang>().unwrap();
                let tar = target.parse::<Lang>().unwrap();
                cases.push((src, tar));

                if !langs_in_testsuite.contains(&src) {
                    langs_in_testsuite.push(src);
                }
                if !langs_in_testsuite.contains(&tar) {
                    langs_in_testsuite.push(tar);
                }
            }
        }

        debug!("Found {} cases: {cases:?}", cases.len());

        // failfast
        // main
        for (source, target) in &cases {
            let Result::Ok(target) = EditionLang::try_from(*target) else {
                continue; // skip if target is not edition
            };
            let args = fixture_main_args(target, *source, target, &fixture_dir);
            let pm = PathManager::new(DictionaryType::Main, &args);

            if let Err(e) = shapshot_main(&args.langs, &args.options, &pm) {
                panic!("({source}): {e}");
            }
        }

        // glossary
        for (source, target) in &cases {
            if source != target {
                continue;
            }

            let Result::Ok(source) = EditionLang::try_from(*source) else {
                continue; // skip if source is not edition
            };

            for possible_target in &langs_in_testsuite {
                let args = fixture_glossary_args(source, source, *possible_target, &fixture_dir);
                let pm = PathManager::new(DictionaryType::Glossary, &args);
                make_simple_dict(DGlossary, &args.options, &pm).unwrap();
            }
        }

        // ipa
        for (source, target) in &cases {
            let Result::Ok(target) = EditionLang::try_from(*target) else {
                continue; // skip if target is not edition
            };
            let args = fixture_main_args(target, *source, target, &fixture_dir);
            let pm = PathManager::new(DictionaryType::Ipa, &args);
            make_simple_dict(DIpa, &args.options, &pm).unwrap();
        }

        cleanup(&fixture_dir.join("dict"));
    }

    /// Delete generated artifacts from previous tests runs, if any
    fn delete_previous_output(pm: &PathManager) -> Result<()> {
        let pathdir_dict_temp = pm.dir_temp_dict();
        if pathdir_dict_temp.exists() {
            debug!("Deleting previous output: {pathdir_dict_temp:?}");
            fs::remove_dir_all(pathdir_dict_temp)?;
        }
        Ok(())
    }

    // NOTE: tidy and yomitan do not use args.edition in the original
    // NOTE: but we do, to validate links, matches etc. so this *can't* take an 'impl SimpleArgs'
    //
    // Read the expected result in the snapshot first, then compare
    fn shapshot_main(langs: &MainLangs, options: &ArgsOptions, pm: &PathManager) -> Result<()> {
        let fixture_path = pm.path_jsonl(langs.source, langs.target.into());
        ensure!(
            fixture_path.exists(),
            "Fixture path {fixture_path:?} does not exist"
        );
        eprintln!("------ Starting test @ {fixture_path:?}");

        delete_previous_output(pm)?;

        pm.setup_dirs().unwrap(); // this makes some noise but ok

        tidy(langs, options, pm, &fixture_path)?;
        let mut diagnostics = Diagnostics::new();
        make_yomitan(langs, options, pm, &mut diagnostics, None)?;
        write_diagnostics(pm, &diagnostics)?;

        check_git_diff(pm)
    }

    // check git --diff for charges in the generated json
    fn check_git_diff(pm: &PathManager) -> Result<()> {
        let output = std::process::Command::new("git")
            .args([
                "diff",
                "--color=always",
                "--unified=0", // show 0 context lines
                "--",
                // we don't care about tidy files
                &pm.dir_temp_dict().to_string_lossy(),
            ])
            .output()?;
        if !output.stdout.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            bail!("changes!")
        }

        Ok(())
    }
}
