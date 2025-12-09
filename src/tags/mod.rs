pub mod tags_constants;

use std::cmp::Ordering;

use indexmap::IndexMap;
use tags_constants::{POSES, TAG_BANK, TAG_ORDER};

use crate::models::kaikki::Tag;
use crate::models::yomitan::TagInformation;

// TODO: a bunch of sorting and handling of tags should go here

/// Tags that are blacklisted if they happen at *some* expanded form @ tidy
pub const BLACKLISTED_FORM_TAGS: [&str; 14] = [
    "inflection-template",
    "table-tags",
    "canonical",
    "class",
    "error-unknown-tag",
    "error-unrecognized-form",
    "includes-article",
    "obsolete",
    "archaic",
    "used-in-the-form",
    "romanization",
    "dated",
    "auxiliary",
    // multiword-construction was in REDUNDANT_TAGS in the original.
    // Yet it only seems to give noise for the fr-en edition (@ prendre):
    // * Form: 'present indicative of avoir + past participle' ???
    // * Tags: ["indicative", "multiword-construction", "perfect", "present"]
    //
    // It also removes valid german forms that are nonetheless most useless:
    // * werde gepflogen haben (for pflegen)
    // (note that gepflogen is already added)
    // This was considered ok. To revisit if it is more intrusive in other languages.
    "multiword-construction",
];
/// Tags that are blacklisted if they happen at *every* expanded form @ tidy
pub const IDENTITY_FORM_TAGS: [&str; 3] = ["nominative", "singular", "infinitive"];
/// Tags that we just remove from forms @ tidy
pub const REDUNDANT_FORM_TAGS: [&str; 1] = ["combined-form"];

/// Sort tags by their position in the tag bank.
///
/// Expects (but does not check) tags WITHOUT spaces.
pub fn sort_tags<T: AsRef<str>>(tags: &mut [T]) {
    // debug_assert!(tags.iter().all(|tag| !tag.contains(' ')));

    tags.sort_by(|a, b| {
        let index_a = TAG_ORDER.iter().position(|&x| x == a.as_ref());
        let index_b = TAG_ORDER.iter().position(|&x| x == b.as_ref());

        match (index_a, index_b) {
            (Some(i), Some(j)) => i.cmp(&j),   // both found → compare positions
            (Some(_), None) => Ordering::Less, // found beats not-found
            (None, Some(_)) => Ordering::Greater,
            // This seems better but it's different from the original
            // (None, None) => a.cmp(b),        // neither found → alphabetical fallback
            (None, None) => Ordering::Equal, // neither found → do nothing
        }
    });
}

/// Sort tags by word-by-word lexicographical similarity, grouping tags that
/// share the same leading words (shorter prefix-only tags sort first).
///
/// Expects (but does not check) tags WITH spaces.
pub fn sort_tags_by_similar(tags: &mut [Tag]) {
    tags.sort_by(|a, b| {
        let mut a_iter = a.split(' ');
        let mut b_iter = b.split(' ');

        loop {
            match (a_iter.next(), b_iter.next()) {
                (Some(a_word), Some(b_word)) => match a_word.cmp(b_word) {
                    Ordering::Equal => continue,
                    non_eq => return non_eq,
                },
                (Some(_), None) => return Ordering::Greater,
                (None, Some(_)) => return Ordering::Less,
                (None, None) => return Ordering::Equal,
            }
        }
    });
}

/// First, remove duplicates.
///
/// Then, remove tag1 if there is a tag2 such that tag1 <= tag2
///
/// Expects (but does not check) tags WITH spaces.
pub fn remove_redundant_tags(tags: &mut Vec<Tag>) {
    tags.sort();
    // We can't just call dedup, because the inner words may not be sorted
    // cf. tags = ["a b", "b a"]
    tags.dedup_by(|a, b| {
        let mut a_words: Vec<_> = a.split(' ').collect();
        let mut b_words: Vec<_> = b.split(' ').collect();
        a_words.sort_unstable();
        b_words.sort_unstable();
        a_words == b_words
    });

    let mut keep = vec![true; tags.len()];

    for i in 0..tags.len() {
        for j in 0..tags.len() {
            // tag_i <= tag_j
            if i != j && tags_are_subset(&tags[i], &tags[j]) {
                keep[i] = false;
                break;
            }
        }
    }

    let mut idx = 0;
    tags.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
}

