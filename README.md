Converts wiktionary data from [kaikki](https://kaikki.org/) ([wiktextract](https://github.com/tatuylonen/wiktextract)) to [yomitan](https://github.com/yomidevs/yomitan)-compatible dictionaries.

This is a port of [kaikki-to-yomitan](https://github.com/yomidevs/kaikki-to-yomitan).

Converted dictionaries can be found on the [downloads](https://daxida.github.io/kty/) page.

## How to run

This example use German (de) to English (en).

```
$ cargo install --git https://github.com/daxida/kty
$ kty main de en
...
✓ Wrote yomitan dict @ data/dict/de/en/kty-de-en.zip (20.94 MB)
```

A list of supported languages isos can be found at `assets/language.json`. It contains every (and possibly more!) language [supported by yomitan](https://raw.githubusercontent.com/yomidevs/yomitan/master/ext/js/language/language-descriptors.js).

## Other options

Output of `kty main --help` (may be outdated):

```
Main dictionary. Uses target for the edition

Usage: kty main [OPTIONS] <SOURCE> <TARGET> [DICT_NAME]

Arguments:
  <SOURCE>     Source language
  <TARGET>     Target language
  [DICT_NAME]  Dictionary name [default: kty]

Options:
  -s, --save-temps           Write temporary files to disk and skip zipping
  -r, --redownload           Redownload kaikki files
      --first <FIRST>        Only keep the first n jsonlines before filtering. -1 keeps all [default: -1]
      --filter <FILTER>      Only keep entries matching certain key–value filters
      --reject <REJECT>      Only keep entries not matching certain key–value filters
  -p, --pretty               Write jsons with whitespace
  -v, --verbose              Verbose output
      --root-dir <ROOT_DIR>  Change the root directory [default: data]
  -h, --help                 Print help

Skip:
      --skip-filtering  Skip filtering the jsonl
      --skip-tidy       Skip running tidy (IR generation)
      --skip-yomitan    Skip running yomitan (mainly for testing)
```

## Tests

Tests are run with `cargo test`. If you only want to run tests for the main dictionary in a single language pair, without capturing output:

```
cargo run -- main ja en --root-dir=tests --save-temps --pretty
```

To add a word to the testsuite, besides copy pasting it, you can run:

```
# If the target is English
cargo run --release -- main de en --skip-tidy --skip-yomitan --filter word,faul
cat data/kaikki/de-en-extract.tmp.jsonl >> tests/kaikki/de-en-extract.jsonl

# Otherwise
cargo run --release -- main de de --skip-tidy --skip-yomitan --filter word,faul
cat data/kaikki/de-de-extract.jsonl >> tests/kaikki/de-de-extract.jsonl
```

## Similar converting projects

- For ebooks there is [ebook_dictionary_creator](https://github.com/Vuizur/ebook_dictionary_creator) that uses [pyglossary](https://github.com/ilius/pyglossary)

