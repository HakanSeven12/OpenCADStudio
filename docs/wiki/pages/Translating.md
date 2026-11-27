# Translating

Open CAD Studio's interface texts are kept in [Fluent](https://projectfluent.org/) (`.ftl`) files, one per language, under [`locales/`](https://github.com/HakanSeven12/OpenCADStudio/tree/main/locales) in the repository:

```text
locales/en-US/opencadstudio.ftl   the source catalog, in English
locales/de-DE/opencadstudio.ftl   German
locales/tr-TR/opencadstudio.ftl   Turkish
...
```

`en-US` is the source. Every other catalog has exactly the same keys, with the text translated. The language is picked under **Options › General**; with *System language* the interface follows the operating system.

## How a catalog looks

```ftl
language =
    .system = System language
    .english = English

text-panel = Text
    .counts = { $characters ->
        [one] { $characters } character
       *[other] { $characters } characters
    }
```

- `language` and `text-panel` are messages, `.system` and `.counts` are their attributes. The message and attribute names (left of `=`) are never translated; only the text right of `=` is.
- `{ $name }` is a value the program fills in, such as a count or a file name. Keep every one of them, with the same name. You can move it within the sentence.
- `{ $n -> [one] ... *[other] ... }` picks a text by number. Use the plural forms of your language (`[one]`, `[few]`, `[many]`, `[other]` ...); the `*` marks the default, which must be there.
- Command names, and the options in prompts such as `[Convert/Revert/Show/Hide]` and `<Convert>`, are what the user types. Keep them in English, as the other catalogs do: `BCONSTRUCTION  Geometrie wählen oder [Convert/Revert/Show/Hide]:`.

## Fixing or improving an existing language

1. Fork the repository on GitHub.
2. Edit `locales/<language>/opencadstudio.ftl`, for example `locales/fr-FR/opencadstudio.ftl`. Search for the English text in `locales/en-US/opencadstudio.ftl` to find its key, then the same key in your language's file.
3. Open a pull request. Small pull requests (one area of the interface at a time) are reviewed fastest.

To see your change, build and run the app as described in the repository's README and choose the language under **Options › General**.

## Adding a new language

1. Copy `locales/en-US/` to `locales/<tag>/`, where `<tag>` is the language tag, such as `sv-SE` or `uk-UA`, and translate `opencadstudio.ftl`. Text you have not translated yet can stay in English, but every key must be there.
2. In `src/i18n.rs`, add the language to `Language` (its tag in `#[serde(rename = "...")]`), to `Language::ALL`, to `requested()` and to `label()`.
3. Add the language's name to the `language` message (for example `.swedish = Svenska`) in **every** catalog, so it appears in the language list in all languages.

If the code part is not for you, open a pull request with only the translated `.ftl` file and say so; the maintainers wire it in.

## Checks

The test suite checks every catalog against the source on each pull request:

- no key is missing and there are no extra or duplicate keys;
- the file is valid Fluent syntax;
- each text keeps the same `{ $name }` values as the English one;
- no text is empty where the English one is not.

You can run the same check locally:

```bash
cargo test --lib every_catalog_covers_and_formats_the_source_catalog
```

## Plugins

A plugin can be translated the same way. Each plugin starts with the interface language in `OCS_LANGUAGE` (read it with `ocs_plugin_api::host_language()`), so a plugin can ship its own `.ftl` files and pick its labels from them. Translations of a plugin go to that plugin's own repository.
