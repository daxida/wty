use crate::lang::{EditionLang, Lang};

/// Different in English and non-English editions.
///
/// Example (el):    `https://kaikki.org/elwiktionary/raw-wiktextract-data.jsonl.gz`
/// Example (sh-en): `https://kaikki.org/dictionary/Serbo-Croatian/kaikki.org-dictionary-SerboCroatian.jsonl.gz`
pub fn url_jsonl_raw_gz(edition: EditionLang, source: Lang) -> String {
    let root = "https://kaikki.org";

    match edition {
        // Depends on source
        // Default download name is: kaikki.org-dictionary-TARGET_LANGUAGE.jsonl.gz
        EditionLang::En => {
            let long = source.long();
            // Serbo-Croatian, Ancient Greek and such cases
            let long_no_special_chars: String =
                long.chars().filter(|c| *c != ' ' && *c != '-').collect();
            let long_escaped = long.replace(' ', "%20");
            format!(
                "{root}/dictionary/{long_escaped}/kaikki.org-dictionary-{long_no_special_chars}.jsonl.gz"
            )
        }
        // Does not depend on source
        // Default download name is: raw-wiktextract-data.jsonl.gz
        other => format!("{root}/{other}wiktionary/raw-wiktextract-data.jsonl.gz",),
    }
}

#[cfg(feature = "html")]
pub mod html {
    use super::{EditionLang, Lang, url_jsonl_raw_gz};

    use anyhow::Result;
    use flate2::read::GzDecoder;
    use std::fs::File;
    use std::io::BufWriter;
    use std::path::Path;

    use crate::utils::{CHECK_C, pretty_println_at_path};

    // TODO: This is not skipping properly!

    /// Download the raw jsonl from kaikki and write it to `path_jsonl_raw`.
    ///
    /// Does not write the .gz file to disk.
    pub fn download_jsonl(
        edition: EditionLang,
        source: Lang,
        path_jsonl_raw: &Path,
        quiet: bool,
    ) -> Result<()> {
        let url = url_jsonl_raw_gz(edition, source);
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

        let mut writer = BufWriter::new(File::create(path_jsonl_raw)?);
        std::io::copy(&mut decoder, &mut writer)?;

        if !quiet {
            pretty_println_at_path(&format!("{CHECK_C} Downloaded"), path_jsonl_raw);
        }

        Ok(())
    }
}
