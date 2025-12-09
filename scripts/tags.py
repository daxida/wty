"""Keep tag_bank_term and tag_order jsons in sync."""

from collections import Counter
import json
from pathlib import Path
from dataclasses import dataclass

ASSETS_PATH = Path("assets")

DOCUMENTED_YOMITAN_TAG_CATEGORIES = {
    "name",
    "expression",
    "popular",
    "frequent",
    "archaism",
    "partOfSpeech",
    # These we don't care
    "dictionary",
    "frequency",
    "search",
    "pronunciation-dictionary",
}


# duplicated from build
@dataclass
class WhitelistedTag:
    short_tag: str
    category: str
    sort_order: str
    # if array, first element will be used, others are aliases
    long_tag_aliases: str | list[str]
    popularity_score: int


def main() -> None:
    tag_bank_path = ASSETS_PATH / "tag_bank_term.json"
    tag_order_path = ASSETS_PATH / "tag_order.json"
    with tag_bank_path.open("r", encoding="utf-8") as f:
        tag_bank = json.load(f)
    with tag_order_path.open("r", encoding="utf-8") as f:
        tag_order = json.load(f)

    order_tags = []
    for category, tags in tag_order.items():
        for cat in tags:
            order_tags.append((category, cat))
    wtags = [WhitelistedTag(*row) for row in tag_bank]

    # Debug wtags categories
    # cf. https://github.com/yomidevs/yomitan/blob/master/docs/making-yomitan-dictionaries.md#tag-categories
    unique_wtags_categories = Counter()
    for wtag in wtags:
        if wtag.category:
            unique_wtags_categories[wtag.category] += 1
    for cat in unique_wtags_categories:
        assert cat in DOCUMENTED_YOMITAN_TAG_CATEGORIES
    print(unique_wtags_categories)

    # Quick diagnostic search
    for category, otag in order_tags:
        for wtag in wtags:
            if (
                otag == wtag.short_tag
                or (
                    isinstance(wtag.long_tag_aliases, list)
                    and otag in wtag.long_tag_aliases
                )
                or (
                    isinstance(wtag.long_tag_aliases, str)
                    and otag == wtag.long_tag_aliases
                )
            ):
                # print(f"       {otag} > {wtag.short_tag}")
                if wtag.category:
                    print(f"OC: {category}, BC: {wtag.category} ({otag})")
                break
        else:
            # Tag in tag_order.json but not in the bank
            # print(f"[miss] {otag}")
            pass


if __name__ == "__main__":
    main()
