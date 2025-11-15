// #![allow(unused_variables)]
// #![allow(unused_imports)]

use anyhow::{Context, Ok, Result, bail};
use flate2::read::GzDecoder;
use indexmap::{IndexMap, IndexSet};
use kty::cli::{Args, FilterKey};
use kty::lang::Lang;
use kty::locale::get_locale_examples_string;
use kty::models::{Example, Form, HeadTemplate, Pos, Sense, Tag, WordEntry};
use kty::tags::{
    BLACKLISTED_TAGS, IDENTITY_TAGS, REDUNDANT_TAGS, find_pos, find_tag_in_bank,
    get_tag_bank_as_tag_info, merge_person_tags, sort_tags,
};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::LazyLock;
use tracing::{Level, debug, error, info, span, trace, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use unicode_normalization::UnicodeNormalization;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn get_file_size_in_mb(path: &Path) -> Result<f64> {
    let metadata = fs::metadata(path)?;
    let size_bytes = metadata.len();
    let size_mb = size_bytes as f64 / (1024.0 * 1024.0);
    Ok(size_mb)
}

fn pretty_msg_at_path(msg: &str, path: &Path) -> String {
    let size_mb = get_file_size_in_mb(path).unwrap_or(-1.0);
    let at = "\x1b[1;36m@\x1b[0m"; // bold + cyan
    let size_str = format!("\x1b[1m{size_mb:.2} MB\x1b[0m"); // bold
    format!("{msg} {at} {} ({})", path.display(), size_str)
}

fn pretty_println_at_path(msg: &str, path: &Path) {
    println!("{}", pretty_msg_at_path(msg, path));
}

fn pretty_print_at_path(msg: &str, path: &Path) {
    print!("{}", pretty_msg_at_path(msg, path));
}

// Some pretty printing codepoints
const DOWNLOAD_C: &str = "⬇";
const SKIP_C: &str = "⏭";
const CHECK_C: &str = "✓";

fn skip_because_file_exists(skipped: &str, path: &Path) {
    let msg = format!("{SKIP_C} Skipping {skipped}: file already exists");
    pretty_println_at_path(&msg, path);
}

/// Download the raw jsonl from kaikki.
///
/// Does not write the .gz artifact to disk.
fn download_jsonl(args: &Args) -> Result<()> {
    let path_raw_jsonl = args.path_raw_jsonl();

    if path_raw_jsonl.exists() && !args.redownload {
        skip_because_file_exists("download", &path_raw_jsonl);
        return Ok(());
    }

    let url = match args.edition {
        // Default download name is: kaikki.org-dictionary-TARGET_LANGUAGE.jsonl.gz
        Lang::En => {
            // TODO: Test Serbo-Croatian (iso: sh), we only deal with spaces for now when escaping
            let filename = args.filename_raw_jsonl_gz();
            let source_long_esc = args.source.long().replace(' ', "%20");
            format!("https://kaikki.org/dictionary/{source_long_esc}/{filename}")
        }
        // Default download name is: raw-wiktextract-data.jsonl.gz
        other => format!("https://kaikki.org/{other}wiktionary/raw-wiktextract-data.jsonl.gz"),
    };

    println!("{DOWNLOAD_C} Downloading {url}");

    let response = match ureq::get(url).call() {
        core::result::Result::Ok(response) => response,
        Err(err @ ureq::Error::StatusCode(404)) => {
            // Normally this is caught at CLI time, but in case language.json or lang.rs
            // are outdated / wrong it may reach this...
            bail!(
                "{err}. Does the language {} ({}) have an edition?",
                args.edition.long(),
                args.edition
            )
        }
        Err(err) => bail!(err),
    };

    if let Some(last_modified) = response.headers().get("last-modified") {
        info!("Raw jsonl was last modified: {:?}", last_modified);
    }

    let reader = response.into_body().into_reader();
    // We can't use gzip's ureq feature because there is no content-encoding in headers
    // https://github.com/tatuylonen/wiktextract/issues/1482
    let mut decoder = GzDecoder::new(reader);

    let mut writer = BufWriter::new(File::create(&path_raw_jsonl)?);
    std::io::copy(&mut decoder, &mut writer)?;

    pretty_println_at_path(&format!("{CHECK_C} Downloaded"), &path_raw_jsonl);

    Ok(())
}

/// Filter by language iso and other input give key-value pairs.
///
/// Does not fully deserialize into WordEntry, it only checks that "lang_code" is equal to the
/// source language.
#[tracing::instrument(skip(args), fields(source = %args.source))]
fn filter_jsonl(args: &Args) -> Result<()> {
    // English edition already gives them filtered.
    // Yet don't skip if we have filter arguments.
    if matches!(args.edition, Lang::En) && args.filter.is_empty() && args.reject.is_empty() {
        println!("{SKIP_C} Skipping filtering: english edition detected");
        return Ok(());
    }

    let reader_path = args.path_raw_jsonl();
    let reader_file = File::open(&reader_path)?;
    let reader = BufReader::new(reader_file);
    let writer_path = args.path_jsonl();
    let writer_file = File::create(&writer_path)?;
    let mut writer = BufWriter::new(writer_file);
    debug!("Filtering: {reader_path:?} > {writer_path:?}",);

    let print_interval = 1000;
    let mut line_count = 1; // enumerate can't start at 1, and forces usize
    let mut extracted_lines_counter = 0;
    let mut printed_progress = false;

    let mut filter = args.filter.clone();
    let reject = args.reject.clone();
    let lang_code_filter = (FilterKey::LangCode, args.source.to_string());
    filter.push(lang_code_filter);
    debug!("Filter {filter:?} - Reject {reject:?}");

    for line in reader.lines() {
        line_count += 1;

        let line = line?;
        // Only relevant for tests. Kaikki jsonlines should not contain empty lines
        if line.is_empty() {
            continue;
        }

        let word_entry: WordEntry = match serde_json::from_str(&line) {
            core::result::Result::Ok(v) => v,
            Err(e) => {
                error!("Error decoding JSON @ filter (line {line_count})");
                bail!(e)
            }
        };

        if line_count % print_interval == 0 {
            printed_progress = true;
            print!("Processed {line_count} lines...\r");
            std::io::stdout().flush()?;
        }

        if line_count == args.first {
            break;
        }

        if reject.iter().any(|(k, v)| k.field_value(&word_entry) == v) {
            continue;
        }

        if !filter.iter().all(|(k, v)| k.field_value(&word_entry) == v) {
            continue;
        }

        extracted_lines_counter += 1;
        writeln!(writer, "{line}")?;
    }

    if printed_progress {
        println!();
    }

    pretty_println_at_path(
        &format!("{CHECK_C} Filtered {extracted_lines_counter} lines out of {line_count}"),
        &args.path_jsonl(),
    );

    Ok(())
}

// Tidy: internal types

type Map<K, V> = IndexMap<K, V>; // Preserve insertion order

type LemmaDict = Map<
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

// Not included in the dictionary: only used for debug
//
// In the future, consider alt_of, form_of
#[allow(unused)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FormSource {
    /// Form extracted from `word_entry.forms`
    Extracted,
    /// Form added via gloss analysis ("is inflection of...")
    Inflection,
}

