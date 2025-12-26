## Dictionary types

These are the different types of dictionaries that can be made with kty (optional parameters have been omitted):

```console
$ kty main              <SOURCE> <TARGET>
$ kty ipa               <SOURCE> <TARGET>
$ kty ipa-merged        <TARGET>
$ kty glossary          <SOURCE> <TARGET>
$ kty glossary-extended <EDITION> <SOURCE> <TARGET>
```

- **main**: main dictionaries, with etymology, examples etc. These have good coverage, but tend to be verbose.
- **glossary**: short dictionaries made from Wiktionary translations section.
- **ipa**: pronunciation dictionaries.

!!! tip "Reminder: roughly, the source is the language we learn. The target is the language we know."

| Dictionary type | Edition(s)  | Source  | Target  |
| --------------- | -------- | ------- | ------- |
| **main**        | **TARGET** | source  | **TARGET** |
| **ipa**         | **TARGET** | source  | **TARGET** |
| **ipa-merged**  | ALL    | X    | target |
| **glossary**    | **SOURCE** | **SOURCE** | target |
| **glossary-extended**    | edition | source | target |

!!! tip "Identical cells in a row are highlighted in bold UPPERCASE"

## Paths

When building locally, dictionaries are usually stored in: `ROOT/dict/SOURCE/TARGET/kty-SOURCE-TARGET.zip`.

The only exception being ipa-merged, since it has no source.

```console
$ kty main de en
✓ Wrote yomitan dict @ data/dict/de/en/kty-de-en.zip (16.05 MB)
$ kty glossary de en
✓ Wrote yomitan dict @ data/dict/de/en/kty-de-en-gloss.zip (3.58 MB)
$ kty ipa-merged en
✓ Wrote yomitan dict @ data/dict/en/all/kty-en-ipa.zip (4.45 MB)
$ kty glossary-extended all de en
✓ Wrote yomitan dict @ data/dict/de/en/kty-all-de-en-gloss.zip (2.70 MB)
```

