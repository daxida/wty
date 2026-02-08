//! Unused - experimental

#![allow(unused)]

use core::panic;
use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::Result;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use rkyv::Archived;
use rusqlite::{Connection, params};

use crate::dict::{DGlossaryExtended, DIpa, DIpaMerged, edition_to_kind};
use crate::dict::{
    DMain, DatasetRequest, DatasetStrategy, Dictionary, Intermediate, IterLang, Langs, LangsKey,
};
use crate::download::DatasetKind;
use crate::lang::{Edition, EditionSpec, Lang, LangSpec};
use crate::models::kaikki::WordEntry;
use crate::path::{PathKind, PathManager};
use crate::utils::skip_because_file_exists;
use crate::{Map, cli::GlossaryLangs};
use crate::{cli::IpaArgs, dict::writer::write_yomitan};
use crate::{
    cli::{DictName, GlossaryArgs, MainArgs, MainLangs, Options},
    dict::DGlossary,
};

// runs main source all
fn release_main(edition: Edition) {
    let sources = Lang::all();
    sources.par_iter().for_each(|source| {
        let start = Instant::now();

        let langs = match (edition, source) {
            (Edition::Simple, Lang::Simple) => MainLangs {
                source: LangSpec::One(edition.into()),
                target: EditionSpec::One(edition),
            },
            (Edition::Simple, _) | (_, Lang::Simple) => return,
            (Edition::En, Lang::Fi) => {
                tracing::warn!("Skipping finnish-english...");
                return;
            }
            _ => MainLangs {
                source: LangSpec::One(*source),
                target: EditionSpec::One(edition),
            },
        };

        let args = MainArgs {
            langs,
            dict_name: DictName::default(),
            options: Options {
                quiet: true,
                root_dir: "data".into(),
                ..Default::default()
            },
        };

        match make_dict(DMain, args) {
            Ok(_) => pp("main", *source, edition.into(), start),
            Err(err) => tracing::error!("[main-{source}-{edition}] ERROR: {err:?}"),
        }
    });
}

fn release_ipa(edition: Edition) {
    let sources = Lang::all();
    sources.par_iter().for_each(|source| {
        let start = Instant::now();

        let langs = match (edition, source) {
            (Edition::Simple, Lang::Simple) => MainLangs {
                source: LangSpec::One(edition.into()),
                target: EditionSpec::One(edition),
            },
            (Edition::Simple, _) | (_, Lang::Simple) => return,
            _ => MainLangs {
                source: LangSpec::One(*source),
                target: EditionSpec::One(edition),
            },
        };

        let args = IpaArgs {
            langs,
            dict_name: DictName::default(),
            options: Options {
                quiet: true,
                root_dir: "data".into(),
                ..Default::default()
            },
        };

        match make_dict(DIpa, args) {
            Ok(_) => pp("ipa", *source, edition.into(), start),
            Err(err) => tracing::error!("[ipa-{source}-{edition}] ERROR: {err:?}"),
        }
    });
}

fn release_glossary(edition: Edition) {
    let targets = Lang::all();
    targets.par_iter().for_each(|target| {
        let start = Instant::now();

        let langs = match (edition, target) {
            (Edition::Simple, Lang::Simple) => GlossaryLangs {
                source: EditionSpec::One(edition),
                target: LangSpec::One(edition.into()),
            },
            (Edition::Simple, _) | (_, Lang::Simple) => return,
            _ if Lang::from(edition) == *target => return,
            _ => GlossaryLangs {
                source: EditionSpec::One(edition),
                target: LangSpec::One(*target),
            },
        };

        let args = GlossaryArgs {
            langs,
            dict_name: DictName::default(),
            options: Options {
                quiet: true,
                root_dir: "data".into(),
                ..Default::default()
            },
        };

        match make_dict(DGlossary, args) {
            // Order may be wrong
            Ok(_) => pp("gloss", *target, edition.into(), start),
            Err(err) => tracing::error!("[gloss-{target}-{edition}] ERROR: {err:?}"),
        }
    });
}

// Pretty print utility
fn pp(dict_name: &str, first_lang: Lang, second_lang: Lang, time: Instant) {
    return;
    eprintln!(
        "[{dict_name}-{first_lang}-{second_lang}] done in {:.2?}",
        time.elapsed()
    )
}

