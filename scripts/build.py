# Makes rust types from language.json

# Note that there is also the isolang.rs crate

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass
class Lang:
    iso: str
    language: str
    display_name: str
    flag: str
    # https://github.com/tatuylonen/wiktextract/tree/master/src/wiktextract/extractor
    has_edition: bool


@dataclass
class WhitelistedTag:
    short_tag: str
    category: str
    sort_order: str
    # if array, first element will be used, others are aliases
    long_tag_aliases: str | list[str]
    popularity_score: int


PosAliases = list[str]


def write_warning(f) -> None:
    f.write("//! This file was generated and should not be edited directly.\n")
    f.write("//! The source code can be found at scripts/build.py\n\n")


def generate_tags_rs(
    tags: list[str],
    whitelisted_tags: list[WhitelistedTag],
    poses: list[PosAliases],
    f,
) -> None:
    idt = " " * 4
    w = f.write  # shorthand

    write_warning(f)

    w(f"pub const TAG_ORDER: [&str; {len(tags)}] = [\n")
    for tag in tags:
        w(f'{idt}"{tag}",\n')
    w("];\n\n")

    w(
        f"pub const TAG_BANK: [(&str, &str, i32, &[&str], i32); {len(whitelisted_tags)}] = [\n"
    )
    for wt in whitelisted_tags:
        longs_as_list = (
            wt.long_tag_aliases
            if isinstance(wt.long_tag_aliases, list)
            else [wt.long_tag_aliases]
        )
        longs_str = str(longs_as_list).replace("'", '"')
        w(
            f'{idt}("{wt.short_tag}", "{wt.category}", {wt.sort_order}, &{longs_str}, {wt.popularity_score}),\n'
        )
    w("];\n\n")

    w(f"pub const POSES: [&[&str]; {len(poses)}] = [\n")
    for pos_aliases in poses:
        aliases_str = str(pos_aliases).replace("'", '"')
        w(f"{idt}&{aliases_str},\n")
    w("];\n\n")