/// Check if all words in string `a` are present in string `b`.
///
/// F.e. "foo bar" is subset of "bar foo baz"
fn tags_are_subset(a: &str, b: &str) -> bool {
    a.split(' ')
        .all(|a_word| b.split(' ').any(|b_word| b_word == a_word))
}

const PERSON_TAGS: [&str; 3] = ["first-person", "second-person", "third-person"];

fn person_sort(tags: &mut [&str]) {
    tags.sort_by_key(|x| PERSON_TAGS.iter().position(|p| p == x).unwrap_or(999));
}

/// Merge similar tags if the only difference is the person-tags.
///
/// F.e.
/// in:  ['first-person singular', 'third-person singular']
/// out: ['singular first/third-person ']
///
/// Note that this does not preserve logical tag order, and should be called before sort_tag.
pub fn merge_person_tags(tags: &mut Vec<Tag>) {
    let contains_person = tags
        .iter()
        .any(|tag| PERSON_TAGS.iter().any(|p| tag.contains(p)));

    if !contains_person {
        return;
    }

    let unmerged_tags = std::mem::take(tags);
    let mut grouped: IndexMap<Vec<&str>, Vec<&str>> = IndexMap::new();

    for tag in &unmerged_tags {
        let (person_tags, other_tags): (Vec<_>, Vec<_>) =
            tag.split(' ').partition(|t| PERSON_TAGS.contains(t));

        match person_tags.as_slice() {
            [person] => grouped.entry(other_tags).or_default().push(person),
            _ => tags.push(tag.to_string()),
        }
    }

    for (other_tags, mut person_matches) in grouped {
        let mut tags_cur: Vec<_> = other_tags.iter().map(|s| s.to_string()).collect();

        person_sort(&mut person_matches);

        // [first-person, third-person] > first/third-person
        let merged_tag = format!(
            "{}-person",
            person_matches
                .iter()
                // SAFETY: PERSON_TAGS contains pmatch so it always ends in -person
                .map(|pmatch| pmatch.strip_suffix("-person").unwrap())
                .collect::<Vec<_>>() // unlucky collect because we can't join a map
                .join("/")
        );

        tags_cur.push(merged_tag);
        // sort_tags(&mut tags_cur);
        tags.push(tags_cur.join(" "));
    }
}

/// Return a Vec<TagInformation> from `tag_bank_terms` that fits the yomitan tag schema.
pub fn get_tag_bank_as_tag_info() -> Vec<TagInformation> {
    TAG_BANK.iter().map(TagInformation::new).collect()
}

/// Look for the tag in `TAG_BANK` (`tag_bank_terms.json`) and return the `TagInformation` if any.
///
/// Note that `long_tag` is returned normalized.
pub fn find_tag_in_bank(tag: &str) -> Option<TagInformation> {
    TAG_BANK.iter().find_map(|entry| {
        if entry.3.contains(&tag) {
            Some(TagInformation::new(entry))
        } else {
            None
        }
    })
}