pub fn release() -> Result<()> {
    let start = Instant::now();

    let editions = Edition::all(); // WARN: OOMS 24GB pool
    let editions: Vec<_> = Edition::all()
        .into_iter()
        .filter(|ed| *ed != Edition::En)
        .collect();
    let editions = vec![Edition::En, Edition::De, Edition::Fr];
    // let editions = vec![Edition::De, Edition::Fr];
    // let editions = vec![Edition::Fr];
    // let editions = vec![Edition::El]; // target for main

    // editions.par_iter().for_each(|target| {
    //     // HACK: be sure to init the db before running when resetting...
    //     tracing::warn!("Hacky db init for {target}");
    //     let args = MainArgs {
    //         langs: MainLangs {
    //             source: match *target {
    //                 Edition::Simple => LangSpec::One(Lang::Simple),
    //                 _ => LangSpec::One(Lang::El),
    //             },
    //             target: EditionSpec::One(*target),
    //         },
    //         dict_name: DictName::default(),
    //         options: Options {
    //             quiet: true,
    //             root_dir: "data".into(),
    //             ..Default::default()
    //         },
    //     };
    //     let pm: &PathManager = &args.try_into().unwrap();
    //     let _ = WiktextractDb::open_from_lang(*target, pm).unwrap();
    // });

    editions.par_iter().for_each(|edition| {
        release_main(*edition);
        release_ipa(*edition);
        release_glossary(*edition);
    });

    let elapsed = start.elapsed();
    println!("Finished in {:.2?}", elapsed);

    Ok(())
}

pub struct WiktextractDb {
    pub conn: Connection,
}

fn find_or_download_jsonl_simple(edition: Edition, pm: &PathManager) -> Result<PathBuf> {
    let paths_candidates = pm.dataset_paths(edition, None);
    let kinds_to_check = vec![PathKind::Unfiltered];
    let of_kind: Vec<_> = paths_candidates
        .inner
        .iter()
        .filter(|p| kinds_to_check.contains(&p.kind))
        .collect();

    if !pm.opts.redownload
        && let Some(existing) = of_kind.iter().find(|p| p.path.exists())
    {
        if !pm.opts.quiet {
            skip_because_file_exists(&format!("download"), &existing.path);
        }
        return Ok(existing.path.clone());
    }

    let path = &of_kind.iter().next_back().unwrap().path;

    // TODO: remove this once it's done: it prevents downloading in the testsuite
    anyhow::bail!(
        "Downloading is disabled but JSONL file was not found @ {}",
        path.display()
    );

    // #[cfg(feature = "html")]
    // crate::download::download_jsonl(edition, lang, kind, path, opts.quiet)?;
    //
    // Ok(path.clone())
}

impl WiktextractDb {
    /// Open or create a new database at the given path
    // #[tracing::instrument(skip_all, level = "debug")]
    pub fn open_from_lang(edition: Edition, pm: &PathManager) -> Result<Self> {
        let db_path = format!("data/db/wiktextract_{edition}.db");
        if let Some(parent) = Path::new(&db_path).parent() {
            let _ = std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS wiktextract (
                id   INTEGER PRIMARY KEY,
                lang TEXT NOT NULL,
                word TEXT NOT NULL,
                pos  TEXT NOT NULL,
                entry BLOB NOT NULL
            );
            -- Regular index
            CREATE INDEX IF NOT EXISTS idx_wiktextract_lang
                ON wiktextract(lang);

            -- COVERING INDEX - includes entry blob so SQLite doesn't need to look up the row
            CREATE INDEX IF NOT EXISTS idx_wiktextract_lang_entry
                ON wiktextract(lang, entry);

            "#,
        )?;

        // Check if the DB is empty at all (no entries)
        let mut db = Self { conn };
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM wiktextract", [], |row| row.get(0))?;
        if count == 0 {
            let jsonl_path = find_or_download_jsonl_simple(edition, pm)?;
            db.import_jsonl(jsonl_path)?;
        } else {
            tracing::trace!("Opening non empty db for {edition}");
        }
        Ok(db)
    }

