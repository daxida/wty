"""Generate a single static downloads page with dropdowns."""

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

REPO_ID = "daxida/test-dataset"
REPO_URL = f"https://huggingface.co/datasets/{REPO_ID}"
BASE_URL = f"{REPO_URL}/resolve/main/dict"
"""https://huggingface.co/datasets/daxida/test-dataset/resolve/main/dict"""


# duplicated from build
@dataclass
class Lang:
    iso: str
    language: str
    display_name: str
    flag: str
    # https://github.com/tatuylonen/wiktextract/tree/master/src/wiktextract/extractor
    has_edition: bool


# duplicated from build
def load_lang(item: Any) -> Lang:
    return Lang(
        item["iso"],
        item["language"],
        item["displayName"],
        item["flag"],
        item.get("hasEdition", False),
    )


# duplicated from build
def load_langs(path: Path) -> list[Lang]:
    with path.open() as f:
        data = json.load(f)
    return [load_lang(item) for item in data]


def render_line(
    label: str, dtype: str, target_options: str, source_options: str | None = None
) -> str:
    line_class = "download-line"
    if source_options is None:
        line_class += " no-source"

    source_html = ""
    if source_options:
        source_html = f"""
      <select class="dl-source">
        <option value="" selected disabled>Select a source...</option>
          {source_options}
      </select>
        """

    return f"""
<tr data-type="{dtype}" class="{line_class}">
  <th>{label}</th>
  <td>
    <select class="dl-target">
      <option value="" selected disabled>Select a target...</option>
        {target_options}
    </select>
  </td>
  <td>{source_html}</td>
  <td>
    <button class="dl-btn">📥</button>
  </td>
  <td class="dl-info"></td>
</tr>
""".strip()


def generate_downloads_page(all_langs: list[Lang], editions: list[Lang]) -> str:
    indent4 = "  " * 4
    indent5 = "  " * 5

    target_options = "\n".join(
        f'{indent4}<option value="{lang.iso}">{lang.flag} {lang.display_name}</option>'
        for lang in all_langs
    ).strip()

    edition_options = "\n".join(
        f'{indent5}<option value="{lang.iso}">{lang.flag} {lang.display_name}</option>'
        for lang in editions
    ).strip()

    table_html = "\n".join(
        [
            render_line("📘 Main", "main", target_options, edition_options),
            render_line("🔤 IPA", "ipa", target_options, edition_options),
            render_line("🧬 IPA merged", "ipa-merged", target_options),  # no source
            render_line("🌍 Glossary", "glossary", target_options, target_options),
        ]
    )

    return f"""# Download

<table class="download-table">
  <tbody>
{table_html}
  </tbody>
</table>

Files are hosted [here]({
        REPO_URL
    }), where you can also see the calendar version (calver) of the dictionaries.

A brief description of the dictionaries can be found [here](dictionaries.md).
""".strip()


def generate_language_page(all_langs, editions) -> str:
    return f"""
[Kaikki](https://kaikki.org/) currently supports **{len(editions)} Wiktionary editions**. Most dictionaries use at least one edition.

For a list of **targets** supported by the English edition, see [here](https://kaikki.org/dictionary/).

For a list of supported languages by Yomitan, see [here](https://yomitan.wiki/supported-languages/). If it is outdated, refer to [here](https://raw.githubusercontent.com/yomidevs/yomitan/master/ext/js/language/language-descriptors.js).

!!! tip "Missing a language? Please **open an [issue](https://github.com/daxida/kty/issues/new)**."

---

**With Wiktionary Editions ({len(editions)}):**  
{", ".join(f"{edition.flag} {edition.display_name} `{edition.iso}`" for edition in editions)}

**All Supported ({len(all_langs)}):**  
{", ".join(f"{lang.flag} {lang.display_name} `{lang.iso}`" for lang in all_langs)}
""".strip()


def main() -> None:
    path_language_json = Path("assets/languages.json")
    path_docs = Path("docs")
    path_download = path_docs / "download.md"
    path_language = path_docs / "language.md"

    print("Loading languages...")
    all_langs = load_langs(path_language_json)
    editions = [lang for lang in all_langs if lang.has_edition]

    print(f"Found {len(all_langs)} languages, {len(editions)} with edition")

    print(f"Generating downloads page @ {path_download}")
    path_download.write_text(generate_downloads_page(all_langs, editions))

    print(f"Generating language page @ {path_language}")
    path_language.write_text(generate_language_page(all_langs, editions))

    print("✓ Done!")


if __name__ == "__main__":
    main()
