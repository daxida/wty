- [x] Download data from kaikki
- [x] basic cli, dict, test
- [x] replace expected in basic test with actual kaikki to yomitan
- [x] narrow lang type (el, Greek, greek) < MAKE SCRIPT
- [x] basic logger, forms / form_of
- [x] fix ipas
- [x] basic etymology_text, head_info_text
- [x] add non-edition lang to tests / workflow (target vs edition etc.) > sq has no edition
- [x] replace elapsed time with tracing
- [x] add Greek testsuite, English testsuite
- [x] Remove redundant tags (turned off for testing)
- [x] tag_bank, styles.css
- [x] write missing tags / pos etc.
- [x] move jsons to assets so one can delete DATA safely 
- [x] add data/ to gitignore, rename language/ to dict/
- [x] add --keep-files (sort of)
- [x] add --filter/reject 
- [x] basic README
- [x] remove (some) emojis
- [x] remove edition language publicly
- [x] rename TidyReturn to Tidy, pass it silently to write yomitan
- [x] iterdir for testsuite (instead of hardcoding)
- [x] also log non-skipped pos/tags
- [x] remove RawEntryfoo code
- [x] [RELEASE]
- [x] don't write .gz file to disk
- [x] do the backlink in tidy instead (better for debug)
- [x] rename downloaded en versions raw jsonls to make it consistent (and to be able to rust tests...)
- [x] move FilterKey validation to CLI
- [x] verbose flag
- [x] [en] finish porting the EN testsuite
- [x] finish porting the testsuite
- [x] bring the registry code
- [x] rename/refactor Tidy now that we won't have to fight with diffs

- [ ] localize tags for fun?
- [ ] Exit code?

# Requires porting the testsuite first or its a mess
- [ ] Be faster ? flamegraph
- [ ] A way to be faster is to shrink as much as possible the Tidy objects
- [ ] Args > Args + Ctx (context)
- [ ] calver


- [x] ureq over reqwest - bloaty-metafile 
      @ [reddit](https://www.reddit.com/r/rust/comments/1osdnzd/i_shrunk_my_rust_binary_from_11mb_to_45mb_with/)

## FAILED
- also pass cached filtering wordentries
  Turned out to be slower, not sure why
- test deserialize to &str
  Failed because it requires BIG ASSUMPTIONS on the characters (f.e. that it does not have to escape stuff)

## USELESS BACKLOG
- [ ] I don't think the build.py is really needed, maybe just read the jsons at runtime...
- [ ] dont hardcode forms/lemmas when writing IR < write_tidy_result (apparently this is done by original for forms only?)

## DIFFS
- filetree
- made DATA deletable (important assets MUST be somewhere else)
- Do not use raw_glosses/raw_tags
- Fixed merge_person_tags not merging three persons at once
- Fixed etymologies being added in the wrong order (αρσενικό)
- Fixed etymologies missing
- (Potentially) add final dots
- sorting order when serializing
- sorting order for tags in forms term_bank (the original didn't sort which caused duplicates and inconsistent order)
- dont download gz for En edition
- dont extract for En edition
- pass tidy IR result when possible
- deinflections are wrongly serialized in Tidy
- Japanese dict is broken
- Added FormSource for better debugging
- The thai (th) testsuite is about a malformed page > ignore

## NOTES
- the '\r' trick depends on terminal size!
