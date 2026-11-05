# Pereplyot — From Reading to Writing (Plan II)

Written 2026-11-05 against v0.22.1 "Full Measure". **Status: phases 10–14 and 16–19 implemented (unreleased);
15 (scripture) left out by choice.** Settled with Cal 2026-11-05: explicit per-click lookups with an off switch, and one
record per document and nothing more. What each phase left undone is listed at the top of `CHANGELOG.md`.

`ACADEMIC-READER-PLAN.md` (phases 0–9) is built: rendering, Reading mode, OCR, navigation that
keeps your place, areas and tags, import/export of marks, the Typst Notebook, EPUB page lists,
retrieval, the palette and caret browsing. Pereplyot now **reads and marks** as well as anything on
any platform. This plan covers what comes next. Pereplyot should **know what it is reading, what
that text cites, and what you have made of it**, and carry all three into the essay, the sermon
or the exam.

Phase numbers carry on from the first plan (10–19). Earlier decisions still hold: **no AI**, **no
pen input**, **notebooks are Typst**, **read-aloud is deferred**, **local-first**.

---

## 1. Where Pereplyot stands

### Strengths to protect

- The four-colour *method* (key idea / evidence / connection / problem), not just four colours.
- Plain-file storage keyed by content hash, three-way merged saves, earlier versions, backup.
- Reading mode with printed page labels and sidenotes, which no other Linux reader has.
- Hover previews that find a bibliography entry even when the citation isn't a link
  (`reflow/citations.rs`).
- Output is writing: Typst/LaTeX/Markdown exports with printed page labels, the Notebook,
  *Open in Zerkalo*.

### Gaps found in the code

| # | Finding | Where | Why it matters |
|---|---|---|---|
| G1 | **A standalone document has no bibliographic identity.** It has only a typed `citation_key` in `LocalMeta`: no title, author, year, DOI or container record, and no citation styles. The tools for this already exist in a dependency: `fond-bib` has `hayagriva` plus DOI/arXiv/CrossRef/OpenLibrary fetchers (`acquire.rs`), and `fond-doc` has `find_doi`/`find_isbn`. | `pereplyot-ui-gtk/src/reader_host.rs:38` | An export cites `@key`, and outside a Kartoteka vault that key resolves to nothing. "Copy with citation" can't produce a Chicago or SBL footnote, which is the thing a student actually pastes. |
| G2 | **References are a hover trick, not a structure.** `citations.rs` finds *one* entry under the pointer. Nothing lists a document's bibliography, shows where a source is cited, or connects a reference to a copy you own. | `reflow/citations.rs` | The key question in a literature review is "what does this cite, and do I have it?" At the moment that means scrolling to the back of the document. |
| G3 | **Carrying marks to a changed file copies coordinates unchanged.** `reattach_local` rewrites the key and keeps every quad. It only triggers when the *same path* has new contents. | `reader_host.rs:290`, `ui/window.rs:795` | That's fine for an OCR'd copy, but wrong for a preprint → published version or a new edition: every mark lands on the wrong words, with no warning. |
| G4 | **Synthesis is linear.** The Notebook is a single Typst page, cards can't be reordered (they can only be removed and dropped again), and nothing gives a view across several sources. | `notebook_ui/view.rs:383` | A literature review compares sources against questions. A linear page can't show that Smith says nothing about method. |
| G5 | **Resurface is a random daily pick** (`resurface::pick(hits, seed)`), not a schedule. | `ui/resurface.rs:15` | For exam revision it shows the same passages you already know as often as the ones you're forgetting. |
| G6 | **Structure that academic books already contain is ignored**: the back-of-book index, lists of figures, tables as data. The sidebars are only Contents and Notes. | `pdf/sidebar.rs` | A monograph's index is the best search tool it has, and it already sits on the page, unlinked. |
| G7 | **One machine only.** Backup exists, but sync doesn't, and there's no way to read a supervisor's or seminar's marks next to your own. | `backup.rs` | Students read on a laptop and a desktop, and seminars read together. |
| G8 | **Debt.** The CI smoke test has failed two hover checks on every run since 0.16.1, and the release workflow gates only on build & test. The Reading-mode far jump still stalls for about 2.5 s (OPEN-ITEMS §1). A scan uses about 643 MB after open (BUDGETS). The open stall is 166 ms on a 600-page book. The README's *Status* section still describes v0.11.0. | `OPEN-ITEMS.md`, `tests/perf/BUDGETS.md` | Phases 12 and 16 rely on fast far jumps in Reading mode (following a reference or an index entry), and a red CI that everyone ignores will hide the next real regression. |