def generate_lang_rs(langs: list[Lang], f) -> None:
    idt = " " * 4
    w = f.write  # shorthand

    write_warning(f)

    # w("#![rustfmt::skip]\n")

    w("use serde::{Deserialize, Serialize};\n\n")

    ### Lang start

    # Lang
    w("#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]\n")
    w("pub enum Lang {\n")
    # Add English on top as the default variant
    w(f"{idt}/// English\n")  # doc
    w(f"{idt}#[default]\n")
    w(f"{idt}En,\n")
    for lang in langs:
        if lang.iso != "en":
            w(f"{idt}/// {lang.language}\n")  # doc
            w(f"{idt}{lang.iso.title()},\n")
    w("}\n\n")

    # Lang: From<EditionLang>
    w("impl From<EditionLang> for Lang {\n")
    w(f"{idt}fn from(e: EditionLang) -> Self {{\n")
    w(f"{idt * 2}match e {{\n")
    for lang in langs:
        if lang.has_edition:
            w(
                f"{idt * 3}EditionLang::{lang.iso.title()} => Self::{lang.iso.title()},\n"
            )
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    w("impl Lang {\n")

    # Lang: help_messages
    is_supported = " | ".join(lang.iso for lang in langs)
    fn_name = "help_supported_isos"
    w(f"{idt}pub const fn {fn_name}() -> &'static str {{\n")
    w(f'{idt * 2}"Supported isos: {is_supported}"\n')
    w(f"{idt}}}\n\n")

    coloured_parts = [
        f"\x1b[32m{lang.iso}\x1b[0m" if lang.has_edition else lang.iso for lang in langs
    ]
    isos_colored = " | ".join(coloured_parts)
    w(f"{idt}pub const fn help_supported_isos_coloured() -> &'static str {{\n")
    w(f'{idt * 2}"Supported isos: {isos_colored}"\n')
    w(f"{idt}}}\n\n")

    with_edition = " | ".join(lang.iso for lang in langs if lang.has_edition)
    w(f"{idt}pub const fn help_supported_editions() -> &'static str {{\n")
    w(f'{idt * 2}"Supported editions: {with_edition}"\n')
    w(f"{idt}}}\n\n")

    # Lang: long. long: Lang::El => "Greek"
    w(f"{idt}pub const fn long(&self) -> &'static str {{\n")
    w(f"{idt * 2}match self {{\n")
    for lang in langs:
        w(f'{idt * 3}Self::{lang.iso.title()} => "{lang.language}",\n')
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # Lang: FromStr
    w("impl std::str::FromStr for Lang {\n")
    w(f"{idt}type Err = String;\n\n")
    w(f"{idt}fn from_str(s: &str) -> Result<Self, Self::Err> {{\n")
    w(f"{idt * 2}match s.to_lowercase().as_str() {{\n")
    for lang in langs:
        w(f'{idt * 3}"{lang.iso.lower()}" => Ok(Self::{lang.iso.title()}),\n')
    w(
        f"{idt * 3}_ => Err(format!(\"unsupported iso code '{{s}}'\\n{{}}\", Self::{fn_name}())),\n"
    )
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # Lang: Display
    w("impl std::fmt::Display for Lang {\n")
    w(f"{idt}fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n")
    w(f'{idt * 2}let debug_str = format!("{{self:?}}");\n')
    w(f'{idt * 2}write!(f, "{{}}", debug_str.to_lowercase())\n')
    w(f"{idt}}}\n")
    w("}\n\n")

    ### Edition start

    # Edition
    w("#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]\n")
    w("pub enum Edition {\n")
    w(f"{idt}/// All editions\n")
    w(f"{idt}All,\n")
    w(f"{idt}/// An `EditionLang`\n")
    w(f"{idt}EditionLang(EditionLang),\n")
    w("}\n\n")

    # Edition: variants (iteration)
    w("impl Edition {\n")
    w(f"{idt}pub fn variants(&self) -> Vec<EditionLang> {{\n")
    w(f"{idt * 2}match self {{\n")
    w(f"{idt * 3}Self::All => vec![\n")
    for lang in langs:
        if lang.has_edition:
            w(f"{idt * 4}EditionLang::{lang.iso.title()},\n")
    w(f"{idt * 3}],\n")
    w(f"{idt * 3}Self::EditionLang(lang) => vec![*lang],\n")
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # Edition: Default
    w("impl Default for Edition {\n")
    w(f"{idt}fn default() -> Self {{\n")
    w(f"{idt * 2}Self::EditionLang(EditionLang::default())\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # Edition: FromStr
    w("impl std::str::FromStr for Edition {\n")
    w(f"{idt}type Err = String;\n\n")
    w(f"{idt}fn from_str(s: &str) -> Result<Self, Self::Err> {{\n")
    w(f"{idt * 2}match s {{\n")
    w(f'{idt * 3}"all" => Ok(Self::All),\n')
    w(f"{idt * 3}other => Ok(Self::EditionLang(other.parse::<EditionLang>()?)),\n")
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # Edition: Display
    w("impl std::fmt::Display for Edition {\n")
    w(f"{idt}fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n")
    w(f"{idt * 2}match self {{\n")
    w(f'{idt * 3}Self::All => write!(f, "all"),\n')
    w(f'{idt * 3}Self::EditionLang(lang) => write!(f, "{{lang}}"),\n')
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    ### EditionLang start

    # EditionLang
    w("#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]\n")
    w("pub enum EditionLang {\n")
    # Add English on top as the default variant
    w(f"{idt}/// English\n")  # doc
    w(f"{idt}#[default]\n")
    w(f"{idt}En,\n")
    for lang in langs:
        if lang.iso != "en" and lang.has_edition:
            w(f"{idt}/// {lang.language}\n")  # doc
            w(f"{idt}{lang.iso.title()},\n")
    w("}\n\n")

    # EditionLang: TryFrom<Lang>
    w("impl std::convert::TryFrom<Lang> for EditionLang {\n")
    w(f"{idt}type Error = &'static str;\n\n")
    w(f"{idt}fn try_from(lang: Lang) -> Result<Self, Self::Error> {{\n")
    w(f"{idt * 2}match lang {{\n")
    for lang in langs:
        if lang.has_edition:
            w(f"{idt * 3}Lang::{lang.iso.title()} => Ok(Self::{lang.iso.title()}),\n")
    w(f'{idt * 3}_ => Err("language has no edition"),\n')
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # EditionLang: TryFrom<Edition>
    w("impl std::convert::TryFrom<Edition> for EditionLang {\n")
    w(f"{idt}type Error = &'static str;\n\n")
    w(f"{idt}fn try_from(edition: Edition) -> Result<Self, Self::Error> {{\n")
    w(f"{idt * 2}match edition {{\n")
    w(f"{idt * 3}Edition::EditionLang(lang) => Ok(lang),\n")
    w(f'{idt * 3}Edition::All => Err("cannot convert Edition::All to EditionLang"),\n')
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # EditionLang: FromStr
    w("impl std::str::FromStr for EditionLang {\n")
    w(f"{idt}type Err = String;\n\n")
    w(f"{idt}fn from_str(s: &str) -> Result<Self, Self::Err> {{\n")
    w(f"{idt * 2}match s.to_lowercase().as_str() {{\n")
    for lang in langs:
        if lang.has_edition:
            w(f'{idt * 3}"{lang.iso.lower()}" => Ok(Self::{lang.iso.title()}),\n')
    w(f"{idt * 3}_ => Err(format!(\"invalid edition '{{s}}'\")),\n")
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # EditionLang: Display
    w("impl std::fmt::Display for EditionLang {\n")
    w(f"{idt}fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n")
    w(f'{idt * 2}let debug_str = format!("{{:?}}", Lang::from(*self));\n')
    w(f'{idt * 2}write!(f, "{{}}", debug_str.to_lowercase())\n')
    w(f"{idt}}}\n")
    w("}\n")


def load_lang(item: Any) -> Lang:
    return Lang(
        item["iso"],
        item["language"],
        item["displayName"],
        item["flag"],
        item.get("hasEdition", False),
    )


def sort_languages_json(path: Path) -> None:
    with path.open() as f:
        text = f.read()

    lines = text.splitlines()
    langs = [
        (load_lang(json.loads(line.strip(","))), idx)
        for idx, line in enumerate(lines[1:-1])
    ]

    langs_sorted = sorted(langs, key=lambda pair: pair[0].display_name)
    if langs == langs_sorted:
        return

    with path.open("w") as f:
        f.write("[\n")
        for _, idx in langs_sorted:
            f.write(lines[idx + 1])
            f.write("\n")
        f.write("]\n")


def main() -> None:
    src = Path("src")
    path_lang_rs = src / "lang.rs"
    path_tags_rs = src / "tags" / "tags_constants.rs"
    jsons_root = Path("assets")
    path_languages_json = jsons_root / "languages.json"
    path_tag_order_json = jsons_root / "tag_order.json"
    path_tag_bank_json = jsons_root / "tag_bank_term.json"
    path_pos_json = jsons_root / "parts_of_speech.json"

    for path in (
        path_languages_json,
        path_tag_order_json,
        path_tag_bank_json,
        path_pos_json,
    ):
        if not path.exists:
            print(f"Path does not exist @ {path}")
            return

    sort_languages_json(path_languages_json)

    with path_languages_json.open() as f:
        data = json.load(f)
        langs = [load_lang(row) for row in data]

    tag_order: list[str] = []
    with path_tag_order_json.open() as f:
        data = json.load(f)
        for _, tags in data.items():
            tag_order.extend(tags)
    with path_tag_bank_json.open() as f:
        data = json.load(f)
    whitelisted_tags = [WhitelistedTag(*row) for row in data]
    with path_pos_json.open() as f:
        data = json.load(f)
    poses: list[PosAliases] = data

    # import sys
    # generate_lang_rs(langs, sys.stdout)
    # generate_tags_rs(tag_order, sys.stdout)

    with path_lang_rs.open("w") as f:
        generate_lang_rs(langs, f)
        print(f"Wrote rust code @ {path_lang_rs}")
    with path_tags_rs.open("w") as f:
        generate_tags_rs(tag_order, whitelisted_tags, poses, f)
        print(f"Wrote rust code @ {path_tags_rs}")


if __name__ == "__main__":
    main()
