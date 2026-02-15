use anyhow::{Context, Ok, Result};
use serde::{Deserialize, Serialize};

use std::borrow::Cow;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use crate::{Map, Set};
use crate::cli::Options;
use crate::dict::writer::write_yomitan;
use crate::download::DatasetKind;
use crate::lang::{Edition, EditionSpec, Lang, LangSpec};
use crate::models::kaikki::WordEntry;
use crate::models::yomitan::YomitanEntry;
use crate::path::{PathKind, PathManager};
use crate::utils::pretty_print_at_path;
use crate::utils::skip_because_file_exists;

const CONSOLE_PRINT_INTERVAL: i32 = 10000;

// pub type E = Box<dyn Iterator<Item = YomitanEntry>>;
pub type E = Vec<YomitanEntry>;

// Used in tests to write separate files for lemmas/forms.
pub struct LabelledYomitanEntry {
    pub label: &'static str,
    pub entries: E,
}

impl LabelledYomitanEntry {
    pub fn new(
        label: &'static str,
        // entries: impl IntoIterator<Item = YomitanEntry> + 'static,
        entries: Vec<YomitanEntry>,
    ) -> Self {
        Self {
            label,
            // entries: Box::new(entries.into_iter()),
            entries,
        }
    }
}

/// Trait for Intermediate representation. Used for postprocessing (merge, etc.) and debugging via snapshots.
///
/// The simplest form is a Vec<YomitanEntry> if we don't want to do anything fancy, cf. `DGlossary`
pub trait Intermediate: Default {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How to write `Self::I` to disk.
    ///
    /// Only called if `opts.save_temps` is set and `Dictionary::write_ir` returns true.
    #[allow(unused_variables)]
    fn write(&self, pm: &PathManager) -> Result<()> {
        Ok(())
    }
}

impl<T> Intermediate for Vec<T>
where
    T: Serialize,
{
    fn len(&self) -> usize {
        Self::len(self)
    }

    fn write(&self, pm: &PathManager) -> Result<()> {
        let writer_path = pm.dir_tidy().join("tidy.jsonl");
        let writer_file = File::create(&writer_path)?;
        let writer = BufWriter::new(&writer_file);
        if pm.opts.pretty {
            serde_json::to_writer_pretty(writer, self)?;
        } else {
            serde_json::to_writer(writer, self)?;
        }
        if !pm.opts.quiet {
            pretty_print_at_path("Wrote tidy", &writer_path);
        }
        Ok(())
    }
}

/// Trait to abstract the process of making a dictionary.
pub trait Dictionary {
    type A: TryInto<PathManager, Error = anyhow::Error>;
    type I: Intermediate;

    /// Whether to keep or not this entry.
    fn keep_if(&self, source: Lang, entry: &WordEntry) -> bool;

    /// Whether `keep_if` only depends on `entry.lang_code` matching `source`.
    ///
    /// This lets us skip full deserialization for clearly irrelevant lines.
    fn supports_lang_code_prefilter(&self) -> bool {
        false
    }

    // NOTE: Maybe we can get rid of this (blocked by mutable behaviour of the main dictionary).
    //
    /// How to preprocess a `WordEntry`. Everything that mutates `entry` should go here.
    #[allow(unused_variables)]
    fn preprocess(&self, langs: Langs, entry: &mut WordEntry, opts: &Options, irs: &mut Self::I) {}

    /// How to transform a `WordEntry` into intermediate representation.
    ///
    /// Most dictionaries only make *at most one* `Self::I` from a `WordEntry`.
    fn process(&self, langs: Langs, entry: &WordEntry, irs: &mut Self::I);

    /// Console message for found irs. It is customized for the main dictionary.
    #[allow(unused_variables)]
    fn found_ir_message(&self, key: &LangsKey, irs: &Self::I) {
        println!("Found {} irs", irs.len());
    }

    /// Whether to write or not `Self::I` to disk.
    ///
    /// Compare to `save_temp`, that rules if `Self::I` AND the `term_banks` are written to disk.
    ///
    /// This is mainly a debug function, in order to allow not writing the ir `Self::I` to disk for
    /// minor dictionaries in the testsuite. It is only set to true in the main dictionary.
    fn write_ir(&self) -> bool {
        false
    }

    /// How to postprocess the intermediate representation.
    ///
    /// This can be implemented to merge entries from different edition, to postprocess tags etc.
    #[allow(unused_variables)]
    fn postprocess(&self, irs: &mut Self::I) {}