### What the best tools do that Pereplyot still doesn't

- **Semantic Reader / Scholarcy:** a reference list beside the paper, citation contexts ("where
  is this cited?"), and reference cards that open the cited paper.
- **Zotero 7:** an item record behind every PDF, CSL styles, and a citation inserted into the word
  processor.
- **LiquidText / Heptabase:** a spatial or tabular view across sources.
- **Anki / RemNote:** spaced repetition built from what you read.
- **Logos / Accordance (theology):** hover a scripture reference to read the passage, and a
  scripture index across your library. No general-purpose reader does either.
- **Skim / Okular:** shared annotation files for review.

---

## 2. North star

> You open a chapter. Pereplyot already knows it's *Harrington, Interpreting the New Testament
> (1979)*. Its references sit in a sidebar, and three of them are marked ● *in your library*. You
> hover "(Kloppenborg 1987, 41)", choose *Open your copy*, and land on printed page 41 of
> Kloppenborg. You mark two passages, tag them `#Q-source`, and close the book. On Friday, the
> synthesis matrix for your essay shelf shows which of your six sources has nothing yet on
> method. The annotated bibliography compiles in SBL style. On Monday, Review asks you to recall
> the passage you marked on Kloppenborg p. 41.

The three postures from Plan I (Read / Study / Synthesise) stay the same. This plan adds:

- **Know**: a document card and a References tab on the left sidebar (Phases 11–12).
- **Compare**: the matrix and the précis (Phase 14).
- **Remember**: a Review tab on the launcher (Phase 17).

Layout rule, unchanged: **reader chrome stays within 1000 px.** New features go into sidebar tabs,
the ⋮ menu and the Ctrl+K palette, never into the status bar.

---

## 3. Phases

Each phase has **UX**, **engineering** and **done when** sections, and ships as its own minor
release.

### Phase 10 — Foundations II: debt and document identity *(little visible change)*

**Engineering**

1. **CI you can trust.** Find out why the two hover smoke checks fail in CI (most likely the
   hover delay on a slow runner: wait for the popover widget, not for a change in ink count). Make
   `release-flatpak.yml` require the smoke job too. A permanently red check is worse than having
   none.
2. **Windowed text in Reading mode** (OPEN-ITEMS §1). Keep only the pages near the viewport in the
   `TextView`, with spacer heights for the rest. Then move scroll position, selection, search
   hits, margin labels, note bubbles and the page counter onto page offsets. Phases 12 and 16 make
   far jumps routine, so this has to come first.
3. **Scan memory.** Load PDFium from the file through a reader callback, or from a single
   shared buffer, instead of `bytes` plus a cloned `Vec`. Target: ≤ 300 MB + 1.0 × file size.
4. **Open stall.** Estimate page geometry from the first page's box and refine it on the worker,
   so a 600-page book doesn't measure 600 pages on the GTK thread. Target ≤ 50 ms (16 ms is still
   the ideal).
5. **Document identity record**, `documents/<hash>.json`. It stores the content hash *plus* a
   set of fingerprints: the PDF trailer `/ID[0]` (survives incremental saves by other programs),
   DOI, ISBN, arXiv id, EPUB `dc:identifier`, a normalised title + first author + year, and a
   short text shingle of the first pages. Phases 11, 12 and 13 are built on this record. It is
   derived data that can be rebuilt from the file, and the bibliographic fields added in Phase 11
   are backed up.
6. **Refresh the README's *Status* section**, which still describes 0.11.0.

**Done when:** CI is green including smoke and the release waits for it, a far jump in Reading
mode on `book-600` stays under 100 ms, the scan budget is met, and every opened document has an
identity record.

### Phase 11 — Every document knows what it is

**UX**

- **Document card** (an *Info* tab on the left sidebar, plus *Edit details…* in ⋮): title,
  authors/editors, year, type (article, book, chapter, thesis), container (journal or edited
  volume), volume/issue/pages, publisher, DOI/ISBN, and the citation key.
- **Filled automatically from the file first:** XMP (`dc:title`, `dc:creator`, `prism:doi`), the
  PDF Info dictionary, a DOI/ISBN/arXiv id found on the first and last pages, and EPUB OPF
  metadata. Each field shows where it came from ("from the PDF's metadata", "found on p. 1").
