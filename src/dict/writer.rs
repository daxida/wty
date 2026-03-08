//! Shared write behaviour.
//!
//! Structs that are dictionary dependent, like the intermediate representation or diagnostics, are
//! not included here and should be next to their dictionary for visibility.

use std::fs::{self, File};
use std::io::{Seek, Write};
use std::path::Path;

use anyhow::Result;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::cli::Options;
use crate::dict::core::LabelledYomitanEntry;
use crate::dict::index::get_index;
use crate::lang::Lang;
use crate::models::yomitan::YomitanEntry;
use crate::path::PathManager;
use crate::tags::get_tag_bank_as_tag_info;
use crate::utils::{CHECK_C, pretty_print_at_path, pretty_println_at_path};

const BANK_SIZE: usize = 25_000;

const STYLES_CSS: &[u8] = include_bytes!("../../assets/styles.css");
const STYLES_CSS_EXPERIMENTAL: &[u8] = include_bytes!("../../assets/styles_experimental.css");

/// Write yomitan labelled entries in banks to a sink (either disk or zip).
///
/// When zipping, also write metadata (index, css etc.).
pub fn write_yomitan(
    source: Lang,
    target: Lang,
    opts: &Options,
    pm: &PathManager,
    labelled_entries: Vec<LabelledYomitanEntry>,
) -> Result<()> {
    let mut bank_index = 0;

    if opts.save_temps {
        let out_dir = pm.dir_temp_dict();
        fs::create_dir_all(&out_dir)?;
        for lentry in labelled_entries {
            write_banks_to_disk(
                opts.pretty,
                opts.quiet,
                &lentry.entries,
                &mut bank_index,
                lentry.label,
                &out_dir,
            )?;
        }

        if !opts.quiet {
            pretty_println_at_path(&format!("{CHECK_C} Wrote temp data"), &out_dir);
        }
        return Ok(());
    }

    if opts.output_stdout {
        let stdout = std::io::stdout();
        let mut zip = ZipWriter::new_stream(stdout.lock());
        write_yomitan_zip(
            source,
            target,
            opts,
            pm,
            labelled_entries,
            &mut bank_index,
            Path::new("<stdout>"),
            &mut zip,
        )?;
        zip.finish()?;
        return Ok(());
    }

    let writer_path = pm.path_dict();
    let writer_file = File::create(&writer_path)?;
    let mut zip = ZipWriter::new(writer_file);
    write_yomitan_zip(
        source,
        target,
        opts,
        pm,
        labelled_entries,
        &mut bank_index,
        &writer_path,
        &mut zip,
    )?;
    zip.finish()?;

    pretty_println_at_path(&format!("{CHECK_C} Wrote yomitan dict"), &writer_path);

    Ok(())
}

fn write_yomitan_zip<W: Write + Seek>(
    source: Lang,
    target: Lang,
    opts: &Options,
    pm: &PathManager,
    labelled_entries: Vec<LabelledYomitanEntry>,
    bank_index: &mut usize,
    output_path: &Path,
    zip: &mut ZipWriter<W>,
) -> Result<()> {
    let zip_opts =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let index_string = get_index(&pm.dict_name_expanded(), source, target);
    zip.start_file("index.json", zip_opts)?;
    zip.write_all(index_string.as_bytes())?;

    zip.start_file("styles.css", zip_opts)?;
    if opts.experimental {
        zip.write_all(STYLES_CSS_EXPERIMENTAL)?;
    } else {
        zip.write_all(STYLES_CSS)?;
    }

    zip.start_file("tag_bank_1.json", zip_opts)?;
    serde_json::to_writer_pretty(&mut *zip, &get_tag_bank_as_tag_info())?;

    for lentry in labelled_entries {
        write_banks_to_zip(
            zip,
            zip_opts,
            opts.pretty,
            opts.quiet,
            &lentry.entries,
            bank_index,
            lentry.label,
            output_path,
        )?;
    }

    Ok(())
}

/// Writes `yomitan_entries` in batches to disk.
#[tracing::instrument(skip_all, level = "DEBUG")]
fn write_banks_to_disk(
    pretty: bool,
    quiet: bool,
    yomitan_entries: &[YomitanEntry],
    bank_index: &mut usize,
    label: &str,
    out_dir: &Path,
) -> Result<()> {
    let bank_name_prefix = match yomitan_entries.first() {
        Some(first) => first.file_prefix(),
        None => return Ok(()),
    };

    let total_bank_num = yomitan_entries.len().div_ceil(BANK_SIZE);

    for (bank_num, bank) in yomitan_entries.chunks(BANK_SIZE).enumerate() {
        *bank_index += 1;

        let bank_name = format!("{bank_name_prefix}_{bank_index}.json");
        let file_path = out_dir.join(&bank_name);
        let mut file = File::create(&file_path)?;
        if pretty {
            serde_json::to_writer_pretty(&mut file, &bank)?;
        } else {
            serde_json::to_writer(&mut file, &bank)?;
        }

        if !quiet {
            if bank_num > 0 {
                print!("\r\x1b[K");
            }
            pretty_print_at_path(
                &format!(
                    "Wrote yomitan {label} bank {}/{total_bank_num} ({} entries)",
                    bank_num + 1,
                    bank.len()
                ),
                &file_path,
            );
            std::io::stdout().flush()?;
        }
    }

    if !quiet {
        println!();
    }

    Ok(())
}

/// Writes `yomitan_entries` in batches to a zip writer.
#[tracing::instrument(skip_all, level = "DEBUG")]
fn write_banks_to_zip<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    zip_options: SimpleFileOptions,
    pretty: bool,
    quiet: bool,
    yomitan_entries: &[YomitanEntry],
    bank_index: &mut usize,
    label: &str,
    output_path: &Path,
) -> Result<()> {
    let bank_name_prefix = match yomitan_entries.first() {
        Some(first) => first.file_prefix(),
        None => return Ok(()),
    };

    let total_bank_num = yomitan_entries.len().div_ceil(BANK_SIZE);

    for (bank_num, bank) in yomitan_entries.chunks(BANK_SIZE).enumerate() {
        *bank_index += 1;

        let bank_name = format!("{bank_name_prefix}_{bank_index}.json");
        let file_path = output_path.join(&bank_name);

        zip.start_file(&bank_name, zip_options)?;
        if pretty {
            serde_json::to_writer_pretty(&mut *zip, &bank)?;
        } else {
            serde_json::to_writer(&mut *zip, &bank)?;
        }

        if !quiet {
            if bank_num > 0 {
                print!("\r\x1b[K");
            }
            pretty_print_at_path(
                &format!(
                    "Wrote yomitan {label} bank {}/{total_bank_num} ({} entries)",
                    bank_num + 1,
                    bank.len()
                ),
                &file_path,
            );
            std::io::stdout().flush()?;
        }
    }

    if !quiet {
        println!();
    }

    Ok(())
}