    /// How to convert `Self::I` into one or more yomitan entries.
    fn to_yomitan(&self, langs: Langs, irs: Self::I) -> Vec<LabelledYomitanEntry>;
}

fn rejected(entry: &WordEntry, opts: &Options) -> bool {
    opts.reject.iter().any(|(k, v)| k.field_value(entry) == v)
        || !opts.filter.iter().all(|(k, v)| k.field_value(entry) == v)
}

#[inline]
fn skip_json_whitespace(line: &[u8], mut idx: usize) -> usize {
    while idx < line.len() {
        match line[idx] {
            b' ' | b'\t' | b'\r' | b'\n' => idx += 1,
            _ => break,
        }
    }
    idx
}

#[inline]
fn skip_json_string(line: &[u8], idx: usize) -> Option<usize> {
    if line.get(idx).copied()? != b'"' {
        return None;
    }
    let mut i = idx + 1;
    while i < line.len() {
        let b = line[i];
        if b == b'"' {
            return Some(i + 1);
        }
        if b == b'\\' {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

#[inline]
fn skip_json_compound(line: &[u8], idx: usize) -> Option<usize> {
    match line.get(idx).copied()? {
        b'{' | b'[' => {}
        _ => return None,
    }

    let mut depth: u32 = 1;
    let mut i = idx + 1;
    let mut in_string = false;

    while i < line.len() {
        let b = line[i];
        if in_string {
            match b {
                b'"' => in_string = false,
                b'\\' => {
                    i += 1;
                    if i >= line.len() {
                        return None;
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }

        match b {
            b'"' => {
                in_string = true;
                i += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => i += 1,
        }
    }

    None
}

#[inline]
fn skip_json_primitive(line: &[u8], idx: usize) -> Option<usize> {
    let mut i = idx;
    while i < line.len() {
        match line[i] {
            b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n' => return Some(i),
            _ => i += 1,
        }
    }
    Some(i)
}

fn skip_json_value(line: &[u8], idx: usize) -> Option<usize> {
    let i = skip_json_whitespace(line, idx);
    match line.get(i).copied()? {
        b'"' => skip_json_string(line, i),
        b'{' | b'[' => skip_json_compound(line, i),
        _ => skip_json_primitive(line, i),
    }
}

/// Fast path used by source-language dictionaries to avoid full serde parsing
/// when `lang_code` doesn't match.
fn probe_top_level_lang_code(line: &[u8]) -> Option<&str> {
    let mut i = skip_json_whitespace(line, 0);
    if line.get(i).copied()? != b'{' {
        return None;
    }
    i += 1;

    loop {
        i = skip_json_whitespace(line, i);
        match line.get(i).copied()? {
            b'}' => return None,
            b'"' => {}
            _ => return None,
        }

        let key_start = i + 1;
        i = skip_json_string(line, i)?;
        let key_end = i - 1;

        i = skip_json_whitespace(line, i);
        if line.get(i).copied()? != b':' {
            return None;
        }
        i += 1;
        i = skip_json_whitespace(line, i);

        if &line[key_start..key_end] == b"lang_code" {
            if line.get(i).copied()? != b'"' {
                return None;
            }
            let value_start = i + 1;
            i = skip_json_string(line, i)?;
            let value_end = i - 1;
            if line[value_start..value_end].contains(&b'\\') {
                return None;
            }
            return std::str::from_utf8(&line[value_start..value_end]).ok();
        }

        i = skip_json_value(line, i)?;
        i = skip_json_whitespace(line, i);

        match line.get(i).copied()? {
            b',' => i += 1,
            b'}' => return None,
            _ => return None,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct LangCodeProbe<'a> {
    #[serde(borrow)]
    lang_code: Cow<'a, str>,
}

impl<'a> Default for LangCodeProbe<'a> {
    fn default() -> Self {
        Self {
            lang_code: Cow::Borrowed(""),
        }
    }
}

use crate::dict::{DGlossary, DGlossaryExtended, DIpa, DIpaMerged, DMain};

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct LangsKey {
    pub edition: EditionSpec,
    pub source: Lang,
    pub target: Lang,
}

pub trait IterLang {
    fn iter_langs(&self, edition: Edition, source: LangSpec, target: LangSpec) -> Vec<Langs>;

    /// Maps an iteration Langs to its aggregation key.
    ///
    /// Used by merged dictionaries to combine data across editions.
    fn langs_to_key(&self, langs: Langs) -> LangsKey {
        LangsKey {
            edition: EditionSpec::One(langs.edition),
            source: langs.source,
            target: langs.target,
        }
    }
}

fn cartesian(edition: Edition, source: LangSpec, target: LangSpec) -> Vec<Langs> {
    let mut out = Vec::new();
    for s in source.variants() {
        for t in target.variants() {
            out.push(Langs::new(edition, s, t));
        }
    }
    out
}

impl IterLang for DMain {
    fn iter_langs(&self, edition: Edition, source: LangSpec, target: LangSpec) -> Vec<Langs> {
        match target {
            LangSpec::All => cartesian(edition, source, LangSpec::One(edition.into())),
            _ => cartesian(edition, source, target),
        }
    }
}

impl IterLang for DIpa {
    fn iter_langs(&self, edition: Edition, source: LangSpec, target: LangSpec) -> Vec<Langs> {
        match target {
            LangSpec::All => cartesian(edition, source, LangSpec::One(edition.into())),
            _ => cartesian(edition, source, target),
        }
    }
}

impl IterLang for DGlossary {
    fn iter_langs(&self, edition: Edition, source: LangSpec, target: LangSpec) -> Vec<Langs> {
        match source {
            LangSpec::All => cartesian(edition, LangSpec::One(edition.into()), target),
            _ => cartesian(edition, source, target),
        }
    }
}

impl IterLang for DIpaMerged {
    fn iter_langs(&self, edition: Edition, _source: LangSpec, target: LangSpec) -> Vec<Langs> {
        match target {
            LangSpec::One(t) => vec![Langs::new(edition, t, t)],
            LangSpec::All => {
                let mut out = Vec::new();
                for t in target.variants() {
                    out.push(Langs::new(edition, t, t));
                }
                out
            }
        }
    }

    fn langs_to_key(&self, langs: Langs) -> LangsKey {
        // Collapse all editions into one logical key
        LangsKey {
            edition: EditionSpec::All,
            source: langs.source,
            target: langs.target,
        }
    }
}

impl IterLang for DGlossaryExtended {
    fn iter_langs(&self, edition: Edition, source: LangSpec, target: LangSpec) -> Vec<Langs> {
        match source {
            LangSpec::All => cartesian(edition, LangSpec::One(edition.into()), target),
            _ => cartesian(edition, source, target),
        }
    }

    fn langs_to_key(&self, langs: Langs) -> LangsKey {
        LangsKey {
            edition: EditionSpec::All,
            source: langs.source,
            target: langs.target,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Langs {
    pub edition: Edition,
    pub source: Lang,
    pub target: Lang,
}

impl Langs {
    pub const fn new(edition: Edition, source: Lang, target: Lang) -> Self {
        Self {
            edition,
            source,
            target,
        }
    }
}

impl fmt::Debug for Langs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Langs")
            .field(&self.edition)
            .field(&self.source)
            .field(&self.target)
            .finish()
    }
}

/// Depending on source, which jsonl should we consume to make this dictionary.
pub trait DatasetStrategy {
    fn dataset_request(&self, source: LangSpec) -> DatasetRequest;
}

#[derive(Debug, Clone, Copy)]
pub enum DatasetRequest {
    /// Read the unfiltered edition-wide JSONL
    UnfilteredEdition,

    /// Read the filtered JSONL for (edition, lang)
    FilteredLang(Lang),

    FilteredEdition,
}

impl DatasetStrategy for DMain {
    fn dataset_request(&self, source: LangSpec) -> DatasetRequest {
        match source {
            LangSpec::All => DatasetRequest::UnfilteredEdition,
            LangSpec::One(lang) => DatasetRequest::FilteredLang(lang),
        }
    }
}

impl DatasetStrategy for DIpa {
    fn dataset_request(&self, source: LangSpec) -> DatasetRequest {
        match source {
            LangSpec::All => DatasetRequest::UnfilteredEdition,
            LangSpec::One(lang) => DatasetRequest::FilteredLang(lang),
        }
    }
}

impl DatasetStrategy for DGlossary {
    fn dataset_request(&self, source: LangSpec) -> DatasetRequest {
        match source {
            LangSpec::All => DatasetRequest::FilteredEdition,
            // WARN: The post-processed (filtered) versions of the English edition have their
            // translations in the sense and not in the top-level, which invalidates our logic.
            LangSpec::One(lang) => DatasetRequest::FilteredLang(lang),
            // LangSpec::One(lang) => DatasetRequest::UnfilteredEdition,
        }
    }
}

impl DatasetStrategy for DIpaMerged {
    fn dataset_request(&self, source: LangSpec) -> DatasetRequest {
        match source {
            LangSpec::All => DatasetRequest::FilteredEdition,
            LangSpec::One(lang) => DatasetRequest::FilteredLang(lang),
        }
    }
}

impl DatasetStrategy for DGlossaryExtended {
    fn dataset_request(&self, source: LangSpec) -> DatasetRequest {
        match source {
            LangSpec::All => DatasetRequest::FilteredEdition,
            LangSpec::One(lang) => DatasetRequest::FilteredLang(lang),
        }
    }
}

pub const fn edition_to_kind(edition: Edition) -> DatasetKind {
    match edition {
        Edition::En => DatasetKind::Filtered,
        _ => DatasetKind::Unfiltered,
    }
}

fn find_or_download_jsonl(
    edition: Edition,
    lang: Option<Lang>,
    kind: DatasetKind,
    pm: &PathManager,
) -> Result<PathBuf> {
    let paths_candidates = pm.dataset_paths(edition, lang);
    let kinds_to_check = match kind {
        DatasetKind::Filtered => vec![PathKind::Filtered],
        DatasetKind::Unfiltered => vec![PathKind::Unfiltered, PathKind::Filtered],
    };
    let of_kind: Vec<_> = paths_candidates
        .inner
        .iter()
        .filter(|p| kinds_to_check.contains(&p.kind))
        .collect();

    if !pm.opts.redownload
        && let Some(existing) = of_kind.iter().find(|p| p.path.exists())
    {
        if !pm.opts.quiet {
            skip_because_file_exists(&format!("download ({kind:?})"), &existing.path);
        }
        return Ok(existing.path.clone());
    }

    let path = &of_kind
        .iter()
        .next_back()
        .unwrap_or_else(|| {
            panic!(
                "No path available for the requested kind: {kind:?}, \
             for edition={edition:?} and lang={lang:?} | {paths_candidates:?}"
            )
        })
        .path;

    // TODO: remove this once it's done: it prevents downloading in the testsuite
    // anyhow::bail!(
    //     "Downloading is disabled but JSONL file was not found @ {}",
    //     path.display()
    // );

    #[cfg(feature = "html")]
    crate::download::download_jsonl(edition, lang, kind, path, false)?;

    Ok(path.clone())
}

fn iter_datasets<'a, D: DatasetStrategy>(
    dict: &'a D,
    pm: &'a PathManager,
) -> impl Iterator<Item = Result<(Edition, PathBuf)>> + 'a {
    let (edition_pm, source_pm, _) = pm.langs();

    edition_pm.variants().into_iter().map(move |edition| {
        let (lang, kind) = match dict.dataset_request(source_pm) {
            DatasetRequest::UnfilteredEdition => (None, DatasetKind::Unfiltered),
            DatasetRequest::FilteredEdition => (Some(edition.into()), DatasetKind::Unfiltered),
            DatasetRequest::FilteredLang(lang) => (Some(lang), edition_to_kind(edition)),
        };
        let path_jsonl = find_or_download_jsonl(edition, lang, kind, pm)?;
        tracing::debug!("edition: {edition}, path: {}", path_jsonl.display());

        Ok((edition, path_jsonl))
    })
}

pub fn make_dict<D: Dictionary + IterLang + DatasetStrategy>(
    dict: D,
    raw_args: D::A,
) -> Result<()> {
    let pm: &PathManager = &raw_args.try_into()?;
    let (_, source_pm, target_pm) = pm.langs();
    let opts = &pm.opts;

    pm.setup_dirs()?;

    let capacity = 256 * (1 << 10); // default is 8 * (1 << 10) := 8KB
    let mut line = Vec::with_capacity(1 << 10);
    // (source, target) -> D::I
    let mut irs_map: Map<LangsKey, D::I> = Map::default();

    for pair in iter_datasets(&dict, pm) {
        let (edition, path_jsonl) = pair?;
        let langs_for_edition = dict.iter_langs(edition, source_pm, target_pm);
        let lang_code_prefilter = if dict.supports_lang_code_prefilter()
            && opts.first < 0
            && opts.filter.is_empty()
            && opts.reject.is_empty()
        {
            Some(
                langs_for_edition
                    .iter()
                    .map(|langs| langs.source.as_ref().to_string())
                    .collect::<Set<_>>(),
            )
        } else {
            None
        };

        let reader_file = File::open(&path_jsonl)?;
        let mut reader = BufReader::with_capacity(capacity, reader_file);

        let mut line_count = 0;
        let mut accepted_count = 0;

        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break; // EOF
            }

            line_count += 1;

            if !opts.quiet && line_count % CONSOLE_PRINT_INTERVAL == 0 {
                print!("Processed {line_count} lines...\r");
                std::io::stdout().flush()?;
            }

            if let Some(lang_code_prefilter) = &lang_code_prefilter {
                let keep = if let Some(lang_code) = probe_top_level_lang_code(&line) {
                    lang_code_prefilter.contains(lang_code)
                } else {
                    let probe: LangCodeProbe = serde_json::from_slice(&line).with_context(
                        || "Error decoding JSON @ make_dict (lang_code prefilter fallback)",
                    )?;
                    lang_code_prefilter.contains(probe.lang_code.as_ref())
                };
                if !keep {
                    continue;
                }
            }

            let mut entry: WordEntry =
                serde_json::from_slice(&line).with_context(|| "Error decoding JSON @ make_dict")?;

            if (!opts.filter.is_empty() || !opts.reject.is_empty()) && rejected(&entry, opts) {
                continue;
            }

            accepted_count += 1;
            if accepted_count == opts.first {
                break;
            }

            for &langs in &langs_for_edition {
                if dict.keep_if(langs.source, &entry) {
                    let key = dict.langs_to_key(langs);
                    let irs = irs_map.entry(key).or_default();
                    dict.preprocess(langs, &mut entry, opts, irs);
                    dict.process(langs, &entry, irs);
                }
            }
        }

        if !opts.quiet {
            println!("Processed {line_count} lines. Accepted {accepted_count} lines.");
        }

        // tracing::debug!(
        //     "After {edition}: irs_map has {} keys, {} total entries",
        //     irs_map.len(),
        //     irs_map.values().map(|ir| ir.len()).sum::<usize>()
        // );
    }

    if irs_map.len() > 1 {
        tracing::debug!("Matrix ({}): {:?}", irs_map.len(), irs_map.keys());
    }

    for (key, mut irs) in irs_map {
        if !opts.quiet {
            dict.found_ir_message(&key, &irs);
        }

        if irs.is_empty() {
            continue;
        }

        dict.postprocess(&mut irs);

        if opts.save_temps && dict.write_ir() {
            irs.write(pm)?;
        }

        if !opts.skip_yomitan {
            let mut pm2 = pm.clone();
            let source = key.source;
            let target = key.target;
            pm2.set_source(source.into());
            pm2.set_target(target.into());
            pm2.setup_dirs()?;
            tracing::trace!("calling to_yomitan with (source={source}, target={target})",);
            let labelled_entries = match key.edition {
                EditionSpec::All => {
                    // HACK: we don't use the edition for IpaMerged: use a dummy for now
                    let langs = Langs::new(Edition::Zh, key.source, key.target);
                    dict.to_yomitan(langs, irs)
                }
                EditionSpec::One(edition) => {
                    let langs = Langs::new(edition, key.source, key.target);
                    dict.to_yomitan(langs, irs)
                }
            };
            write_yomitan(source, target, opts, &pm2, labelled_entries)?;
        }
    }

    Ok(())
}
// TODO: rename this to make_dicts when done, and keep the original

#[cfg(test)]
mod tests {
    use super::probe_top_level_lang_code;

    #[test]
    fn probe_lang_code_returns_top_level_value() {
        let line = br#"{"word":"x","lang_code":"ja","translations":[{"lang_code":"en"}]}"#;
        assert_eq!(probe_top_level_lang_code(line), Some("ja"));
    }

    #[test]
    fn probe_lang_code_ignores_nested_values_before_key() {
        let line = br#"{"meta":{"lang_code":"en"},"word":"x","lang_code":"ja"}"#;
        assert_eq!(probe_top_level_lang_code(line), Some("ja"));
    }

    #[test]
    fn probe_lang_code_ignores_string_content() {
        let line = br#"{"word":"\"lang_code\":\"en\"","lang_code":"ja"}"#;
        assert_eq!(probe_top_level_lang_code(line), Some("ja"));
    }

    #[test]
    fn probe_lang_code_handles_whitespace() {
        let line = b"{\n  \"word\": \"x\",\n  \"lang_code\": \"de\"\n}";
        assert_eq!(probe_top_level_lang_code(line), Some("de"));
    }

    #[test]
    fn probe_lang_code_none_when_missing() {
        let line = br#"{"word":"x","translations":[{"lang_code":"en"}]}"#;
        assert_eq!(probe_top_level_lang_code(line), None);
    }
}
