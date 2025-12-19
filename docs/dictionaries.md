WIP. These are the different types of dictionaries that can be made with kty:

```console
$ kty main              [OPTIONS] <SOURCE> <TARGET> [DICT_NAME]
$ kty ipa               [OPTIONS] <SOURCE> <TARGET> [DICT_NAME]
$ kty ipa-merged        [OPTIONS] <TARGET> [DICT_NAME]
$ kty glossary          [OPTIONS] <SOURCE> <TARGET> [DICT_NAME]
$ kty glossary-extended [OPTIONS] <EDITION> <SOURCE> <TARGET> [DICT_NAME]
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