- **Looked up online only on request:** *Look up 10.1017/…* uses `fond-bib::acquire` (CrossRef,
  arXiv, OpenLibrary). It's an explicit button, and the first use explains what is sent where. A
  *Never go online* switch removes the button entirely.
- **Citation styles** through `hayagriva`'s CSL: Chicago notes–bibliography, Chicago
  author–date, APA 7, MLA 9, **SBL 2**, Turabian. The choice is global and can be overridden per
  shelf. *Copy with citation* gains *as footnote* ("Harrington, *Interpreting the New Testament*,
  42.") and *as reference list entry*, using printed page labels. The first footnote in a
  notebook export is the full form and later ones are short forms.
- **Exports are self-contained:** Typst/LaTeX exports write `refs.yml`/`refs.bib` next to the
  file, so they compile without Kartoteka.
- **Library cards show author and year**, with sorting by author, year or title.
- **Vault mode:** Kartoteka's entry is the record. The card is read-only and *Edit in Kartoteka*
  opens it there. Standalone, *Send to Kartoteka* (which already exists) hands over the record
  as well as the file.

**Boundary with Kartoteka** (worth stating, so Pereplyot doesn't turn into it): one record per
document, with no collections, no BibTeX library management and no duplicate merging. Shelves
remain the only grouping.

**Engineering:** stored as hayagriva YAML in `bib/<hash>.yml`, added to `backup.rs` `DIRS`. The
existing `citation_key` migrates into it. The key is generated with Kartoteka's convention (check
`fond-bib` for the generator, and reuse it rather than re-implement it). Extraction is pure
functions over `fond-doc` text, tested on fixtures. CSL rendering and the network calls run on a
worker.

**Done when:** offline and standalone, title/authors/year/DOI come out right for ≥ 8 of 10 fixture
articles and ≥ 7 of 10 fixture books, *Copy citation (SBL, footnote)* matches the SBL Handbook
examples, and a Typst export compiles with its generated bibliography.

### Phase 12 — The reference graph: what this cites, and whether you have it

**UX**

- **References tab** (left sidebar): the document's bibliography, parsed into entries, each with
  a count ("cited 7×"). Expand an entry to see its **citation contexts**: the sentence around
  each place it is cited, with printed page. Click one to jump there; Back returns.
- **From the text:** the existing hover preview gains *Show all places this is cited* and *Open
  your copy*.
- **In your library:** each entry is matched against identity records (DOI exactly, otherwise
  normalised title + first author + year) across History, Library and the Kartoteka vault.
  Matched entries get a ●. *Open your copy* opens that document, and **when the citation names
  a page ("Kloppenborg 1987, 41") it opens at printed page 41**. No reader does this, and it's the
  single most useful thing in this plan for anyone writing a thesis.
- **Not in your library:** *Copy reference*, *Open DOI*, *Add to Kartoteka by DOI* (an additive
  Kartoteka flag), or *Search my documents for this title*.
- **Cited by (in your library):** a document's card lists the documents you own that cite it,
  built in the background by the index worker.
- **Humanities citation styles first-class:** many books have no bibliography, only notes.
  Parse full notes ("1. Author, *Title* (City: Publisher, Year), 42.") and short notes ("Author,
  *Short Title*, 42."), and resolve *ibid.* and *op. cit.* to the previous note's source. Those
  become entries too.

**Engineering**

- Turn `citations.rs` into a reference parser (pure functions): split entries (hanging indent,
  leading labels, year patterns), then extract authors/year/title/container/DOI with heuristics.
  No model, per the no-AI decision. Low-confidence fields are left empty rather than guessed.
- Fixtures in eight styles: APA, Chicago NB (bibliography + notes), Chicago author–date,
  Harvard, IEEE/Vancouver numeric, MLA, SBL, and a footnote-only humanities monograph. Every
  case is fuzzed, as the parsers already are.
- A per-document cache of the reference index under `~/.cache/pereplyot/refs/`, built on the
  render/index worker and rebuildable.
- Cross-app: an additive `--add-doi=` (or a D-Bus action) in Kartoteka, which needs a Kartoteka
  release *first*, as the README already requires for any new flag.

**Done when:** across the fixtures, ≥ 90 % of entries split correctly, ≥ 80 % of in-text
citations resolve to their entry, and following a page-bearing citation to an owned copy lands on
the cited printed page.

### Phase 13 — Marks that survive new versions and editions

**UX**

- **Detection beyond same path:** when a newly opened file's identity record matches one you've
  annotated (same DOI, same `/ID[0]`, or the same title/author/year), Pereplyot offers: "You have
  42 notes on another version of this article (preprint, opened March). Bring them over?"
- **Re-anchoring, not copying:** every mark is located again by quote, prefix/suffix and position.
  The fuzzy anchors from `anchor.rs` are already used for EPUB, and PDF marks keep their context.
  Each mark ends up as **exact**, **moved** (same words, new place), **uncertain** or **orphaned**.
- **Review queue** for uncertain and orphaned marks: old and new versions side by side in the
  existing split view, with *Accept*, *Re-select* or *Keep as orphan*. Orphans stay listed in
  Notes under "Not found in this version" and are **never deleted**.
- Page labels are recomputed for the new edition, and notebook quote cards and connections that
  pointed at the old ids follow through an id map.
- **Compare versions** (⋮ → *Compare with…*): two versions in split view, with changed
  paragraphs marked from a diff of the structured text (preprint vs. published, first vs. second
  edition).
- The current same-path case (OCR'd or re-saved) also goes through re-anchoring, so a re-saved
  file whose layout shifted is caught too.

**Engineering:** a pure function `reanchor(old_marks, new_text) -> Vec<Outcome>` over
`PdfText`/EPUB text with a confidence score, tested on fixture pairs (preprint → journal layout,
first → second edition with renumbered pages, a scan → its OCR'd copy). Areas re-anchor by caption
text, then by a perceptual hash of the crop, and become orphans otherwise. The old sidecar is left
in place, as now.

**Done when:** on the preprint → journal fixture pair, ≥ 95 % of marks are exact or moved, the
rest are flagged, none are lost, and the notebook's quotes still open the right passage.

### Phase 14 — Synthesis II: the matrix, the précis, the annotated bibliography

**UX**

- **Synthesis matrix** (from a shelf or a notebook, ⋮ → *Matrix*): rows are sources, columns are
  tags or questions you define ("Method", "Date of Q", "Argues against Farrer?"), and each cell
  holds the marks from that source with that tag, plus a short note you write. **Empty cells are
  shown as empty**, which is the point of the matrix. Click a quote to open it at its place.
  Export to a Typst table, CSV or Markdown.
- **Précis per document** (Info tab): built from the four colour meanings (key ideas, evidence,
  connections, problems) with room for your own three-sentence summary. *Shelf → Annotated
  bibliography* turns each précis into a CSL entry (Phase 11) followed by your summary, as one
  Typst document that compiles. Coursework asks for this constantly, and nothing produces it.
- **Notebook II** (the Plan I "not yet" items, plus more):
  - drag cards to reorder them;
  - a headings outline in the notebook's sidebar;
  - backlinks: a Notes card shows "quoted in *Essay 2*, *Seminar 5*";
  - several notebooks per shelf, with a switcher;
  - a word count;
  - full-form/short-form footnotes on export (Phase 11).
- **Writing hand-off across the suite (cross-app, additive):**
  - **Zerkalo:** *Insert at cursor in Zerkalo* (quote + `@key` + page) over D-Bus, so the essay
    being written receives the quote directly instead of through a copied file.
  - **Iskra:** *Add to sermon* sends a quote with its source to the open sermon's ideas.
  - Each needs a release of the receiving app first.

**Engineering:** the matrix is a plain file (Typst data block or TOML) under `matrices/`, added to
backup. Cells are queries over the existing annotation index, so the matrix never stores copies of
quotes, only its columns, notes and source list. Every export shape gets a real `typst compile`
test (cross-cutting rule).

**Done when:** from a five-source shelf you can build a four-column matrix, export it, and generate
an SBL annotated bibliography, and both compile.

### Phase 15 — Domain references: scripture first

Cal's own field, and an open gap: no general reader understands that "Rom 8:28–39" is a
citation.

**UX**

- **Scripture references are detected** in any document: "Rom 8:28–39", "1 Cor. 13", "Mk
  4.1–20", "vv. 4–6" (inherits the book from the context before it), and lists ("Isa 40:3; Mal
  3:1").
- **Hover a reference to read the passage** in a bundled public-domain translation (World English
  Bible; KJV optional), with *Copy passage* and *Copy reference*. Other translations are
  OSIS/USFM files you add yourself, with no downloads, matching the OCR language policy.
- **A scripture index for this book** (a generated tab, in canonical order) and **across your
  library**: the Search tab gains `@Rom 8`, which matches references in the text *and* your marks
  on passages about it.
- **Iskra link:** from a lectionary Sunday, *What have I read on these readings?* opens
  Pereplyot's search (a new additive `--search=` flag).
- Original languages: check Reading mode with polytonic Greek and right-to-left Hebrew (OFL Noto
  Serif Greek/Hebrew if the LaTeX Look's font lacks them). Plan I left RTL pagination open.

**Engineering:** lift Iskra's `bible.rs` book dictionary into a `fond-scripture` crate in fond-core
(Iskra and Pereplyot share it, the same way `fond-index` was meant to be shared). Add a range parser
and a `ReferenceScheme` trait, so classical citation schemes (Stephanus "Rep. 514a", Bekker "EN
1094a") can be added later without redesign. Only scripture ships in this phase. The translation
text is a compact index loaded on the worker.

**Done when:** on a theology fixture monograph, ≥ 95 % of references are detected with correct
ranges, the hover shows the right verses, and `@Rom 8` finds both text references and marks.

### Phase 16 — Structure books already have: index, figures, tables

**UX**

- **The back-of-book index, live** (an *Index* tab): detect the index pages, then parse entries,
  sub-entries, ranges ("45–47"), notes ("203n4"), "passim" and *see also*. Printed labels map to
  pages through the existing page-label machinery. Search it, expand an entry to see every
  location with a snippet, and hover for a preview. "203n4" opens note 4 on p. 203 in Reading mode.
- **Figures tab:** every figure and table with its caption (`reflow/figures.rs` already finds
  captions) as a thumbnail grid. Click one to go there, or pin it (as with existing pins).
- **Tables as data:** an area clip over a table offers *Copy as table* (TSV, CSV, Typst `table`),
  with columns found by clustering word boxes. The picture stays the reference, and the data is
  offered alongside it, never in its place.
- **Equations stay images.** Recognising LaTeX from an equation would need a model, which the
  no-AI decision rules out. Equation-number hover already works.

**Done when:** a fixture monograph's index resolves ≥ 95 % of its locators to the right pages, and
a fixture table round-trips to CSV with correct cells.

### Phase 17 — Remembering what you read

**UX**

- **Review** (a launcher tab, opt-in): spaced repetition built from your own marks.
  - Turn on *Remember this* for a mark, a tag or a whole shelf.
  - **Card types:** cloze (the highlighted phrase blanked inside its sentence), the précis
    question ("What is Harrington's thesis?"), and your own `Q: … A: …` lines written in a note.
  - Rate with **1–4**, the same keys as marking. Every card shows source and printed page, with
    *Open in context*.
- Resurface keeps its "a few passages on the launcher" spot, but draws from due and
  nearly-forgotten cards instead of a random seed.
- **Reading log** (opt-in, local, off by default): minutes and pages per document, which gives
  "≈ 25 min left in this chapter, at your pace" in the breadcrumb tooltip (not a new status-bar
  control). Sputnik could read it later for reading-load planning, using an additive flag.

**Engineering:** implement the FSRS scheduler formulas directly (a couple of hundred lines with
pure tests; no optimiser, no ML dependency). Review state lives in `review/<hash>.json`, *outside*
the annotation sidecar, so the `fond-annot` schema and Kartoteka/Sputnik are untouched. Added to
backup.

**Done when:** a 200-card deck schedules correctly against FSRS reference values, a review session
runs entirely from the keyboard, and turning Review off leaves no trace in any sidecar.

### Phase 18 — More than one machine, more than one reader

**UX**

- **Sync folder** (Preferences): point the data folder at a Syncthing/Nextcloud/Dropbox folder.
  Notes already merge by id. Everything else is made sync-safe too, and sync clients' conflict
  copies (`*.sync-conflict-*`, `* (conflicted copy)*`) are found and merged automatically. No
  server and no account.
- **Layers** for seminars and supervision:
  - *Share my notes on this document…* writes one file (marks + your name).
  - *Add a layer…* shows someone else's file beside yours: read-only, outlined rather than
    filled, with their name, and toggled per layer.
  - *Adopt* copies one of their marks into your own.
  - Annotated-PDF export (which exists) stays the route to people who don't use Pereplyot.

**Engineering:**

- Each data file is audited for merge semantics. Bookmarks become a set union. `LocalMeta`'s
  last-writer-wins fields get per-field timestamps. The notebook, matrix and connections become
  per-device journals, or merge by block id.
- Layers are stored as `layers/<hash>/<name>.json`, separate files rather than new sidecar
  fields, so this needs no schema change.
- A two-directory sync simulation test that interleaves edits from both sides.

**Done when:** two profiles syncing through one folder, edited alternately and concurrently, lose
nothing across a 1,000-step randomised test, and a shared layer can be shown, hidden and adopted
from.

### Phase 19 — Finish for 1.0

- **Narrow windows:** Cal kept the defaults before (OPEN-ITEMS §3). Raise it again, since split
  view, three sidebars and the matrix make an `adw::Breakpoint` collapse more valuable, provided
  the default-width chrome and smoke coordinates don't change.
- **An Orca pass** over everything added since Plan I: Notes cards, the notebook, the matrix,
  Review, the References and Index tabs.
- **Look up…** on a selection: user-editable URL templates (Wikipedia, Logeion for Greek/Latin,
  PhilPapers, a library catalogue, Google Scholar). These open the browser and send nothing
  automatically.
- **First-run teaching:** the Welcome window offers a fictional sample document with demo marks,
  which explains the four-colour method by showing it.
- **Re-measure every budget** with the new features in `tests/perf/BUDGETS.md`, and declare 1.0
  when every row passes.

---

## 4. Cross-cutting rules

Carried over from Plan I: CLI flags are additive only, with the receiving app released first. No
writer of a new `fond-annot` field ships before every consumer has the tolerant parser. Disk, PDF,
OCR, index and network work stays off the main thread. Persistence goes through
`fsutil`/`SidecarSync`, and new user data is added to `backup.rs`. Every export shape compiles in a
test. CHANGELOG and README are updated in the same session.

New in this plan:

- **Network is explicit.** No request leaves the machine without a click on a button that names
  the service, and *Never go online* hides every such button.
- **Heuristics never lose text silently.** Every parser (references, notes, index, scripture,
  tables, re-anchoring) has a fixture suite, a fuzz target and a visible fallback state
  ("unmatched", "uncertain", "orphaned").
- **Prefer separate files to schema changes.** Bibliographic records, review state, layers and
  matrices each get their own file, so Kartoteka and Sputnik are never forced to upgrade.
- **Chrome width.** New features go into sidebar tabs, ⋮ and the palette, never the status bar.

## 5. Questions for Cal

1. **Network lookups** (Phase 11): is an explicit, per-click CrossRef/OpenLibrary lookup
   acceptable, or should standalone metadata come only from the file and Kartoteka?
   *Recommendation: explicit lookup, with the off switch.*
2. **How much bibliography Pereplyot itself should hold:** one record per document and no more
   (recommended), or a lighter Kartoteka of its own?
3. **Scripture** (Phase 15): ship it as a first-class feature? Which translations should be
   bundled? (WEB is public domain; most modern ones aren't licensable.)
4. **Review / spaced repetition** (Phase 17): wanted, or out of scope for a reader?
5. **Sync folder and layers** (Phase 18): wanted, or is backup enough?
6. **Narrow-window layout:** revisit now, or keep it closed?

## 6. Suggested order and releases

| Release | Phase | Theme |
|---|---|---|
| 0.23 | 10 | Trustworthy CI, windowed Reading mode, identity records |
| 0.24 | 11 | Document card, CSL styles, self-contained exports |
| 0.25 | 12 | References tab, citation contexts, *Open your copy* at the cited page |
| 0.26 | 13 | Re-anchoring across versions and editions, Compare |
| 0.27 | 14 | Matrix, précis, annotated bibliography, Notebook II, Zerkalo/Iskra hand-off |
| 0.28 | 15 | Scripture references (theology first) |
| 0.29 | 16 | Live back-of-book index, Figures tab, tables as data |
| 0.30 | 17 | Review (FSRS), reading log |
| 0.31 | 18 | Sync folder, shared layers |
| 1.0 | 19 | Narrow layout, Orca pass, Look up…, budgets met |

Phases 10 → 11 → 12 → 13 → 14 depend on each other in that order (identity → record → graph →
re-anchoring → synthesis). Phases 15, 16 and 17 each depend only on Phase 10 and can be moved
earlier. Phase 15 is cheap because Iskra's book dictionary already exists.
