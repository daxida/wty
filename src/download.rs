use anyhow::Result;

use crate::lang::Edition;

const KAIKKI_ROOT_URL_ENV_VAR: &str = "WTY_KAIKKI_ROOT_URL";

/// Return the url of the "raw" dataset.
fn url_jsonl_gz(edition: Edition) -> Result<String> {
    let root =
        std::env::var(KAIKKI_ROOT_URL_ENV_VAR).unwrap_or_else(|_| "https://kaikki.org".to_string());

    match edition {
        Edition::En => Ok(format!("{root}/dictionary/raw-wiktextract-data.jsonl.gz")),
        other => Ok(format!(
            "{root}/{other}wiktionary/raw-wiktextract-data.jsonl.gz"
        )),
    }
}

#[cfg(feature = "html")]
pub use html::*;

#[cfg(feature = "html")]
mod html {
    use super::url_jsonl_gz;

    use anyhow::Result;
    use flate2::read::GzDecoder;
    use std::fs::File;
    use std::io::BufRead;
    use std::io::{BufReader, BufWriter};
    use std::path::Path;

    use crate::{
        lang::Edition,
        utils::{CHECK_C, pretty_println_at_path},
    };

    // In the past, we supported downloading the post-processed, English-edition-only,
    // filtered datasets.
    // Those became deprecated cf. <https://github.com/tatuylonen/wiktextract/issues/1178>
    // but also caused some issues due to not being structured as their "raw" counterparts.
    //
    fn jsonl_reader(edition: Edition) -> Result<GzDecoder<Box<dyn std::io::Read>>> {
        let url = url_jsonl_gz(edition)?;
        let response = ureq::get(url).call()?;

        if let Some(last_modified) = response.headers().get("last-modified") {
            tracing::info!("Download was last modified: {:?}", last_modified);
        }

        let reader: Box<dyn std::io::Read> = Box::new(response.into_body().into_reader());
        Ok(GzDecoder::new(reader))
    }

    /// Download the "raw" jsonl (jsonlines) from kaikki and write it to `path_jsonl`.
    ///
    /// "Raw" means that it does not include extra information, not intended for general use,
    /// that they (kaikki) use for their website generation.
    ///
    /// Does not write the .gz file to disk.
    ///
    /// WARN: expects `path_jsonl` to be a valid path (with existing parents etc.)
    pub fn download_jsonl_to_path(edition: Edition, path_jsonl: &Path, quiet: bool) -> Result<()> {
        let url = url_jsonl_gz(edition)?;
        if !quiet {
            println!("⬇ Downloading {url}");
        }

        // We can't use gzip's ureq feature because there is no content-encoding in headers
        // https://github.com/tatuylonen/wiktextract/issues/1482
        let mut decoder = jsonl_reader(edition)?;

        let mut writer = BufWriter::new(File::create(path_jsonl)?);
        std::io::copy(&mut decoder, &mut writer)?;

        if !quiet {
            pretty_println_at_path(&format!("{CHECK_C} Downloaded"), path_jsonl);
        }

        Ok(())
    }

    pub fn stream_jsonl_reader(
        edition: Edition,
        quiet: bool,
        capacity: usize,
    ) -> Result<Box<dyn BufRead>> {
        let url = url_jsonl_gz(edition)?;
        if !quiet {
            println!("⬇ Streaming {url}");
        }

        Ok(Box::new(BufReader::with_capacity(
            capacity,
            jsonl_reader(edition)?,
        )))
    }
}