    #[tracing::instrument(skip_all, level = "debug")]
    pub fn import_jsonl<P: AsRef<Path>>(&mut self, jsonl_path: P) -> Result<()> {
        let start = Instant::now();
        let file = File::open(&jsonl_path)?;
        let reader = BufReader::new(file);

        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO wiktextract (lang, word, pos, entry) VALUES (?, ?, ?, ?)")?;

            for line in reader.lines() {
                let line = line?;
                let word_entry: WordEntry = serde_json::from_str(&line)?;
                let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&word_entry)?;

                stmt.execute(params![
                    word_entry.lang_code,
                    word_entry.word,
                    word_entry.pos,
                    bytes.as_ref()
                ])?;
            }
        }
        tx.commit()?;
        tracing::debug!(
            "Making db took {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );

        Ok(())
    }

    pub fn blob_to_word_entry(blob: &[u8]) -> Result<WordEntry> {
        let archived: &Archived<WordEntry> =
            rkyv::access::<Archived<WordEntry>, rkyv::rancor::Error>(blob).unwrap();
        let word_entry: WordEntry =
            rkyv::deserialize::<WordEntry, rkyv::rancor::Error>(archived).unwrap();
        Ok(word_entry)
    }
}

pub fn make_dict<D: Dictionary + IterLang + DatasetStrategy + EditionFrom>(
    dict: D,
    raw_args: D::A,
) -> Result<()> {
    let pm: &PathManager = &raw_args.try_into()?;
    let (_, source_pm, target_pm) = pm.langs();
    let opts = &pm.opts;
    pm.setup_dirs()?;

    // (source, target) -> D::I
    let mut irs_map: Map<LangsKey, D::I> = Map::default();

    for pair in iter_datasets(&dict, pm) {
        let (edition, _path_jsonl) = pair?;

        // Open database (auto-creates and imports if needed)
        let db = WiktextractDb::open_from_lang(edition.into(), pm)?;
        let source = match source_pm {
            LangSpec::All => panic!(),
            LangSpec::One(lang) => lang,
        };
        let target = match target_pm {
            LangSpec::All => panic!(),
            LangSpec::One(lang) => lang,
        };
        let langs = Langs {
            edition,
            source,
            target,
        };

        let other = match dict.edition_is() {
            EditionIs::Target => source,
            EditionIs::Source => target,
        };
        tracing::trace!("Opened db, selecting {other}...");
        let start = Instant::now();
        let mut stmt = db
            .conn
            .prepare("SELECT entry FROM wiktextract WHERE lang = ?")?;
        let mut rows = stmt.query([other.as_ref()])?;

        let mut line_count = 0;
        while let Some(row) = rows.next()? {
            line_count += 1;

            let blob: &[u8] = row.get_ref(0)?.as_blob()?;
            let mut entry = WiktextractDb::blob_to_word_entry(blob)?;

            // if line_count % 10_000 == 0 {
            //     print!("Processed {line_count} lines...\r");
            //     std::io::stdout().flush()?;
            // }

            // TODO: iter_langs doesn't make any sense...
            // we should make a dict for (edition, source, target) at a time...
            let key = dict.langs_to_key(langs);
            let irs = irs_map.entry(key).or_default();
            dict.preprocess(langs, &mut entry, opts, irs);
            dict.process(langs, &entry, irs);
        }
    }

    if irs_map.len() > 1 {
        tracing::debug!("Matrix ({}): {:?}", irs_map.len(), irs_map.keys());
    }

    for (key, mut irs) in irs_map {
        // if !opts.quiet {
        dict.found_ir_message(&key, &irs);
        // }
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
    anyhow::bail!(
        "Downloading is disabled but JSONL file was not found @ {}",
        path.display()
    );

    // #[cfg(feature = "html")]
    // crate::download::download_jsonl(edition, lang, kind, path, opts.quiet)?;
    //
    // Ok(path.clone())
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
        // tracing::debug!(
        //     "edition: {edition}, lang: {lang:?}, path: {}",
        //     path_jsonl.display()
        // );

        Ok((edition, path_jsonl))
    })
}

pub enum EditionIs {
    Target,
    Source,
}

// Replacement of IterLang/DatasetStrategy here
pub trait EditionFrom {
    fn edition_is(&self) -> EditionIs;
}

impl EditionFrom for DMain {
    fn edition_is(&self) -> EditionIs {
        EditionIs::Target
    }
}

impl EditionFrom for DIpa {
    fn edition_is(&self) -> EditionIs {
        EditionIs::Target
    }
}

impl EditionFrom for DGlossary {
    fn edition_is(&self) -> EditionIs {
        EditionIs::Source
    }
}

impl EditionFrom for DIpaMerged {
    fn edition_is(&self) -> EditionIs {
        todo!()
    }
}

impl EditionFrom for DGlossaryExtended {
    fn edition_is(&self) -> EditionIs {
        todo!()
    }
}
