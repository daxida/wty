pub mod cli;
pub mod diagnostic;
pub mod dict;
pub mod download;
pub mod lang;
pub mod locale;
pub mod models;
pub mod path;
pub mod tags;
pub mod utils;

use anyhow::{Context, Ok, Result};
use fxhash::FxBuildHasher;
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;
#[allow(unused)]
use tracing::{Level, debug, error, info, span, trace, warn};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::cli::ArgsOptions;
use crate::diagnostic::Diagnostics;
use crate::dict::get_index;
#[cfg(feature = "html")]
use crate::download::download_jsonl;
use crate::lang::{EditionLang, Lang};
use crate::models::kaikki::WordEntry;
use crate::models::yomitan::YomitanEntry;
use crate::path::PathManager;
use crate::tags::get_tag_bank_as_tag_info;
use crate::utils::{
    CHECK_C, pretty_print_at_path, pretty_println_at_path, skip_because_file_exists,
};

pub type Map<K, V> = IndexMap<K, V, FxBuildHasher>; // Preserve insertion order
pub type Set<K> = IndexSet<K, FxBuildHasher>;

const STYLES_CSS: &[u8] = include_bytes!("../assets/styles.css");
const STYLES_CSS_EXPERIMENTAL: &[u8] = include_bytes!("../assets/styles_experimental.css");

type LabelledYomitanEntry = (&'static str, Vec<YomitanEntry>);

enum BankSink<'a> {
    Disk,
    Zip(&'a mut ZipWriter<File>, SimpleFileOptions),
}

/// Write lemma / form / whatever banks to either disk or zip.
///
/// If `save_temps` is true, we assume that the user is debugging and does not need the zip.
fn write_yomitan(
    source: Lang,
    target: Lang,
    options: &ArgsOptions,
    pm: &PathManager,
    labelled_entries: &[LabelledYomitanEntry],
) -> Result<()> {
    let mut bank_index = 0;

    if options.save_temps {
        let out_dir = pm.dir_temp_dict();
        fs::create_dir_all(&out_dir)?;
        for (entry_ty, entries) in labelled_entries {
            write_banks(
                options.pretty,
                options.quiet,
                entries,
                &mut bank_index,
                entry_ty,
                &out_dir,
                BankSink::Disk,
            )?;
        }

        if !options.quiet {
            pretty_println_at_path(&format!("{CHECK_C} Wrote temp data"), &out_dir);
        }
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
                options.quiet,
                entries,
                &mut bank_index,
                entry_ty,
                &writer_path,
                BankSink::Zip(&mut zip, zip_options),
            )?;
        }

        zip.finish()?;

        if !options.quiet {
            pretty_println_at_path(&format!("{CHECK_C} Wrote yomitan dict"), &writer_path);
        }
    }

    Ok(())
}

/// Writes `yomitan_entries` in batches to `out_sink` (either disk or a zip).
#[tracing::instrument(skip_all)]
fn write_banks(
    pretty: bool,
    quiet: bool,
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

        if !quiet {
            if bank_num > 1 {
                print!("\r\x1b[K");
            }
            pretty_print_at_path(
                &format!(
                    "Wrote yomitan {entry_ty} bank {bank_num}/{total_bank_num} ({upto} entries)"
                ),
                &file_path,
            );
            std::io::stdout().flush()?;
        }

        start = end;
    }

    if !quiet && bank_num == total_bank_num {
        println!();
    }

    Ok(())
}

/// Trait for Intermediate representation. Used for postprocessing (merge, etc.) and debugging via snapshots.
///
/// The simplest form is a Vec<YomitanEntry> if we don't want to do anything fancy, cf. `DGlossary`
pub trait Intermediate: Default {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How to write `Self::I` to disk. This is only called if `options.save_temps` is set and
    /// `Dictionary::write_ir` returns true
    ///
    /// The default blank implementation does nothing.
    #[allow(unused_variables)]
    fn write(&self, pm: &PathManager, options: &ArgsOptions) -> Result<()> {
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

    fn write(&self, pm: &PathManager, options: &ArgsOptions) -> Result<()> {
        let writer_path = pm.dir_tidy().join("tidy.jsonl");
        let writer_file = File::create(&writer_path)?;
        let writer = BufWriter::new(&writer_file);
        if options.pretty {
            serde_json::to_writer_pretty(writer, self)?;
        } else {
            serde_json::to_writer(writer, self)?;
        }
        if !options.quiet {
            pretty_print_at_path("Wrote tidy", &writer_path);
        }
        Ok(())
    }
}

// If this ends up having too much overhead for dictionaries that do not use Self::I, consider this
// other trait:
//
// trait SimpleDictionary {
//     fn paths_jsonl_raw(&self, pm: &PathManager) -> Vec<(EditionLang, PathBuf)>;
//     fn process(&self, source: Lang, target: Lang, entry: &WordEntry) -> Vec<YomitanEntry>;
// }
//
// and rewrite make_dict to instead just store YomitanEntries.
//
/// Trait to abstract the process of writing a dictionary.
pub trait Dictionary {
    type I: Intermediate;

