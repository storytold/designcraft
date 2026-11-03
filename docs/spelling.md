# Spelling and proofreading

DesignCraft checks spelling with the suite's shared engine, [`craft-proof`](https://github.com/storytold/craft-libs/tree/main/crates/proof),
and reviews it with the shared interface, [`craft-proof-ui`](https://github.com/storytold/craft-libs/tree/main/crates/proof-ui).

## Using it

**Edit > Spelling > Check Spelling…** opens the Spelling panel beside the page, on the first issue in
the document, with the word selected in the text. Clicking a suggestion corrects the word and moves
on; corrections are ordinary undoable edits. **Ignore Once** and **Ignore All** last for this pass, and
**Add to Dictionary** adds to the document's user dictionary. **Change…** takes a typed replacement. The
document stays editable: if the text under the current issue changes, the panel asks to check again
instead of applying a correction to the wrong place.

Right-clicking a misspelled word while typing offers its suggestions, Add to Dictionary and Check
Spelling…. **Edit > Spelling > Dynamic Spelling** underlines issues as you type, and proofs a story again
only when its text, its styles or the user dictionary change.

Each run of text is checked in its own language; **[No Language]** text is not checked. A language with
no dictionary installed is not checked either.

## Dictionaries

| Language | Source |
|---|---|
| English | The Language data from `CRAFT_I18N_DIR` (SCOWL) when a build embeds it, else an installed Hunspell dictionary, else the Moby list built into `craft-proof` |
| Khmer | The optional data pack, below |
| Others | An installed Hunspell dictionary or Language pack, as `craft-proof` documents |

Hyphenation does not use these: it keeps its own patterns.

## Khmer

With the Khmer data included, unspaced Khmer gets spelling suggestions and preferred-form checks. Those
checks are limited rules, not a general grammar checker. Text in the default English style is checked as
Khmer; an explicit other language or [No Language] is respected. Word boundaries, repetition signs and
honorifics are handled in the shared engine, and offsets always address the original text: nothing is
normalized or inserted.

The default build contains no Khmer dictionaries. It never reports Khmer as checked without them: the
panel says the text was not checked, and `spelling.status` reports availability. Build with the data as
[`crates/khmer/DATA.md`](https://github.com/storytold/craft-libs/blob/main/crates/khmer/DATA.md) in
craft-libs describes, setting `CRAFT_KHMER_DIR` and `CRAFT_KHMER_REQUIRED=1`.

SBBIC identifies its internal work, the SEAlang frequency lists and SIL NRSI's khmerlbdict as the data's
sources; their MIT notices are in `NOTICE`. The tagger was trained on internal documents that SBBIC has
permission to train with; those documents are not distributed.

## For automation

`spelling.check {suggestions:false}` lists every issue without generating candidates;
`spelling.suggest {story,start,end}` revalidates one issue and returns its candidates;
`spelling.at {story,at}` returns the issue around an offset, or null. `app.checkSpelling` opens the panel
and `app.spelling {action,to?}` carries out its actions.