type FormsMap = Map<
    String, // lemma
    Map<
        String, // form
        Map<
            Pos, // pos
            // Vec<String>, // inflections (tags really)
            (FormSource, Vec<String>), // (source, inflections (tags really))
        >,
    >,
>;

fn flat_iter_forms(
    form_map: &FormsMap,
) -> impl Iterator<Item = (&String, &String, &Pos, &FormSource, &Vec<String>)> {
    form_map.iter().flat_map(|(lemma, forms)| {
        forms.iter().flat_map(move |(form, pos_map)| {
            pos_map
                .iter()
                .map(move |(pos, (source, infls))| (lemma, form, pos, source, infls))
        })
    })
}

fn flat_iter_forms_mut(
    form_map: &mut FormsMap,
) -> impl Iterator<Item = (&String, &String, &Pos, &mut FormSource, &mut Vec<String>)> {
    form_map.iter_mut().flat_map(|(lemma, forms)| {
        forms.iter_mut().flat_map(move |(form, pos_map)| {
            pos_map
                .iter_mut()
                .map(move |(pos, (source, infls))| (lemma, form, pos, source, infls))
        })
    })
}

// Lemmainfo in the original
//
// NOTE: the less we have here the better. For example, the links could be entirely moved to the
// yomitan side of things. It all depends on what we may or may not consider useful for debugging.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct RawSenseEntry {
    ipa: Vec<Ipa>,

    #[serde(rename = "glossTree")]
    gloss_tree: GlossTree,

    #[serde(skip_serializing_if = "Option::is_none")]
    etymology_text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    head_info_text: Option<String>,

    #[serde(rename = "wlink")]
    link_wiktionary: String,

    // This is not included in the dictionary and is only used for debugging
    // * Should we include it in the dictionary though? Seems useful
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Ipa {
    ipa: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<Tag>,
}

/// Intermediate representation: useful for snapshots and debugging
#[derive(Debug, Default)]
struct Tidy {
    lemma_dict: LemmaDict,

    /// Forms that come from deinflection
    forms_map: FormsMap, // TODO: Rename to FormsDict for consistency

    /// Forms that come from expanding forms attribute (processforms)
    automated_forms_map: FormsMap,
}

impl Tidy {
    fn insert_lemma_entry(&mut self, lemma: &str, reading: &str, pos: &str, entry: &RawSenseEntry) {
        let etym_map = self
            .lemma_dict
            .entry(lemma.to_string())
            .or_default()
            .entry(reading.to_string())
            .or_default()
            .entry(pos.to_string())
            .or_default();

        // Assign a new etymology number incrementally
        let etymology_number = etym_map.len().to_string();

        etym_map.insert(etymology_number, entry.clone());
    }

    /// Generic helper to insert tags into a nested map
    fn insert_tags(
        map: &mut FormsMap,
        lemma: &str,
        form: &str,
        pos: &str,
        source: FormSource,
        tags: Vec<String>,
    ) {
        let entry = map
            .entry(lemma.to_string())
            .or_default()
            .entry(form.to_string())
            .or_default()
            .entry(pos.to_string())
            .or_insert_with(|| (source, Vec::new()));

        entry.1.extend(tags);
    }

    // rg: add_deinflection adddeinflection
    fn insert_inflections_forms(
        &mut self,
        lemma: &str,
        form: &str,
        pos: &str,
        source: FormSource,
        tags: Vec<Tag>,
    ) {
        debug_assert!(matches!(source, FormSource::Inflection));
        // TODO: normalizeinflection
        if lemma == form {
            return; // NOP: we don't add tautological forms
        }
        Self::insert_tags(&mut self.forms_map, lemma, form, pos, source, tags);
    }

    fn insert_expansion_forms(
        &mut self,
        lemma: &str,
        form: &str,
        pos: &str,
        source: FormSource,
        tags: Vec<Tag>,
    ) {
        debug_assert!(matches!(source, FormSource::Extracted));
        // TODO: normalizeinflection
        if lemma == form {
            return; // NOP: we don't add tautological forms
        }
        Self::insert_tags(
            &mut self.automated_forms_map,
            lemma,
            form,
            pos,
            source,
            tags,
        );
    }

    /// Return both inner `FormsMap` merged
    ///
    /// removes redundant tags on the process! TODO: should be done somewhere else for clarity
    fn all_forms(&self) -> FormsMap {
        let mut merged = self.forms_map.clone();

        for (lemma, form, pos, source, tags) in flat_iter_forms(&self.automated_forms_map) {
            let merged_form_map = merged.entry(lemma.clone()).or_default();
            let merged_pos_map = merged_form_map.entry(form.clone()).or_default();

            let entry = merged_pos_map
                .entry(pos.clone())
                .or_insert_with(|| (source.clone(), Vec::new()));

            entry.1.extend(tags.clone());
        }

        // TODO: move this somewhere else
        // remove redundant tags
        // turned off for the moment since it messes diffs, but it works fine
        // for (_, _, _, tags) in flat_iter_forms_mut(&mut merged) {
        //     kty::tags::remove_redundant_tags(tags);
        // }

        merged
    }
    // INVARIANTS: for debug (maybe put this as a test)
    fn check_invariants(&self) {
        for (_, _, _, _, tags) in flat_iter_forms(&self.forms_map) {
            let mut seen = IndexSet::new();
            for tag in tags {
                debug_assert!(seen.insert(tag), "Duplicate tag found: {tag}");
            }
        }
    }
}

#[tracing::instrument(skip_all)]
fn tidy(args: &Args, path_jsonl: &Path) -> Result<Tidy> {
    let input_file =
        File::open(path_jsonl).with_context(|| format!("Failed to open: {path_jsonl:?} @ tidy"))?;
    let reader = BufReader::new(input_file);
    let ret = tidy_go(args, reader)?;
    write_tidy(args, &ret)?;
    Ok(ret)
}

static PARENS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(.+?\)").unwrap());