    // TODO: support filter (cache)

    // NOTE:Maybe in the future we can get rid of this. It requires cleaning up the legacy mutable
    // behaviour of the main dictionary.
    //
    /// How to preprocess a `WordEntry`.
    ///
    /// Inspired by the Main dictionary, everything that mutates `word_entry` should go here.
    ///
    /// The default blank implementation does nothing.
    #[allow(unused_variables)]
    fn preprocess(
        &self,
        edition: EditionLang,
        source: Lang,
        target: Lang,
        word_entry: &mut WordEntry,
        options: &ArgsOptions,
        irs: &mut Self::I,
    ) {
    }

    /// How to transform a `WordEntry` into intermediate representation.
    ///
    /// Most dictionaries only make *at most one* `Self::I` from a `WordEntry`.
    fn process(
        &self,
        edition: EditionLang,
        source: Lang,
        target: Lang,
        word_entry: &WordEntry,
        irs: &mut Self::I,
    );

    /// Console message for found entries.
    ///
    /// It happens to be customized for the main dictionary.
    fn found_ir_message(&self, irs: &Self::I) {
        println!("Found {} entries", irs.len());
    }

    /// Whether to write or not `Self::I` to disk
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
    /// This can be implemented, for instance, to merge entries from different editions, or to
    /// postprocess forms, tags etc.
    ///
    /// The default blank implementation does nothing.
    #[allow(unused_variables)]
    fn postprocess(&self, irs: &mut Self::I) {}

    // Does not have access to WordEntry!
    //
    // Returns a Vec<LabelledYomitanEntry> instead of Vec<YomitanEntry> because that is the
    // argument type of write_yomitan, but it should be doable to change it back to
    // Vec<YomitanEntry>
    fn to_yomitan(
        &self,
        edition: EditionLang,
        source: Lang,
        target: Lang,
        options: &ArgsOptions,
        diagnostics: &mut Diagnostics,
        irs: Self::I,
    ) -> Vec<LabelledYomitanEntry>;

    /// How to write diagnostics, if any.
    ///
    /// The default blank implementation does nothing.
    #[allow(unused_variables)]
    fn write_diagnostics(&self, pm: &PathManager, diagnostics: &Diagnostics) -> Result<()> {
        Ok(())
    }
}

fn find_or_download_jsonl(
    edition: EditionLang,
    lang: Lang,
    paths: &[PathBuf],
    options: &ArgsOptions,
) -> Result<PathBuf> {
    let first_path_found = paths.iter().find(|pbuf| pbuf.exists());

    if let (false, Some(pbuf)) = (options.redownload, first_path_found) {
        if !options.quiet {
            skip_because_file_exists("download", pbuf);
        }
        Ok(pbuf.clone())
    } else {
        let path_jsonl_raw_of_download = paths.last().unwrap();
        #[cfg(feature = "html")]
        download_jsonl(edition, lang, path_jsonl_raw_of_download, options.quiet)?;
        Ok(path_jsonl_raw_of_download.clone())
    }
}

const CONSOLE_PRINT_INTERVAL: i32 = 10000;

pub fn make_dict<D: Dictionary>(dict: D, options: &ArgsOptions, pm: &PathManager) -> Result<()> {
    let (edition_pm, source_pm, target_pm) = pm.langs();

    pm.setup_dirs()?;

    // rust default is 8 * (1 << 10) := 8KB
    let capacity = 256 * (1 << 10);
    let mut line = Vec::with_capacity(1 << 10);
    let mut entries = D::I::default();

    for (edition, paths) in pm.paths_jsonl_raw() {
        let path_jsonl_raw = find_or_download_jsonl(edition, source_pm, &paths, options)?;
        tracing::debug!("path_jsonl_raw: {}", path_jsonl_raw.display());

        let reader_path = &path_jsonl_raw;
        let reader_file = File::open(reader_path)?;
        let mut reader = BufReader::with_capacity(capacity, reader_file);

        let mut cached_lines = Vec::new();
        let mut line_count = 0;
        let mut accepted_count = 0;

        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break; // EOF
            }

            line_count += 1;

            let mut word_entry: WordEntry =
                serde_json::from_slice(&line).with_context(|| "Error decoding JSON @ make_dict")?;

            if !options.quiet && line_count % CONSOLE_PRINT_INTERVAL == 0 {
                print!("Processed {line_count} lines...\r");
                std::io::stdout().flush()?;
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

            if options.cache_filter {
                cached_lines.extend(line.clone());
            }

            accepted_count += 1;
            if accepted_count == options.first {
                break;
            }

            dict.preprocess(
                edition,
                source_pm,
                target_pm,
                &mut word_entry,
                options,
                &mut entries,
            );

            dict.process(edition, source_pm, target_pm, &word_entry, &mut entries);
        }

        if !options.quiet {
            println!("Processed {line_count} lines. Accepted {accepted_count} lines.");
        }

        if options.cache_filter {
            let mut writer_file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(reader_path)?;
            writer_file.write_all(&cached_lines)?;
        }
    }

