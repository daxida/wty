# Makes rust types from language.json

# Note that there is also the isolang.rs crate

import json
from dataclasses import dataclass
from pathlib import Path


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
    f.write("/// This file was generated and should not be edited directly.\n")
    f.write("/// The source code can be found at scripts/build.py\n")


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

    # pub enum Lang { En, Fr, ... }
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

    w("impl Lang {\n")

    is_supported = " | ".join(lang.iso for lang in langs)
    w(f"{idt}pub const fn is_supported_iso_help_message() -> &'static str {{\n")
    w(f'{idt * 2}"Supported isos: {is_supported}"\n')
    w(f"{idt}}}\n\n")

    # has_edition
    w(f"{idt}pub const fn has_edition(&self) -> bool {{\n")
    with_edition_title = " | ".join(
        f"Self::{lang.iso.title()}" for lang in langs if lang.has_edition
    )
    w(f"{idt * 2}matches!(self, {with_edition_title})\n")
    w(f"{idt}}}\n\n")

    with_edition = " | ".join(lang.iso for lang in langs if lang.has_edition)
    w(f"{idt}pub const fn has_edition_help_message() -> &'static str {{\n")
    w(f'{idt * 2}"Valid editions: {with_edition}"\n')
    w(f"{idt}}}\n\n")

    # long: Lang::El => "Greek"
    w(f"{idt}pub const fn long(&self) -> &'static str {{\n")
    w(f"{idt * 2}match self {{\n")
    for lang in langs:
        w(f'{idt * 3}Self::{lang.iso.title()} => "{lang.language}",\n')
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # FromStr impl
    w("impl std::str::FromStr for Lang {\n")
    w(f"{idt}type Err = String;\n\n")
    w(f"{idt}fn from_str(s: &str) -> Result<Self, Self::Err> {{\n")
    w(f"{idt * 2}match s.to_lowercase().as_str() {{\n")
    for lang in langs:
        w(f'{idt * 3}"{lang.iso.lower()}" => Ok(Self::{lang.iso.title()}),\n')
    w(
        f"{idt * 3}_ => Err(format!(\"unsupported iso code '{{s}}'\\n{{}}\", Self::is_supported_iso_help_message())),\n"
    )
    w(f"{idt * 2}}}\n")
    w(f"{idt}}}\n")
    w("}\n\n")

    # Display impl
    w("impl std::fmt::Display for Lang {\n")
    w(f"{idt}fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n")
    w(f'{idt * 2}write!(f, "{{}}", format!("{{self:?}}").to_lowercase())\n')
    w(f"{idt}}}\n")
    w("}\n")


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

    with path_languages_json.open() as f:
        data = json.load(f)
    langs = [
        Lang(
            row["iso"],
            row["language"],
            row["displayName"],
            row["flag"],
            row.get("hasEdition", False),
        )
        for row in data
    ]

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