/// Look for the short form in POSES (`tag_bank_terms.json` with category "partOfSpeech") and
/// return the short form if any.
pub fn find_short_pos(pos: &str) -> Option<&'static str> {
    POSES
        .into_iter()
        .find_map(|(long, short)| if long == pos { Some(short) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_string_vec(str_vec: &[&str]) -> Vec<String> {
        str_vec.iter().map(|s| (*s).to_string()).collect()
    }

    fn to_str_vec<'a>(str_vec: &[&'a str]) -> Vec<&'a str> {
        str_vec.iter().copied().collect()
    }

    // This imitates the original. Can be removed if sort_tags logic changes.
    #[test]
    fn sort_tags_base() {
        let tag_not_found = "__sentinel";
        assert!(!TAG_ORDER.contains(&tag_not_found));
        let mut received = to_str_vec(&[tag_not_found, "Gheg"]);
        let expected = to_string_vec(&[tag_not_found, "Gheg"]);
        sort_tags(&mut received);
        assert_eq!(received, expected);
    }

    fn make_test_sort_tags_by_similar(received: &[&str], expected: &[&str]) {
        let mut vreceived: Vec<String> = to_string_vec(received);
        let vexpected: Vec<String> = to_string_vec(expected);
        sort_tags_by_similar(&mut vreceived);
        assert_eq!(vreceived, vexpected);
    }

    #[test]
    fn sort_tags_by_similar1() {
        make_test_sort_tags_by_similar(&["singular", "accusative"], &["accusative", "singular"]);
    }

    #[test]
    fn sort_tags_by_similar2() {
        make_test_sort_tags_by_similar(
            &["accusative", "singular", "neuter", "nominative", "vocative"],
            &["accusative", "neuter", "nominative", "singular", "vocative"],
        );
    }

    #[test]
    fn sort_tags_by_similar3() {
        make_test_sort_tags_by_similar(
            &["dual nominative", "accusative dual", "dual vocative"],
            &["accusative dual", "dual nominative", "dual vocative"],
        );
    }

    fn make_test_merge_person_tags(received: &[&str], expected: &[&str]) {
        let mut vreceived: Vec<String> = to_string_vec(received);
        let vexpected: Vec<String> = to_string_vec(expected);
        merge_person_tags(&mut vreceived);
        assert_eq!(vreceived, vexpected);
    }

    #[test]
    fn merge_person_tags1() {
        make_test_merge_person_tags(
            &[
                "first-person singular present",
                "third-person singular present",
            ],
            &["singular present first/third-person"],
        );
    }

    // Improvement over the original that would return:
    // "first/second-person singular past",
    // "third-person singular past",
    #[test]
    fn merge_person_tags2() {
        make_test_merge_person_tags(
            &[
                "first-person singular past",
                "second-person singular past",
                "third-person singular past",
            ],
            &["singular past first/second/third-person"],
        );
    }

    #[test]
    fn remove_redundant_tags1() {
        let mut received = to_string_vec(&["foo", "bar", "foo bar", "foo bar zee"]);
        let expected = to_string_vec(&["foo bar zee"]);
        remove_redundant_tags(&mut received);
        assert_eq!(received, expected);
    }

    #[test]
    fn remove_redundant_tags2() {
        let mut received = to_string_vec(&[
            "first-person singular indicative preterite",
            "first-person singular preterite",
        ]);
        let expected = to_string_vec(&["first-person singular indicative preterite"]);
        remove_redundant_tags(&mut received);
        assert_eq!(received, expected);
    }

    #[test]
    fn remove_redundant_tags_duplicates1() {
        let mut received = to_string_vec(&["a b", "a b"]);
        let expected = to_string_vec(&["a b"]);
        remove_redundant_tags(&mut received);
        assert_eq!(received, expected);
    }

    #[test]
    fn remove_redundant_tags_duplicates2() {
        let mut received = to_string_vec(&["a b", "b a"]);
        let expected = to_string_vec(&["a b"]);
        remove_redundant_tags(&mut received);
        assert_eq!(received, expected);
    }

    #[test]
    fn remove_redundant_tags_duplicates3() {
        let mut received = to_string_vec(&["a b", "c a b", "b a", "b a c", "c b a"]);
        let expected = to_string_vec(&["b a c"]);
        remove_redundant_tags(&mut received);
        assert_eq!(received, expected);
    }

    #[test]
    fn tags_subsets() {
        assert!(tags_are_subset("foo bar", "bar foo baz"));
        assert!(!tags_are_subset("foo qux", "foo bar baz"));
    }
}