    if !options.quiet {
        dict.found_ir_message(&entries);
    }

    if entries.is_empty() {
        return Ok(());
    }

    dict.postprocess(&mut entries);
    // println!("Postprocessed down to {} entries", entries.len());

    if options.save_temps && dict.write_ir() {
        entries.write(pm, options)?;
    }

    if !options.skip_yomitan {
        let mut diagnostics = Diagnostics::default();

        let labelled_entries = dict.to_yomitan(
            // HACK: This unwrap_or is only for GlossaryExtended and works as a filler
            // because the edition is not used in the implementation of to_yomitan for that dict.
            // It is basically here to not crash the code. Happy face.
            edition_pm.try_into().unwrap_or(EditionLang::En),
            source_pm,
            target_pm,
            options,
            &mut diagnostics,
            entries,
        );

        dict.write_diagnostics(pm, &diagnostics)?;

        write_yomitan(source_pm, target_pm, options, pm, &labelled_entries)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cli::{GlossaryArgs, GlossaryLangs, MainArgs, MainLangs};
    use crate::dict::{DGlossary, DIpa, DMain};
    use crate::path::DictionaryType;

    use anyhow::{Ok, Result};
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::format::FmtSpan;

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

    fn setup_tracing_test() {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(FmtSpan::CLOSE)
            .with_target(true)
            .with_level(true)
            .init();
    }

    /// Test via snapshots and git diffs like the original
    #[test]
    fn snapshot() {
        setup_tracing_test();

        let fixture_dir = PathBuf::from("tests");
        // have to hardcode this since we have not initialized args
        let fixture_input_dir = fixture_dir.join("kaikki");

        // Nuke the output dir to prevent pollution
        // It has the disadvantage of massive diffs if we failfast.
        //
        // let fixture_output_dir = fixture_dir.join("dict");
        // Don't crash if there is no output dir. It may happen if we nuke it manually
        // let _ = fs::remove_dir_all(fixture_output_dir);

        let mut cases = Vec::new();
        let mut langs_in_testsuite = Vec::new();

        // iterdir and search for source-target-extract.jsonl files
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

        tracing::debug!("Found {} cases: {cases:?}", cases.len());

        // failfast
        // main
        for (source, target) in &cases {
            let Result::Ok(target) = EditionLang::try_from(*target) else {
                continue; // skip if target is not edition
            };
            let args = fixture_main_args(target, *source, target, &fixture_dir);
            let pm = PathManager::new(DictionaryType::Main, &args);

            if let Err(e) = shapshot_main(&args.options, &pm) {
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
                make_dict(DGlossary, &args.options, &pm).unwrap();
            }
        }

        // ipa
        for (source, target) in &cases {
            let Result::Ok(target) = EditionLang::try_from(*target) else {
                continue; // skip if target is not edition
            };
            let args = fixture_main_args(target, *source, target, &fixture_dir);
            let pm = PathManager::new(DictionaryType::Ipa, &args);
            make_dict(DIpa, &args.options, &pm).unwrap();
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

    /// Read the expected result in the snapshot first, then git diff
    fn shapshot_main(options: &ArgsOptions, pm: &PathManager) -> Result<()> {
        delete_previous_output(pm)?;
        make_dict(DMain, options, pm)?;
        check_git_diff(pm)?;
        Ok(())
    }

    /// Run git --diff for charges in the generated json
    fn check_git_diff(pm: &PathManager) -> Result<()> {
        let output = std::process::Command::new("git")
            .args([
                "diff",
                "--color=always",
                "--unified=0", // show 0 context lines
                "--",
                // we don't care about changes in tidy files
                &pm.dir_temp_dict().to_string_lossy(),
            ])
            .output()?;
        if !output.stdout.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            anyhow::bail!("changes!")
        }

        Ok(())
    }
}
