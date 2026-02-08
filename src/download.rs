use anyhow::{Result, bail};

use crate::lang::{Edition, Lang};

#[derive(Debug)]
pub enum DatasetKind {
    /// Post-processed, English-edition-only, filtered by language.
    /// Should not use: <https://github.com/tatuylonen/wiktextract/issues/1178>
    Filtered,
    /// Every edition (including English) raw datasets
    Unfiltered,
}

/// Different in English and non-English editions.
///
/// Default download name is:
/// - Filtered:   `kaikki.org-dictionary-TARGET.jsonl.gz`
/// - Unfiltered: `raw-wiktextract-data.jsonl.gz`
///
/// Example (el):    `https://kaikki.org/elwiktionary/raw-wiktextract-data.jsonl.gz`
/// Example (sh-en): `https://kaikki.org/dictionary/Serbo-Croatian/kaikki.org-dictionary-SerboCroatian.jsonl.gz`
pub fn url_jsonl_gz(edition: Edition, lang: Option<Lang>, kind: DatasetKind) -> Result<String> {
    let root = "https://kaikki.org";

    match (edition, kind) {
        (Edition::En, DatasetKind::Filtered) => {
            let Some(lang) = lang else {
                bail!("filtered dataset requires lang to not be None");
            };
            let long = lang.long();
            // Serbo-Croatian, Ancient Greek and such cases
            let long_compact: String = long.chars().filter(|c| *c != ' ' && *c != '-').collect();
            let long_escaped = long.replace(' ', "%20");
            Ok(format!(
                "{root}/dictionary/{long_escaped}/kaikki.org-dictionary-{long_compact}.jsonl.gz"
            ))
        }
        (_, DatasetKind::Filtered) => {
            bail!("Kaikki does not support filtered kind for non-English editions")
        }
        (Edition::En, DatasetKind::Unfiltered) => {
            Ok(format!("{root}/dictionary/raw-wiktextract-data.jsonl.gz"))
        }
        (other, DatasetKind::Unfiltered) => Ok(format!(
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
    use std::io::BufWriter;
    use std::path::Path;

    use crate::{
        download::DatasetKind,
        lang::{Edition, Lang},
        utils::{CHECK_C, pretty_println_at_path},
    };

    /// Download the "raw" jsonl (jsonlines) from kaikki and write it to `path_jsonl`.
    ///
    /// "Raw" means that it does not include extra information, not intended for general use,
    /// that they (kaikki) use for their website generation.
    ///
    /// Does not write the .gz file to disk.
    pub fn download_jsonl(
        edition: Edition,
        lang: Option<Lang>,
        kind: DatasetKind,
        path_jsonl: &Path,
        quiet: bool,
    ) -> Result<()> {
        let url = url_jsonl_gz(edition, lang, kind)?;
        if !quiet {
            println!("⬇ Downloading {url}");
        }

        let response = ureq::get(url).call()?;

        if let Some(last_modified) = response.headers().get("last-modified") {
            tracing::info!("Download was last modified: {:?}", last_modified);
        }

        let reader = response.into_body().into_reader();
        // We can't use gzip's ureq feature because there is no content-encoding in headers
        // https://github.com/tatuylonen/wiktextract/issues/1482
        let mut decoder = GzDecoder::new(reader);

        let mut writer = BufWriter::new(File::create(path_jsonl)?);
        std::io::copy(&mut decoder, &mut writer)?;

        if !quiet {
            pretty_println_at_path(&format!("{CHECK_C} Downloaded"), path_jsonl);
        }

        Ok(())
    }
}