// TODO: refactor, much of this should go to handle_line > handle_word_entry
#[tracing::instrument(skip_all)]
fn tidy_go(args: &Args, reader: BufReader<File>) -> Result<Tidy> {
    let mut ret = Tidy::default();

    for line in reader.lines() {
        let line = line?;
        // We can not remove the is_empty check and the deserialization error handling, because
        // the tests will call directly this function without previously filtering.
        //
        // only relevant for tests - the kaikki jsonlines should not contain empty lines
        if line.is_empty() {
            continue;
        }
        let mut word_entry: WordEntry = match serde_json::from_str(&line) {
            core::result::Result::Ok(v) => v,
            Err(e) => {
                error!("Error decoding JSON @ tidy");
                bail!(e)
            }
        };

        // rg searchword
        // debug (with only relevant, as in, deserialized, information)
        // if word_entry.word == "зимний" {
        //     // if args.source.to_string() == "ru" && args.target.to_string() == "en" {
        //     // println!("{}", serde_json::to_string_pretty(&word_entry.senses)?);
        //     println!("{}", serde_json::to_string_pretty(&word_entry)?);
        // }

        // Everything that mutates word_entry
        preprocess_word_entry(args, &mut word_entry, &mut ret);

        // rg processforms
        process_forms(&word_entry, &mut ret);

        // dont push lemma if inflection
        if word_entry.senses.is_empty() {
            continue;
        }

        // debug (for nested stuff)
        // if let Some(existing) = ret.lemma_dict.get(&word_entry.word) {
        //     error!("'{}' already has an entry: {:?}", word_entry.word, existing);
        // }

        // rg insertlemma handleline
        // easy version of handleLine
        let reading = get_reading(args, &word_entry);
        let raw_sense_entry = process_word_entry(args, &word_entry);

        ret.insert_lemma_entry(
            &word_entry.word,
            &reading,
            &word_entry.pos,
            &raw_sense_entry,
        );
    }

    // postprocessing
    //
    // the original only similar sorts automated forms... (not sure why?)
    for (_, _, _, _, tags) in flat_iter_forms_mut(&mut ret.automated_forms_map) {
        // Keep only unique tags
        let mut seen = IndexSet::new();
        tags.retain(|tag| seen.insert(tag.clone()));

        *tags = merge_person_tags(tags);
        kty::tags::sort_tags_by_similar(tags);
    }
    for (_, _, _, _, tags) in flat_iter_forms_mut(&mut ret.forms_map) {
        // Keep only unique tags
        let mut seen = IndexSet::new();
        tags.retain(|tag| seen.insert(tag.clone()));
    }

    // TODO: rg: handleautomatedforms
    // dump automated_forms_map to forms_map via add_deinflections

    // TODO: Remove this once done with testing
    ret.check_invariants();

    Ok(ret)
}

// Everything that mutates word_entry
fn preprocess_word_entry(args: &Args, word_entry: &mut WordEntry, ret: &mut Tidy) {
    // WARN: mutates word_entry::senses::glosses
    //
    // rg: final dot
    // https://github.com/yomidevs/yomitan/issues/2232
    // Add a full stop if there is no trailing punctuation
    // static TRAILING_PUNCT_RE: LazyLock<Regex> =
    //     LazyLock::new(|| Regex::new(r"\p{P}$").unwrap());
    // for sense in word_entry.senses.iter_mut() {
    //     for gloss in sense.glosses.iter_mut() {
    //         if !TRAILING_PUNCT_RE.is_match(gloss) {
    //             gloss.push('.');
    //         }
    //     }
    // }

    // WARN: mutates word_entry::senses::sense::tags
    //
    // [en]
    // not entirely sure why this hack was needed... (can't we just look at forms?)
    // it does indeed add some tags from head_templates in the grc/en testsuite
    if matches!(args.target, Lang::En) {
        let tag_matches = [
            ["pf", "perfective"],
            ["impf", "imperfective"],
            ["m", "masculine"],
            ["f", "feminine"],
            ["n", "neuter"],
            ["inan", "inanimate"],
            ["anim", "animate"],
        ];
        for head_template in &word_entry.head_templates {
            let cleaned = PARENS_RE.replace_all(&head_template.expansion, "");
            let words: Vec<_> = cleaned.split_whitespace().collect();

            for sense in &mut word_entry.senses {
                for tag_match in tag_matches {
                    let short_tag = tag_match[0];
                    let long_tag = tag_match[1].to_string();
                    if words.contains(&short_tag) && !sense.tags.contains(&long_tag) {
                        sense.tags.push(long_tag);
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
    if matches!(args.target, Lang::Ru) {
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
    if matches!(args.target, Lang::El) {
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
    // easy inflection handling
    // if is_inflection_gloss then process inflection glosses etc.
    //
    // make a vec for senses not inflected etc. filter
    // temporarily take ownership of the old senses
    let old_senses = std::mem::take(&mut word_entry.senses);
    let mut senses_without_inflections = Vec::new();
    for sense in old_senses {
        if is_inflection_gloss(args, word_entry, &sense) {
            handle_inflection_gloss(args, word_entry, &sense, ret);
        } else {
            senses_without_inflections.push(sense);
        }
    }
    word_entry.senses = senses_without_inflections;
}

// rg: processforms
fn process_forms(word_entry: &WordEntry, ret: &mut Tidy) {
    for form in &word_entry.forms {
        // bunch of validation: skip
        // blacklisted forms (happens at least in English)
        if form.form == "-" {
            continue;
        }

        // easy filtering tags
        let is_blacklisted = form
            .tags
            .iter()
            .any(|tag| BLACKLISTED_TAGS.contains(&tag.as_str()));
        let is_identity = form
            .tags
            .iter()
            .all(|tag| IDENTITY_TAGS.contains(&tag.as_str()));
        if is_blacklisted || is_identity {
            continue;
        }

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

        ret.insert_expansion_forms(
            &word_entry.word,
            &form.form,
            &word_entry.pos,
            FormSource::Extracted,
            vec![filtered_tags.join(" ")],
        );
    }
}

// There are potentially more than one, but I haven't seen that happen
fn get_reading(args: &Args, word_entry: &WordEntry) -> String {
    match args.source {
        Lang::Ja => get_japanese_reading(args, word_entry),
        Lang::Fa => {
            // use romanization over canonical_word_form
            let romanization_form = word_entry
                .forms
                .iter()
                .find(|form| form.tags == ["romanization"] && !form.form.is_empty());
            match romanization_form {
                Some(romanization_form) => romanization_form.form.clone(),
                None => word_entry.word.clone(),
            }
        }
        _ => get_canonical_word_form(args, word_entry).to_string(),
    }
}

/// The canonical form may contain extra diacritics.
///
/// For most languages, this is equal to word, but for, let's say, Latin, there may be a
/// difference (cf. <https://en.wiktionary.org/wiki/fama>, where `word_entry.word` is fama, but
/// this will return fāma).
fn get_canonical_word_form<'a>(args: &Args, word_entry: &'a WordEntry) -> &'a str {
    match args.source {
        Lang::La | Lang::Ru => match get_canonical_form(word_entry) {
            Some(cform) => &cform.form,
            None => &word_entry.word,
        },
        _ => &word_entry.word,
    }
}

// Guarantees that form.form is not empty
fn get_canonical_form(word_entry: &WordEntry) -> Option<&Form> {
    word_entry
        .forms
        .iter()
        .find(|form| form.tags.iter().any(|tag| tag == "canonical") && !form.form.is_empty())
}

// Does not support multiple readings
fn get_japanese_reading(_args: &Args, word_entry: &WordEntry) -> String {
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
fn process_word_entry(args: &Args, word_entry: &WordEntry) -> RawSenseEntry {
    // default version getphonetictranscription
    let ipas: Vec<_> = word_entry
        .sounds
        .iter()
        .filter_map(|sound| {
            if sound.ipa.is_empty() {
                return None;
            }
            let ipa = sound.ipa.clone();
            let mut tags = sound.tags.clone();
            if !sound.note.is_empty() {
                tags.push(sound.note.clone());
            }
            Some(Ipa { ipa, tags })
        })
        .collect();

    // done in saveIpaResult - we just group it here
    // basically group by ipa
    let mut ipas_grouped: Vec<Ipa> = Vec::new();
    for ipa in ipas {
        if let Some(existing) = ipas_grouped.iter_mut().find(|e| e.ipa == ipa.ipa) {
            for tag in ipa.tags {
                if !existing.tags.contains(&tag) {
                    existing.tags.push(tag);
                }
            }
        } else {
            ipas_grouped.push(ipa.clone());
        }
    }

    // etymology_text
    // Reconvert to Option ~ a bit dumb, could deserialize it as Option, but we use defaults
    // at most WordEntry attributes so I think it's better to be consistent
    let etymology_text = if word_entry.etymology_text.is_empty() {
        None
    } else {
        Some(word_entry.etymology_text.clone())
    };

    let head_info_text = get_head_info(&word_entry.head_templates);

    let gloss_tree = get_gloss_tree(word_entry);

    RawSenseEntry {
        ipa: ipas_grouped,
        gloss_tree,
        etymology_text,
        head_info_text,
        link_wiktionary: get_link_wiktionary(args, &word_entry.word),
        link_kaikki: get_link_kaikki(args, &word_entry.word),
    }
}

// Useful for debugging too
fn get_link_wiktionary(args: &Args, word: &str) -> String {
    format!(
        "https://{}.wiktionary.org/wiki/{}#{}",
        args.target,
        word,
        args.source.long()
    )
}

// Same debug but for kaikki
#[allow(unused)]
fn get_link_kaikki(args: &Args, word: &str) -> String {
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
    let dictionary = match args.edition {
        Lang::En => "dictionary".to_string(),
        other => format!("{other}wiktionary"),
    };
    let unescaped_url = format!(
        "https://kaikki.org/{}/{}/meaning/{}.html",
        dictionary,
        args.source.long(),
        search_query
    );
    unescaped_url.replace(' ', "%20")
}

// rg: getheadinfo
// if there is no head_templates we compile the regex pointlessly but it should return None
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

// isnt this in javascript
// {"_type": "map", "map": [[key, value]]}
//
// equal to just
// {key: value} ?
//
// It doesn't really matter... it's just the IR
fn get_gloss_tree(entry: &WordEntry) -> GlossTree {
    let mut gloss_tree = GlossTree::new();

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

        // for gloss in &sense.glosses {
        //     gloss_tree.insert(
        //         gloss.clone(),
        //         GlossInfo {
        //             _tags: sense.tags.clone(),
        //             _examples: filtered_examples.clone(),
        //             _children: Map::default(),
        //         },
        //     );
        // }

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
        children: GlossTree::new(),
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
fn is_inflection_gloss(args: &Args, _word_entry: &WordEntry, sense: &Sense) -> bool {
    match args.target {
        Lang::De => {
            static RE_INFLECTION_DE: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r" des (Verbs|Adjektivs|Substantivs|Demonstrativpronomens|Possessivpronomens|Pronomens)").unwrap()
            });
            sense
                .glosses
                .iter()
                .any(|gloss| RE_INFLECTION_DE.is_match(gloss))
        }
        Lang::El => {
            !sense.form_of.is_empty() && sense.glosses.iter().any(|gloss| gloss.contains("του"))
        }
        Lang::En => {
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

fn handle_inflection_gloss(args: &Args, word_entry: &WordEntry, sense: &Sense, ret: &mut Tidy) {
    match args.target {
        Lang::El => {
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
                .map(|t| t.to_string())
                .collect();
            let inflection_tags: Vec<_> = if allowed_tags.is_empty() {
                vec![format!("redirected from {}", word_entry.word)]
            } else {
                allowed_tags
            };
            for lemma in &sense.form_of {
                ret.insert_inflections_forms(
                    &lemma.word,
                    &word_entry.word,
                    &word_entry.pos,
                    FormSource::Inflection,
                    inflection_tags.clone(),
                );
            }
        }
        Lang::En => handle_inflection_gloss_en(args, word_entry, sense, ret),
        Lang::De => {
            if sense.glosses.is_empty() {
                return;
            }

            static INFLECTION_RE: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(
        r"^(.*)des (?:Verbs|Adjektivs|Substantivs|Demonstrativpronomens|Possessivpronomens|Pronomens) (.*)$"
    ).unwrap()
            });

            if let Some(caps) = INFLECTION_RE.captures(&sense.glosses[0]) {
                if let (Some(inflection_tags), Some(lemma)) = (caps.get(1), caps.get(2)) {
                    let inflection_tags = inflection_tags.as_str().trim();
                    let lemma = lemma.as_str().trim();

                    if !inflection_tags.is_empty() {
                        ret.insert_inflections_forms(
                            &lemma,
                            &word_entry.word,
                            &word_entry.pos,
                            FormSource::Inflection,
                            vec![inflection_tags.to_string()],
                        );
                    }
                }
            }
        }
        _ => (),
    }
}

// this is awful
//
// tested in the es-en suite
fn handle_inflection_gloss_en(args: &Args, word_entry: &WordEntry, sense: &Sense, ret: &mut Tidy) {
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

    static LEMMA_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"of ([^\s]+)\s*(\(.+?\))?$").unwrap());
    static INFLECTION_RE_1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\(.+?\)$").unwrap());
    // dont need a regex for this really
    static INFLECTION_RE_2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

    for mut inflection in gloss_pieces {
        // Extract lemma
        if let Some(caps) = LEMMA_RE.captures(&inflection)
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
            .trim()
            .to_string();
        // Remove parentheses and compress whitespace
        inflection = INFLECTION_RE_1.replace_all(&inflection, "").to_string();
        inflection = INFLECTION_RE_2.replace_all(&inflection, " ").to_string();

        if !inflection.trim().is_empty() {
            inflections.insert(inflection);
        }
    }

    if let Some(lemma) = lemmas.iter().next() {
        // Not sure if this is better (cf. ru-en) over word_entry.word but it is what was done in
        // the original, so lets not change that for the moment.
        let cform_str = get_canonical_word_form(args, word_entry);

        for inflection in inflections {
            ret.insert_inflections_forms(
                lemma,
                // &word_entry.word, // < here, not canonical
                cform_str,
                &word_entry.pos,
                FormSource::Inflection,
                vec![inflection],
            );
        }
    }
}

// NOTE: we write stuff even if ret.attribute is empty
//
/// Write a Tidy struct to disk.
///
/// This is effectively a snapshot of our tidy intermediate representation.
#[tracing::instrument(skip_all)]
fn write_tidy(args: &Args, ret: &Tidy) -> Result<()> {
    let opath = args.path_lemmas();
    let file = File::create(&opath)?;
    let writer = BufWriter::new(file);
    if args.ugly {
        serde_json::to_writer(writer, &ret.lemma_dict)?;
    } else {
        serde_json::to_writer_pretty(writer, &ret.lemma_dict)?;
    }
    pretty_println_at_path(
        &format!("Wrote tidy lemmas: {}", ret.lemma_dict.len()),
        &opath,
    );

    // Forms are written by chunks in the original (cf. mapChunks). Not sure if needed.
    // If I even change that, do NOT hardcode the forms number (i.e. the 0 in ...forms-0.json)
    let opath = args.path_forms();
    let file = File::create(&opath)?;
    let writer = BufWriter::new(file);
    let all_forms = ret.all_forms();
    if args.ugly {
        serde_json::to_writer(writer, &all_forms)?;
    } else {
        serde_json::to_writer_pretty(writer, &all_forms)?;
    }
    let n_forms: usize = all_forms.values().map(|v| v.len()).sum();
    let n_deinflected_forms: usize = ret.forms_map.values().map(|v| v.len()).sum();
    let n_extracted_forms: usize = ret.automated_forms_map.values().map(|v| v.len()).sum();
    pretty_println_at_path(
        &format!(
            "Wrote tidy forms: {} (D{},E{})",
            n_forms, n_deinflected_forms, n_extracted_forms
        ),
        &opath,
    );
    println!(
        "* For a total of: {} entries",
        n_forms + ret.lemma_dict.len()
    );

    Ok(())
}

fn get_index(args: &Args) -> String {
    let current_date = chrono::Utc::now().format("%Y-%m-%d");
    format!(
        r#"{{
  "title": "{}",
  "format": 3,
  "revision": "{}",
  "sequenced": true,
  "author": "Kaikki-to-Yomitan contributors",
  "url": "https://github.com/yomidevs/kaikki-to-yomitan",
  "description": "Dictionaries for various language pairs generated from Wiktionary data, via Kaikki and Kaikki-to-Yomitan.",
  "attribution": "https://kaikki.org/",
  "sourceLanguage": "{}",
  "targetLanguage": "{}"
}}"#,
        args.dict_name, current_date, args.source, args.target
    )
}

// https://github.com/MarvNC/yomichan-dict-builder/blob/master/src/types/yomitan/termbank.ts
// @ TermInformation
#[derive(Debug, Serialize, Deserialize)]
pub struct YomitanEntry(
    pub String,                  // term
    pub String,                  // reading
    pub String,                  // definition_tags
    pub String,                  // rules
    pub u32,                     // frequency
    pub Vec<DetailedDefinition>, // definitions
    pub u32,                     // sequence
    pub String,                  // term_tags
);

// https://github.com/MarvNC/yomichan-dict-builder/blob/master/src/types/yomitan/termbank.ts
// @ StructuredContentNode
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum Node {
    Text(String),
    Array(Box<Vec<Node>>),
    Generic(Box<GenericNode>),
    Backlink(BacklinkContent),
}

impl Node {
    /// Push a new node into the array variant.
    pub fn push(&mut self, node: Self) {
        match self {
            Self::Array(boxed_vec) => boxed_vec.push(node),
            _ => panic!("Error: called 'push' with a non Node::Array"),
        }
    }

    /// Inserts a new node at position `index` into the array variant.
    pub fn insert(&mut self, index: usize, node: Self) {
        match self {
            Self::Array(boxed_vec) => boxed_vec.insert(index, node),
            _ => panic!("Error: called 'insert' with a non Node::Array"),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Array(boxed_vec) => boxed_vec.is_empty(),
            _ => panic!("Error: called 'is_empty' to a non Node::Array"),
        }
    }

    #[must_use]
    pub fn to_array_node(self) -> Self {
        Self::Array(Box::new(vec![self]))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(transparent)]
pub struct NodeData {
    pub inner: Map<String, String>,
}

impl<K, V> FromIterator<(K, V)> for NodeData
where
    K: Into<String>,
    V: Into<String>,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let inner = iter
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        Self { inner }
    }
}

// The order follows kty serialization, not yomichan builder order
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GenericNode {
    /// 'span' | 'div' | 'ol' | 'ul' | 'li' | 'details' | 'summary'
    /// INVARIANT is not respected here (tag could be any string)
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<NodeData>,
    pub content: Node,
}

impl GenericNode {
    pub fn to_node(self) -> Node {
        Node::Generic(Box::new(self))
    }

    pub fn to_array_node(self) -> Node {
        Node::Array(Box::new(vec![self.to_node()]))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BacklinkContent {
    pub tag: String,
    pub href: String,
    pub content: String,
}

// https://github.com/MarvNC/yomichan-dict-builder/blob/master/src/types/yomitan/termbank.ts
// @ DetailedDefinition
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum DetailedDefinition {
    // Plain(String),
    StructuredContent(StructuredContent),
    Inflection((String, Vec<String>)),
}

impl DetailedDefinition {
    /// Build a `DetailedDefinition::StructuredContent` variant from a Node
    pub fn structured(content: Node) -> Self {
        Self::StructuredContent(StructuredContent {
            ty: "structured-content".to_string(),
            content,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StructuredContent {
    #[serde(rename = "type")]
    pub ty: String, // should be hardcoded to "structured-content" (but then to serialize it...)
    pub content: Node,
}

fn wrap(tag: &str, content_ty: &str, content: Node) -> Node {
    GenericNode {
        tag: tag.into(),
        title: None, // hardcoded since most of the wrap calls don't use it
        data: match content_ty {
            "" => None,
            _ => Some(NodeData::from_iter([("content", content_ty)])),
        },
        content,
    }
    .to_node()
}

type TagCounter = Map<Tag, usize>;
type PosCounter = Map<Pos, usize>;

// For debugging purposes
#[derive(Debug, Default)]
struct Diagnostics {
    /// Tags found in bank
    accepted_tags: TagCounter,
    /// Tags not found in bank
    rejected_tags: TagCounter,

    /// POS found in bank
    accepted_pos: PosCounter,
    /// POS not found in bank
    rejected_pos: PosCounter,
}

impl Diagnostics {
    fn new() -> Self {
        Self::default()
    }

    fn increment_accepted_tag(&mut self, tag: Tag) {
        *self.accepted_tags.entry(tag).or_insert(0) += 1;
    }

    fn increment_rejected_tag(&mut self, tag: Tag) {
        *self.rejected_tags.entry(tag).or_insert(0) += 1;
    }

    fn increment_accepted_pos(&mut self, pos: Pos) {
        *self.accepted_pos.entry(pos).or_insert(0) += 1;
    }

    fn increment_rejected_pos(&mut self, pos: Pos) {
        *self.rejected_pos.entry(pos).or_insert(0) += 1;
    }

    #[allow(unused)] // replaced for writing diagnostics but can be of use later on
    fn log(&self) {
        let span = span!(Level::INFO, "diagnostics");
        let _span = span.enter();

        let skipped_tags_count: usize = self.rejected_tags.values().sum();
        let skipped_pos_count: usize = self.rejected_pos.values().sum();

        debug!(
            "skipped tags ({}): {:?}",
            skipped_tags_count, self.rejected_tags
        );
        debug!(
            "skipped  pos ({}): {:?}",
            skipped_pos_count, self.rejected_pos
        );
    }
}

// For el/en/fr this is trivial ~ Needs lang script for the rest
fn normalize_orthography(source_lang: Lang, term: &str) -> String {
    match source_lang {
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
    args: &Args,
    lemma_dict: &LemmaDict,
    diagnostics: &mut Diagnostics,
) -> Vec<YomitanEntry> {
    let mut yomitan_entries = Vec::new();

    for (lemma, readings) in lemma_dict {
        for (reading, pos_word) in readings {
            let normalized_lemma = normalize_orthography(args.source, lemma);

            for (pos, etyms) in pos_word {
                for (_etym_number, info) in etyms {
                    let yomitan_entry = make_yomitan_lemma(
                        args,
                        reading,
                        &normalized_lemma,
                        pos,
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

fn make_yomitan_lemma(
    args: &Args,
    reading: &String,
    normalized_lemma: &String,
    pos: &Pos,
    info: &RawSenseEntry,
    diagnostics: &mut Diagnostics,
) -> YomitanEntry {
    // rg: findpartofspeech findpos
    let found_pos: String = if let Some(short_pos) = find_pos(pos) {
        diagnostics.increment_accepted_pos(pos.into());
        short_pos
    } else {
        diagnostics.increment_rejected_pos(pos.into());
        pos
    }
    .to_string();

    // common tags to all glosses (this is an English edition reasoning really...)
    let common_tags: Vec<Tag> = info
        .gloss_tree
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
                if tag != pos {
                    diagnostics.increment_rejected_tag(tag.into());
                }
            }
            Some(res) => {
                if tag != pos {
                    diagnostics.increment_accepted_tag(tag.into());
                }
                common_short_tags_recognized.push(res.short_tag);
            }
        }
        // save to ymtTags to write later: skip
    }
    // Some filtering here: skip
    let definition_tags = common_short_tags_recognized.join(" ");

    // gloss_tree
    let gloss_content =
        get_structured_glosses(args, &info.gloss_tree, &common_short_tags_recognized);

    let mut detailed_definition_content =
        wrap("ol", "glosses", Node::Array(Box::new(gloss_content))).to_array_node();

    // rg: etymologytext / head_info_text headinfo
    if info.etymology_text.is_some() || info.head_info_text.is_some() {
        let structured_preamble =
            get_structured_preamble(info.etymology_text.as_ref(), info.head_info_text.as_ref());
        detailed_definition_content.insert(0, structured_preamble);
    }

    let backlink = get_structured_backlink(&info.link_wiktionary);
    detailed_definition_content.push(backlink);

    let detailed_definition = DetailedDefinition::structured(detailed_definition_content);

    let yomitan_reading = if *reading == *normalized_lemma {
        ""
    } else {
        reading
    };

    YomitanEntry(
        normalized_lemma.clone(),
        yomitan_reading.into(),
        definition_tags,
        found_pos,
        0,
        vec![detailed_definition],
        0,
        String::new(),
    )
}

fn build_details_entry(ty: &str, content: &str) -> Node {
    let mut summary = wrap("summary", "summary-entry", Node::Text(ty.into())).to_array_node();
    let div = wrap("div", &format!("{ty}-content"), Node::Text(content.into()));
    summary.push(div);
    wrap("details", &format!("details-entry-{ty}"), summary)
}

fn get_structured_preamble(
    etymology_text: Option<&String>,
    head_info_text: Option<&String>,
) -> Node {
    let mut preamble_content = Node::Array(Box::default());
    if let Some(head_info_text) = &head_info_text {
        let detail = build_details_entry("Grammar", head_info_text);
        preamble_content.push(detail);
    }
    if let Some(etymology_text) = &etymology_text {
        let detail = build_details_entry("Etymology", etymology_text);
        preamble_content.push(detail);
    }
    let preamble = wrap("div", "preamble", preamble_content);

    wrap("div", "", preamble.to_array_node())
}

fn get_structured_backlink(link: &str) -> Node {
    wrap(
        "div",
        "backlink",
        Node::Backlink(BacklinkContent {
            tag: "a".into(),
            href: link.into(),
            content: "Wiktionary".into(),
        })
        .to_array_node(),
    )
}

// should return Node for consistency
fn get_structured_glosses(
    args: &Args,
    gloss_tree: &GlossTree,
    common_short_tags_recognized: &[Tag],
) -> Vec<Node> {
    let mut sense_content = Vec::new();
    for (gloss, gloss_info) in gloss_tree {
        let synthetic_branch = GlossTree::from_iter([(gloss.clone(), gloss_info.clone())]);
        let nested_gloss =
            get_structured_glosses_go(args, &synthetic_branch, common_short_tags_recognized, 0);
        let structured_gloss = wrap("li", "", Node::Array(Box::new(nested_gloss)));
        sense_content.push(structured_gloss);
    }
    sense_content
}

// Recursive helper
// should return Node for consistency
fn get_structured_glosses_go(
    args: &Args,
    gloss_tree: &GlossTree,
    common_short_tags_recognized: &[Tag],
    level: usize,
) -> Vec<Node> {
    let html_tag = if level == 0 { "div" } else { "li" };
    let mut nested = Vec::new();

    for (gloss, gloss_info) in gloss_tree {
        let level_tags = gloss_info.tags.clone();
        // delete _tags but why

        // processglosstags: skip
        let minimal_tags: Vec<_> = level_tags
            .into_iter()
            .filter(|tag| !common_short_tags_recognized.contains(tag))
            .collect();

        let mut level_content = Node::Array(Box::default());
        // delete _examples but why

        if let Some(structured_tags) =
            get_structured_tags(&minimal_tags, common_short_tags_recognized)
        {
            level_content.push(structured_tags);
        }

        let gloss_content = Node::Text(gloss.into());
        level_content.push(gloss_content);

        if !gloss_info.examples.is_empty() {
            let structured_examples = get_structured_examples(args, &gloss_info.examples);
            level_content.push(structured_examples);
        }

        let level_structured = wrap(html_tag, "", level_content);
        nested.push(level_structured);

        if !gloss_info.children.is_empty() {
            // we dont want tags from the parent appearing again in the children
            let mut new_common_short_tags_recognized = common_short_tags_recognized.to_vec();
            new_common_short_tags_recognized.extend(minimal_tags);

            let child_defs = get_structured_glosses_go(
                args,
                &gloss_info.children,
                &new_common_short_tags_recognized,
                level + 1,
            );
            let structured_child_defs = wrap("ul", "", Node::Array(Box::new(child_defs)));
            nested.push(structured_child_defs);
        }
    }

    nested
}

// uses an option because we need to check if structured_tags_content is empty in order to push it
// outside of this function, but it is extremely hard to do so with the opaque Node
//
// ~~ maybe fix it later and make it work like get_structured_examples
fn get_structured_tags(tags: &[Tag], common_short_tags_recognized: &[Tag]) -> Option<Node> {
    let mut structured_tags_content = Vec::new();

    for tag in tags {
        let full_tag = find_tag_in_bank(tag);
        if full_tag.is_none() {
            continue;
        }

        // minimaltags
        // HACK: the conversion to short tag is done differently in the original
        let short_tag = full_tag
            .as_ref()
            .map(|t| t.short_tag.clone())
            .unwrap_or_default();
        if common_short_tags_recognized.contains(&short_tag) {
            // We dont want "masculine" appear twice...
            continue;
        }

        // defaults to "" if None
        let title = full_tag
            .as_ref()
            .map(|t| t.long_tag.clone())
            .unwrap_or_default();
        let category = full_tag
            .as_ref()
            .map(|t| t.category.clone())
            .unwrap_or_default();

        let structured_tag_content = GenericNode {
            tag: "span".into(),
            title: Some(title),
            data: Some(NodeData::from_iter([
                ("content", "tag"),
                ("category", &category),
            ])),
            content: Node::Text(short_tag),
        }
        .to_node();
        structured_tags_content.push(structured_tag_content);
    }

    if structured_tags_content.is_empty() {
        None
    } else {
        Some(wrap(
            "div",
            "tags",
            Node::Array(Box::new(structured_tags_content)),
        ))
    }
}

fn get_structured_examples(args: &Args, examples: &[Example]) -> Node {
    let mut structured_examples_content = wrap(
        "summary",
        "summary-entry",
        Node::Text(get_locale_examples_string(&args.target, examples.len())),
    )
    .to_array_node();

    for example in examples {
        let mut structured_example_content = wrap(
            "div",
            "example-sentence-a",
            Node::Text(example.text.clone()),
        )
        .to_array_node();
        if !example.translation.is_empty() {
            let structured_translation_content = wrap(
                "div",
                "example-sentence-b",
                Node::Text(example.translation.clone()),
            );
            structured_example_content.push(structured_translation_content);
        }
        let structured_example_content_wrap = wrap(
            "div",
            "extra-info",
            wrap("div", "example-sentence", structured_example_content),
        );
        structured_examples_content.push(structured_example_content_wrap);
    }

    wrap(
        "details",
        "details-entry-examples",
        structured_examples_content,
    )
}

fn make_yomitan_forms(args: &Args, forms_dict: &FormsMap) -> Vec<YomitanEntry> {
    let mut yomitan_entries = Vec::new();

    for (lemma, form, _pos, _source, glosses) in flat_iter_forms(forms_dict) {
        let mut inflection_hypotheses: Vec<Vec<String>> = Vec::new();
        for gloss in glosses {
            let hypotheses: Vec<Vec<String>> = vec![vec![gloss.into()]];

            // Normalize each hypothesis: trim each inflection, drop empties, convert NBSP back to space
            for hypothesis in hypotheses {
                let normalized: Vec<String> = hypothesis
                    .into_iter()
                    .map(|inflection| inflection.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.replace('\u{00A0}', " ")) // kludge from the original
                    .collect();

                if !normalized.is_empty() {
                    inflection_hypotheses.push(normalized);
                }
            }
        }
        // TODO: sort tags
        let deinflection_definitions: Vec<DetailedDefinition> = inflection_hypotheses
            .clone()
            .into_iter()
            .map(|hy| DetailedDefinition::Inflection((lemma.to_string(), hy)))
            .collect();

        // just push for now
        let normalized = normalize_orthography(args.source, form);
        let reading = if normalized == *form { "" } else { form };
        let yomitan_entry = YomitanEntry(
            normalized,
            reading.into(),
            "non-lemma".into(),
            String::new(),
            0,
            deinflection_definitions,
            0,
            String::new(),
        );
        yomitan_entries.push(yomitan_entry);
    }

    yomitan_entries
}

fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);
    match serde_json::from_reader(reader) {
        core::result::Result::Ok(value) => Ok(value),
        Err(e) => {
            error!("Failed to parse JSON @ {}: {}", path.display(), e);
            Err(e)?
        }
    }
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
    args: &Args,
    diagnostics: &mut Diagnostics,
    tidy_cache: Option<Tidy>,
) -> Result<()> {
    let (lemma_dict, forms_map) = if let Some(ret) = tidy_cache {
        trace!("Skip loading Tidy result: passing it instead");
        let all_forms = ret.all_forms();
        (ret.lemma_dict, all_forms)
    } else {
        trace!("Loading Tidy result");
        let lemma_path = args.path_lemmas();
        let lemma_dict: LemmaDict = load_json(&lemma_path)?;
        let forms_path = args.path_forms();
        let forms_map: FormsMap = load_json(&forms_path)?;
        (lemma_dict, forms_map)
    };

    let yomitan_entries = make_yomitan_lemmas(args, &lemma_dict, diagnostics);
    let yomitan_forms = make_yomitan_forms(args, &forms_map);
    write_yomitan(args, yomitan_entries, yomitan_forms)
}

fn write_yomitan(
    args: &Args,
    yomitan_entries: Vec<YomitanEntry>,
    yomitan_forms: Vec<YomitanEntry>,
) -> Result<()> {
    let out_dir = args.pathdir_dict_temp();

    // clean the folder to prevent pollution from other runs
    fs::remove_dir_all(&out_dir)?;
    fs::create_dir(&out_dir)?;

    let index_string = get_index(args);
    let index_path = args.pathdir_dict_temp().join("index.json");
    fs::write(index_path, index_string)?;

    let mut bank_index = 0;
    write_in_banks(args, &out_dir, yomitan_entries, "lemmas", &mut bank_index)?;
    write_in_banks(args, &out_dir, yomitan_forms, "forms", &mut bank_index)?;

    // Now zip everything
    let file = File::create(args.path_dict())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Copy paste styles.css (include_bytes! to append them to the binary)
    let input_path = args.path_styles();
    // let styles_bytes =
    //     fs::read(&input_path).with_context(|| format!("Failed to open: {input_path:?} @ yomitan"))?;
    const STYLES_CSS: &[u8] = include_bytes!("../assets/styles.css"); // = ../args.path_styles()
    let styles_bytes = STYLES_CSS;
    let fname = input_path.file_name().and_then(|s| s.to_str()).unwrap();
    zip.start_file(fname, options)?;
    zip.write_all(styles_bytes)?;

    // Copy paste tag_bank.json
    // NOTE: In the original, we only add to the dictionary those tags that appear in at least one
    // term. I don't see the point of supporting that as of now. Just dump every possible one.
    let tag_bank = get_tag_bank_as_tag_info();
    let tag_bank_bytes = serde_json::to_vec_pretty(&tag_bank)?;
    zip.start_file("tag_bank_1.json", options)?;
    zip.write_all(&tag_bank_bytes)?;

    for entry in fs::read_dir(args.pathdir_dict_temp())? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let name = path.file_name().unwrap().to_string_lossy();
            let bytes = fs::read(&path)?;
            zip.start_file(name.as_ref(), options)?;
            zip.write_all(&bytes)?;
        }
    }

    zip.finish()?;

    pretty_println_at_path(&format!("{CHECK_C} Wrote yomitan dict"), &args.path_dict());

    Ok(())
}

/// Writes `yomitan_entries` in batches as JSONL files.
#[tracing::instrument(skip(args, out_dir, yomitan_entries, bank_index), fields(entry_ty = %entry_ty))]
fn write_in_banks(
    args: &Args,
    out_dir: &Path,
    mut yomitan_entries: Vec<YomitanEntry>,
    entry_ty: &str,
    bank_index: &mut usize,
) -> Result<()> {
    let batch_size = 25_000;
    let total_entries = yomitan_entries.len();
    let total_batch_num = total_entries.div_ceil(batch_size);
    let mut batch_num = 0;

    while !yomitan_entries.is_empty() {
        *bank_index += 1;
        batch_num += 1;

        let upto = yomitan_entries.len().min(batch_size);
        let file_path = out_dir.join(format!("term_bank_{bank_index}.json"));
        let mut file = BufWriter::new(File::create(&file_path)?);

        // potentially faster but low priority
        //
        // for entry in yomitan_entries.drain(0..upto) {
        //     serde_json::to_writer_pretty(&mut file, &entry)?;
        //     file.write_all(b"\n,")?; // fails at edges < CARE
        // }

        let batch: Vec<_> = yomitan_entries.drain(0..upto).collect();
        if args.ugly {
            serde_json::to_writer(&mut file, &batch)?;
        } else {
            serde_json::to_writer_pretty(&mut file, &batch)?;
        }

        file.flush()?;

        if batch_num > 1 {
            print!("\r\x1b[K");
        }
        pretty_print_at_path(
            &format!("Wrote {entry_ty} batch {batch_num}/{total_batch_num} ({upto} entries)"),
            &file_path,
        );
        if batch_num == total_batch_num {
            println!();
        }
        std::io::stdout().flush()?;
    }

    Ok(())
}

fn write_diagnostics(args: &Args, diagnostics: &Diagnostics) -> Result<()> {
    let pathdir = args.pathdir_diagnostics();
    fs::create_dir_all(&pathdir)?;

    write_sorted_json(
        &pathdir,
        "pos.json",
        &diagnostics.accepted_pos,
        &diagnostics.rejected_pos,
    )?;
    write_sorted_json(
        &pathdir,
        "tags.json",
        &diagnostics.accepted_tags,
        &diagnostics.rejected_tags,
    )?;

    Ok(())
}

// hacky: takes advantage of insertion order
fn sort_indexmap(map: &IndexMap<String, usize>) -> IndexMap<String, usize> {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1));
    let mut sorted_map = IndexMap::new();
    for (k, v) in entries {
        sorted_map.insert(k.into(), *v);
    }
    sorted_map
}

fn write_sorted_json(
    pathdir: &Path,
    name: &str,
    accepted: &TagCounter,
    rejected: &TagCounter,
) -> Result<()> {
    if accepted.is_empty() && rejected.is_empty() {
        return Ok(());
    }

    let accepted_sorted = sort_indexmap(accepted);
    let rejected_sorted = sort_indexmap(rejected);
    let json: IndexMap<&'static str, _> =
        IndexMap::from_iter([("rejected", rejected_sorted), ("accepted", accepted_sorted)]);

    let content = serde_json::to_string_pretty(&json)?;
    fs::write(pathdir.join(name), content)?;
    Ok(())
}

#[tracing::instrument(skip_all)]
fn run(args: &Args) -> Result<()> {
    args.setup_dirs()?;

    download_jsonl(args)?;

    if !args.skip_filter {
        filter_jsonl(args)?;
    }

    let tidy_cache = if args.skip_tidy {
        None
    } else {
        let ret = tidy(args, &args.path_jsonl())?;
        Some(ret)
    };

    let mut diagnostics = Diagnostics::new();
    if !args.skip_yomitan {
        make_yomitan(args, &mut diagnostics, tidy_cache)?;
    }

    // diagnostics.log();
    write_diagnostics(args, &diagnostics)?;

    if args.delete_files {
        fs::remove_dir_all(args.temp_dir())?;
    }

    Ok(())
}

fn setup_tracing(args: &Args) {
    // tracing_subscriber::fmt::init();
    // Same defaults as the above, without timestamps
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(if args.verbose { "debug" } else { "warn" })),
        )
        .with_span_events(FmtSpan::CLOSE)
        // .without_time()
        .with_target(true)
        .with_level(true)
        .init();
}

fn main() -> Result<()> {
    let args = Args::parse_args();
    setup_tracing(&args);
    debug!("{args:#?}");
    run(&args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Ok, Result};
    use std::path::PathBuf;

    // test via snapshots and commits like the original
    #[test]
    fn snapshot() {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
            )
            .init();

        let fixture_dir = PathBuf::from("tests");
        // have to hardcode this since we have not initialized args
        let fixture_input_dir = fixture_dir.join("kaikki");

        // iterdir and search for source-target-extract.jsonl files
        // issues no errors on wrong filenames and just ignores them
        let mut cases: Vec<(String, String)> = Vec::new();
        for entry in fs::read_dir(&fixture_input_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if let Some(fname) = path.file_name().and_then(|f| f.to_str())
                && let Some(base) = fname.strip_suffix("-extract.jsonl")
                && let Some((source, target)) = base.split_once('-')
            {
                cases.push((source.to_string(), target.to_string()));
            }
        }

        debug!("Found {} cases: {cases:?}", cases.len());

        // failfast
        for (lang, expected) in cases {
            if let Err(e) = shapshot_for_lang(&lang, &expected, &fixture_dir) {
                panic!("({lang}): {e}");
            }
        }

        // let mut errors = Vec::new();
        // for (lang, expected) in cases {
        //     if let Err(e) = shapshot_for_lang(&lang, &expected, &fixture_dir) {
        //         errors.push(format!("({lang}): {e}"));
        //     }
        // }
        //
        // assert!(
        //     errors.is_empty(),
        //     "Some expected_shape_for_lang tests failed:\n{}",
        //     errors.join("\n")
        // )
    }

    /// Delete generated artifacts from previous tests runs, if any
    fn delete_previous_output(args: &Args) -> Result<()> {
        let pathdir_dict_temp = args.pathdir_dict_temp();
        if pathdir_dict_temp.exists() {
            debug!("Deleting previous output: {pathdir_dict_temp:?}");
            fs::remove_dir_all(pathdir_dict_temp)?;
        }
        Ok(())
    }

    // NOTE: tidy and yomitan do not use args.edition in the original
    //
    // Read the expected result in the snapshot first, then compare
    fn shapshot_for_lang(source: &str, target: &str, fixture_dir: &PathBuf) -> Result<()> {
        let mut args = Args::default();

        args.set_dict_name("kty");
        args.set_source(source)?;
        args.set_target(target)?;
        args.set_root_dir(fixture_dir);

        // it would be better to do something like args.path()
        let fixture_path = fixture_dir.join(format!("kaikki/{source}-{target}-extract.jsonl"));
        if !fixture_path.exists() {
            bail!("Fixture path {fixture_path:?} does not exist")
        }
        eprintln!("***** Starting test @ {fixture_path:?}");

        delete_previous_output(&args)?;

        args.setup_dirs().unwrap(); // this makes some noise but ok
        tidy(&args, &fixture_path)?;
        let mut diagnostics = Diagnostics::new();
        make_yomitan(&args, &mut diagnostics, None)?;
        write_diagnostics(&args, &diagnostics)?;

        // check git --diff for charges in the generated json
        let output = std::process::Command::new("git")
            .args([
                "diff",
                "--color=always",
                "--unified=0", // show 0 context lines
                "--",
                // we don't care about tidy files
                &args.pathdir_dict_temp().to_string_lossy(),
            ])
            .output()?;
        if !output.stdout.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            bail!("changes!")
        }

        // diagnostics.log();

        Ok(())
    }
}
