# Export Configuration for a Novel-Writing App

For a writing app that supports novels, short stories, and other long-form writing, I'd design the export system around two principles:

1. **Common settings should be easy to find and understand.** Most writers want to select a format, choose a few options, and export.
2. **Advanced settings should accommodate different publishing workflows.** Someone preparing a manuscript for an editor has different needs from someone creating an ebook for Kindle or a polished PDF for personal reading.

I'd organize the feature into three layers:
- **Basic settings:** Options almost everyone needs.
- **Format-specific settings:** Options for PDF, EPUB, and HTML.
- **Advanced features:** Automation, reusable templates, publishing workflows, and fine-grained customization.

One important architectural decision: **separate the content being exported from the settings controlling its presentation.** This lets you reuse the same manuscript and export configuration across formats without duplicating your content or mixing writing-related metadata with layout settings.

## 1. Common export settings

These settings should form the foundation of your export feature and be available regardless of the output format, wherever they make sense.

### A. Export scope and content selection

Writers need control over what goes into the exported document.

| Setting | Description | Priority |
|---|---|---|
| Export entire project | Export the complete novel, collection, or manuscript. | Essential |
| Export selected chapters | Export only specific chapters or sections. | Essential |
| Export selected text | Export a selection from the editor, if supported. | Nice to have |
| Include front matter | Include title page, dedication, preface, and other introductory material. | Essential |
| Include back matter | Include acknowledgments, author biography, appendices, and other ending sections. | Essential |
| Include table of contents | Generate a navigable or printed contents list. | Essential |
| Include notes | Include or exclude author notes and research notes. | Important |
| Include comments | Include or exclude editorial comments and annotations. | Important |
| Include revision history | Include previous versions or change information. | Nice to have |
| Preserve chapter order | Respect the project's defined chapter and section order. | Essential |

**Recommendation:** Allow writers to select content using the same structure they see in the writing app. If a project has parts, chapters, scenes, and subsections, the export interface should reflect that hierarchy.

For example:

- ☑ Part I: The Beginning
  - ☑ Chapter 1: The Arrival
  - ☑ Chapter 2: The Discovery
- ☑ Part II: The Journey
  - ☑ Chapter 3: The Crossing
  - ☐ Chapter 4: The Return
- ☑ Author's Note

This is especially useful when exporting a sample chapter for an editor or agent.

### B. Document metadata

Metadata identifies the work and can be used by ebook readers, browsers, and document viewers.

| Setting | Description | Priority |
|---|---|---|
| Title | Title of the book or document. | Essential |
| Subtitle | Optional subtitle. | Important |
| Author name | Author or pen name. | Essential |
| Additional contributors | Editors, translators, illustrators, and coauthors. | Important |
| Language | Primary language of the work. | Essential |
| Description | Short description or book synopsis. | Important |
| Copyright notice | Copyright statement and year. | Important |
| Publisher | Publisher name, if applicable. | Nice to have |
| Publication date | Publication or release date. | Nice to have |
| Identifier | ISBN or another identifier, when applicable. | Nice to have |
| Cover image | Image used as the cover where supported. | Important |
| Keywords | Search terms or descriptive metadata. | Nice to have |

**Implementation detail:** Store metadata with the project when possible, but allow export-specific overrides. For example, a writer might use a pen name for an EPUB while retaining their legal name in internal project information.

Also, don't require an ISBN. Many independent authors, unpublished writers, and people exporting personal reading copies won't have one.

### C. Typography and text formatting

These settings control how the writing looks.

| Setting | Description | Priority |
|---|---|---|
| Font family | Select a typeface for the exported text. | Essential |
| Font size | Set the default body text size. | Essential |
| Line spacing | Single, 1.15, 1.5, double, or custom spacing. | Essential |
| Paragraph spacing | Control spacing before or after paragraphs. | Essential |
| Paragraph indentation | Configure first-line indentation. | Essential |
| Text alignment | Left, justified, or other supported alignment. | Important |
| Heading styles | Configure chapter and section heading appearance. | Essential |
| Emphasis preservation | Preserve bold, italic, underline, and other supported inline formatting. | Essential |
| Special characters | Preserve typographic punctuation, symbols, and Unicode characters. | Essential |
| Scene breaks | Control the appearance of transitions between scenes. | Important |
| Drop caps | Use decorative initial letters at chapter openings. | Nice to have |
| Custom styles | Allow users to define reusable formatting styles. | Important |

I'd avoid making every typography setting equally prominent. A basic panel could offer a font, font size, spacing preset, and formatting toggle, with detailed controls in an advanced panel.

One subtle but important choice: **distinguish between preserving the author's formatting and applying a new export style.** A writer should be able to retain their existing emphasis and headings while changing the overall presentation.

### D. Page and layout settings

These are particularly important for PDF, but some can influence the other formats.

| Setting | Description | Priority |
|---|---|---|
| Page size | Letter, A4, trade paperback sizes, or custom dimensions. | Essential for PDF |
| Margins | Top, bottom, left, and right margins. | Essential for PDF |
| Orientation | Portrait or landscape. | Essential for PDF |
| Header and footer | Configure running headers and footers. | Important |
| Page numbers | Enable, disable, and configure numbering. | Essential for PDF |
| Starting page number | Set the initial page number. | Important |
| Chapter page breaks | Start each chapter on a new page. | Essential |
| Chapter opening position | Start chapters on a right-hand page or a new odd-numbered page. | Nice to have |
| Widow and orphan control | Avoid isolated lines at page boundaries. | Nice to have |
| Hyphenation | Enable or disable automatic word hyphenation. | Important |
| Page background | Set a background color or image. | Nice to have |

A useful distinction is between **page layout** and **reading layout**. PDFs have fixed pages, whereas EPUBs are usually reflowable. Avoid exposing page-oriented settings as if they behaved identically in all three formats.

### E. Export behavior and file handling

These settings make the feature easier to use repeatedly.

| Setting | Description | Priority |
|---|---|---|
| Output format | PDF, EPUB, or HTML. | Essential |
| File name | Set the exported file name. | Essential |
| Output location | Choose where to save the file. | Essential for desktop apps |
| Overwrite behavior | Ask before replacing an existing file. | Essential |
| Export profile | Save and reuse a collection of settings. | Important |
| Preview before export | Review the output before creating the final file. | Important |
| Open after export | Open the generated file automatically. | Nice to have |
| Export validation | Warn about missing metadata, broken links, or unsupported content. | Important |
| Error reporting | Explain what failed and how to fix it. | Essential |

I would make export profiles part of the initial design, even if the first release only supports a few built-in presets.

For example:

- **Manuscript for Editing**
- **Reader-Friendly PDF**
- **EPUB for E-readers**
- **Web Publication**
- **Plain HTML Archive**

A profile should contain presentation and export settings, not a snapshot of the manuscript itself.

---

## 2. Format-specific export settings

Some settings should appear only when they're relevant to the selected format. This keeps the configuration interface manageable.

### A. PDF export settings

PDF is ideal for printable manuscripts, reading copies, sharing with editors, and documents that need consistent page layout.

| Setting | Description | Priority |
|---|---|---|
| Page size | Letter, A4, trade paperback, or custom size. | Essential |
| Margins | Configure page margins. | Essential |
| Font embedding | Embed fonts to preserve appearance on other systems, subject to font licensing. | Essential |
| Page numbering | Enable numbering and choose its position and style. | Essential |
| Headers and footers | Display the book title, chapter title, author, or other text. | Important |
| Table of contents | Generate a contents page with page numbers. | Important |
| PDF bookmarks | Create a navigable outline from parts and chapters. | Important |
| Cover page | Add a cover as the first page. | Important |
| Print-friendly mode | Minimize backgrounds and decorative elements to save ink. | Important |
| Image quality | Configure image resolution and compression. | Important |
| Document permissions | Optionally restrict printing, copying, or editing. | Nice to have |
| Password protection | Require a password to open the PDF. | Nice to have |
| PDF accessibility | Generate a tagged PDF with meaningful document structure and reading order. | Important |
| PDF version | Select a supported PDF compatibility level. | Advanced |

**My recommendation:** Offer three built-in PDF presets.

1. **Standard Manuscript:** Letter paper, one-inch margins, 12-point serif text, double spacing, first-line paragraph indentation, and page numbers.
2. **Reading Copy:** Attractive typography, comfortable spacing, chapter-opening pages, and a clean title page.
3. **Print Layout:** Configurable trim size, margins, embedded fonts, and print-oriented page layout.

The Standard Manuscript preset is particularly valuable for fiction writers submitting work to agents, editors, and publishers.

A tagged, accessible PDF is also worth prioritizing. Proper document structure, reading order, headings, and image descriptions can make the exported document more usable with assistive technology. [Adobe Help Center](https://helpx.adobe.com/acrobat/using/creating-accessible-pdfs.html?utm_source=chatgpt.com)

### B. EPUB export settings

EPUB is the most important format to treat differently from PDF. It is designed primarily for digital reading, with text that normally reflows to fit different screens and user-selected font sizes. [W3C](https://www.w3.org/TR/epub-33/?utm_source=chatgpt.com)

| Setting | Description | Priority |
|---|---|---|
| EPUB version | Choose a supported EPUB specification version. | Essential |
| Reflowable layout | Let text adapt to the reader's screen and font preferences. | Essential |
| Fixed layout | Preserve a specific visual arrangement for illustrated or highly designed works. | Advanced |
| Table of contents | Generate navigable chapter and section links. | Essential |
| Cover image | Include an ebook cover. | Essential |
| Metadata | Populate title, author, language, description, and other book information. | Essential |
| Chapter splitting | Create separate content documents at chapter or section boundaries. | Important |
| Default font | Specify a preferred font while allowing reader overrides where appropriate. | Important |
| Embedded fonts | Include selected fonts when permitted by their licenses. | Nice to have |
| CSS theme | Apply custom styling for headings, paragraphs, scene breaks, and other elements. | Important |
| Publisher stylesheet | Include a custom stylesheet for more control over presentation. | Advanced |
| Images | Include illustrations and control their placement and sizing. | Important |
| Accessibility metadata | Record applicable accessibility information and features. | Important |
| Validation | Check the resulting EPUB for structural and packaging errors. | Essential |
| Language direction | Support left-to-right and right-to-left writing systems where applicable. | Important |

A few implementation details deserve special attention.

**1. Make the table of contents navigable.**

The EPUB should contain a proper navigation document, not just a visible list of chapters. EPUB 3.3 defines the navigation document as a required component. [W3C](https://www.w3.org/TR/epub-33/?utm_source=chatgpt.com)

**2. Preserve semantic structure.**

A chapter title should be a heading, not merely a paragraph with a larger font. Proper structure improves navigation and accessibility.

**3. Keep the reader in control.**

For reflowable EPUBs, avoid forcing a fixed font size, page size, or margin unless there is a specific reason. Readers should generally be able to adjust their reading experience.

**4. Include a validation step.**

Don't just check whether the file was created. Verify that its internal structure, metadata, navigation, and resources conform to the target EPUB specification.

### C. HTML export settings

HTML is useful for publishing stories on websites, sharing individual chapters, archiving work, and moving content into other writing or publishing systems.

I'd support two distinct HTML export modes.

- **Standalone HTML:** One self-contained HTML file, ideally with images and styles embedded when practical.
- **HTML package:** An HTML document or set of documents with separate CSS, images, and other supporting assets.

| Setting | Description | Priority |
|---|---|---|
| HTML version | Generate modern, standards-compliant HTML. | Essential |
| Single file or package | Export one document or multiple files. | Essential |
| CSS styling | Include styling for typography, spacing, and headings. | Essential |
| Inline styles or external stylesheet | Choose how presentation rules are stored. | Important |
| Responsive layout | Adapt to mobile, tablet, and desktop screens. | Essential |
| Table of contents | Generate linked chapter navigation. | Important |
| Chapter splitting | Create separate HTML documents for individual chapters. | Important |
| Image handling | Embed images or save them as separate files. | Important |
| Relative links | Preserve links between documents and local resources. | Essential |
| Document metadata | Include title, author, language, and description. | Important |
| Semantic HTML | Preserve headings, paragraphs, lists, and other structural elements. | Essential |
| Theme | Choose a basic visual theme. | Important |
| JavaScript | Optionally include scripts for enhanced functionality. | Nice to have |
| Minification | Reduce HTML and CSS file size. | Nice to have |
| Clean HTML | Generate readable markup with minimal application-specific styling. | Important |

**A particularly useful feature is clean HTML export.** Writers often want to paste content into a content management system, convert it to another format, or reuse it in a website.

For that purpose, offer an option that generates semantic HTML with minimal styling rather than exporting the writing app's complete visual presentation.

For security, strip or sanitize potentially unsafe HTML and scripts from user-generated content when producing HTML intended for public display.

---

## 3. Nice-to-have features

The following features can make the export system substantially more useful. I've grouped them by their likely value to fiction writers.

### A. Reusable export profiles and templates

I'd put this group near the top of the enhancement list because it benefits almost every writer.

| Feature | What it does | Value |
|---|---|---|
| Custom export profiles | Save and reuse combinations of export settings. | Very high |
| Built-in templates | Supply presets for manuscripts, ebooks, reading copies, and web publication. | Very high |
| Project-specific defaults | Remember preferred settings for each project. | High |
| Global defaults | Establish settings for new projects. | High |
| Export profile duplication | Create a variation without rebuilding a profile. | High |
| Import/export profiles | Share settings between installations or users. | Medium |
| Template inheritance | Create a profile based on another profile and override a few settings. | Medium |

For example, a writer could create a profile called **"Agent Submission"** that automatically applies:

- Letter-sized pages.
- One-inch margins.
- 12-point Courier New or another selected manuscript font.
- Double spacing.
- First-line paragraph indentation.
- Page numbers and a running header.
- A title page with author and contact details.

They could then create a separate profile called **"My Reading Copy"** without changing the manuscript itself.

### B. Export preview and quality assurance

These features help writers catch problems before sending their work to someone else.

| Feature | What it does |
|---|---|
| Live preview | Show how the export will look before generating the file. |
| Format-specific preview | Preview the PDF, EPUB, or HTML rendering independently. |
| Chapter navigation preview | Show the generated table of contents and chapter hierarchy. |
| Missing-content warnings | Identify empty chapters, omitted sections, or unexpected gaps. |
| Broken-link detection | Find links that won't work in the exported document. |
| Image validation | Detect missing images, oversized images, and missing alternative text. |
| Font substitution warnings | Identify fonts that might not be available or embeddable. |
| Overflow detection | Warn when content exceeds a fixed page or layout boundary. |
| Metadata validation | Flag missing or inconsistent book information. |
| EPUB compatibility report | Summarize validation errors and warnings. |
| Export comparison | Compare two generated versions to spot formatting or content differences. |

**My favorite feature here is an export preflight report.**

Before creating the final file, show a concise report such as:

- 42 chapters included.
- 3 chapters have no content.
- 1 image is missing.
- 2 internal links point to excluded chapters.
- EPUB metadata is complete.
- No critical structural errors found.

Warnings should distinguish actual export failures from optional improvements. A missing cover, for example, might be a warning rather than an error when exporting a draft.

### C. Professional book formatting

These features are particularly attractive for writers who want to prepare a book for publication or create polished reading copies.

| Feature | Description |
|---|---|
| Chapter-opening templates | Choose how each chapter begins. |
| Part title pages | Create dedicated pages for major sections of a book. |
| Front-matter templates | Add title pages, copyright pages, dedications, and epigraphs. |
| Back-matter templates | Add acknowledgments, author biographies, and other closing material. |
| Decorative scene breaks | Use symbols, ornaments, or custom images between scenes. |
| Running headers | Alternate chapter titles, book titles, or author names across pages. |
| Roman numeral pagination | Use Roman numerals for selected preliminary pages. |
| Custom page numbering | Restart numbering or omit numbers on selected pages. |
| Multiple typography themes | Offer serif, sans-serif, and other coordinated styles. |
| Print trim presets | Provide common paperback and hardcover dimensions. |
| Chapter-specific styles | Apply different layouts to prologues, epilogues, appendices, and other sections. |
| Image and illustration placement | Control image alignment, sizing, and captions. |

One useful design choice is to make front matter and back matter first-class elements of the project rather than treating them as ordinary chapters.

This makes it easier to apply different formatting and pagination rules to them.

### D. Publishing-oriented features

These are useful if you want your app to support writers beyond the drafting stage.

| Feature | Description |
|---|---|
| Publishing platform presets | Configure exports for specific distribution or publishing workflows. |
| Ebook cover validation | Check image dimensions, format, and other technical requirements. |
| ISBN management | Store and insert identifiers where appropriate. |
| Copyright page generator | Generate a page from project metadata and user-provided text. |
| Author biography templates | Reuse author information across multiple books. |
| Series metadata | Record series title, volume number, and related book information. |
| Multiple author identities | Maintain different pen names and associated metadata. |
| Publication metadata presets | Reuse publisher, language, and other metadata across projects. |
| Retailer-specific validation | Check a file against selected platform requirements. |
| Print-layout export profiles | Save configurations for different trim sizes and publishing workflows. |

I'd start with general EPUB validation and reusable metadata, then add retailer-specific presets if your users want to publish commercially.

Avoid claiming that an EPUB is ready for every retailer merely because it passes a generic EPUB validator. Platform requirements can differ.

### E. Advanced content control

These features give experienced writers more control over what leaves their writing environment.

| Feature | Description |
|---|---|
| Exclude internal notes | Keep research and planning notes out of the final document. |
| Exclude comments | Remove editorial comments from publication copies. |
| Exclude revision markers | Export the clean, accepted version of the text. |
| Include selected annotations | Include specific notes or annotations in a research export. |
| Find-and-replace rules | Apply optional transformations during export. |
| Smart punctuation | Convert straight quotes and other punctuation according to user preferences. |
| Custom scene-break rules | Convert scene markers into symbols or styled separators. |
| Custom section-break rules | Control how different section types are separated. |
| Footnotes and endnotes | Convert notes to the appropriate presentation for each format. |
| Cross-references | Maintain references to chapters, sections, and other internal content. |
| Glossary generation | Build a glossary from structured entries. |
| Character or location index | Generate indexes from explicitly tagged entities. |
| Multiple language support | Export multilingual content with appropriate language metadata and text direction. |

One caution: automatic punctuation conversion should be configurable. Smart quotes, dashes, and similar transformations are useful, but they should never silently damage intentionally formatted text or code snippets.

### F. Automation and workflow

These features become more valuable as writers work on multiple projects or export frequently.

| Feature | Description |
|---|---|
| One-click export | Repeat the last export with the same settings. |
| Batch export | Export multiple projects or stories in one operation. |
| Multi-format export | Generate PDF, EPUB, and HTML from a single action. |
| Automatic export | Generate files when a project is saved or a milestone is reached. |
| Export naming templates | Generate filenames from title, author, date, or version. |
| Export history | Record previous exports, timestamps, and settings. |
| Versioned exports | Preserve earlier versions rather than overwriting them. |
| Output folder templates | Organize exports by project, format, or date. |
| Command-line export | Allow scripted exports for advanced users. |
| API integration | Allow external tools to trigger exports. |
| Post-export actions | Open the file, run validation, or execute an approved workflow. |

A simple but powerful feature would be **Export All Formats**:

1. Select an export profile.
2. Click one button.
3. Generate PDF, EPUB, and HTML.
4. Validate each output.
5. Present the resulting files and any warnings.

This is especially helpful for writers who maintain a personal website alongside ebook and print editions.

### G. Accessibility and internationalization

These are easy to overlook but can make the export system more robust and inclusive.

| Feature | Description |
|---|---|
| Accessible PDF output | Preserve document structure and logical reading order. |
| Accessible EPUB metadata | Record supported accessibility features accurately. |
| Alternative text | Preserve image descriptions. |
| Language tagging | Identify the language of the document and, where appropriate, individual passages. |
| Right-to-left support | Support languages such as Arabic and Hebrew. |
| Unicode preservation | Retain international characters and typographic symbols. |
| Dyslexia-friendly presets | Offer optional typography and spacing preferences. |
| High-contrast themes | Offer visually distinct styles where applicable. |
| Screen-reader-friendly navigation | Preserve meaningful headings and navigation structure. |
| Accessibility validation | Identify common issues before export. |

For EPUB, accessibility should be part of the output design rather than an afterthought. The EPUB specifications include accessibility guidance, and the document structure can support more effective navigation and reading. [W3C](https://www.w3.org/TR/epub-33/?utm_source=chatgpt.com)

---

## 4. Recommended configuration interface

I'd avoid putting all these options on one screen. Instead, use a progressive interface that exposes the common settings first.

A possible layout:

```text
EXPORT PROJECT
────────────────────────────────────────

Format
  [ PDF ]  [ EPUB ]  [ HTML ]

Export
  (•) Entire project
  ( ) Selected chapters

Content
  ☑ Front matter
  ☑ Chapters
  ☑ Back matter
  ☑ Table of contents
  ☐ Notes and comments

Metadata
  Title:       [______________________]
  Author:      [______________________]
  Language:    [English              ▼]
  Description: [______________________]

Style
  Template:    [Reader-Friendly PDF  ▼]
  Font:        [Georgia              ▼]
  Font size:   [12 pt                ▼]

Format-specific settings
  [Configure PDF settings...]

Advanced settings
  [Show advanced options]

────────────────────────────────────────
  [Preview]                [Export]
```

The interface would change slightly depending on the chosen format.

For instance, selecting EPUB would expose the ebook cover, EPUB version, reflowable layout, and navigation options. Selecting PDF would expose page size, margins, pagination, and headers and footers.

The selected template should populate the relevant settings, but users should be able to override individual values without creating a new template unless they choose to save the changes.

### Suggested interaction details

- **Remember the last-used settings** for each project and format.
- **Show a preview** that updates when the user changes major presentation settings.
- **Explain unfamiliar options** using brief descriptions or tooltips.
- **Warn about incompatible settings** rather than silently ignoring them.
- **Restore template defaults** when the user wants to undo customizations.
- **Preserve user selections** when switching formats, but disable settings that don't apply.

For example, if someone switches from PDF to EPUB, don't erase their font preference just because page margins no longer apply.

---

## 5. Suggested implementation priorities

If I were building this feature, I'd approach it in phases.

| Priority | Features | Rationale |
|---|---|---|
| **Phase 1: Core export** | PDF, EPUB, HTML; content selection; metadata; basic typography; file naming; basic validation. | Delivers the primary functionality. |
| **Phase 2: Useful presets** | Manuscript PDF, reading-copy PDF, reflowable EPUB, clean HTML; reusable profiles; cover and contents support. | Makes export useful without extensive configuration. |
| **Phase 3: Professional output** | PDF bookmarks, headers and footers, chapter-opening controls, accessible output, EPUB validation, advanced metadata. | Improves quality and publishing readiness. |
| **Phase 4: Advanced workflows** | Batch export, export history, automated exports, custom templates, command-line support. | Benefits power users and larger projects. |
| **Phase 5: Publishing tools** | Retailer-specific presets, print trim templates, richer front matter, series metadata, advanced validation. | Extends the app into the publishing workflow. |

I would prioritize reliable output and a good set of presets over an enormous collection of formatting options.

A writer should be able to produce a good-looking file without needing to understand page layout, HTML, CSS, or ebook packaging.

---

## 6. Three architectural decisions worth making early

These decisions will affect how easy it is to add new export features later.

### A. Use a format-neutral document model

Represent the manuscript as structured content rather than as a collection of preformatted strings.

For example:

```text
Project
 ├── Metadata
 ├── Front Matter
 ├── Part
 │    ├── Chapter
 │    │    ├── Paragraph
 │    │    ├── Paragraph
 │    │    └── Scene Break
 │    └── Chapter
 └── Back Matter
```

Text elements can have semantic types such as:

- Chapter heading
- Section heading
- Body paragraph
- Block quotation
- Footnote
- Scene break
- Image
- Caption
- List

The PDF renderer can turn these elements into page layouts, the EPUB renderer can turn them into reflowable XHTML, and the HTML renderer can generate semantic web content.

This also makes it easier to add formats later without redesigning the writing engine.

### B. Separate content, styling, and format-specific rules

I would divide the export configuration into three layers.

**Content settings**
- Which chapters and sections to include.
- Whether to include notes or comments.
- How to handle front matter and back matter.

**Style settings**
- Typography.
- Heading styles.
- Paragraph formatting.
- Scene-break appearance.
- Colors and decorative elements.

**Format settings**
- PDF page size and pagination.
- EPUB version, navigation, and packaging.
- HTML document structure, stylesheet handling, and resource packaging.

This separation prevents PDF-specific properties from leaking into the EPUB and HTML configuration.

It also allows a single style profile to be reused across formats wherever the settings are meaningful.

### C. Treat validation as part of the export pipeline

A reliable export pipeline might look like this:

1. Load the project and metadata.
2. Resolve the selected chapters and sections.
3. Build a normalized document structure.
4. Apply content transformations.
5. Apply the selected style profile.
6. Render the target format.
7. Validate the resulting file.
8. Report errors and warnings.
9. Save the output.

For EPUB, validation should include structural and packaging checks. For PDF, it should include checks appropriate to page rendering and any accessibility requirements. For HTML, it should include document structure, resource references, and link handling.

This approach makes it easier to diagnose problems and to add new output formats in the future.

---

## 7. My final recommendation: the 15 features I'd build first

If you want a concrete starting point, these would be my first 15 features, in roughly this order:

1. **PDF, EPUB, and HTML format selection**
2. **Entire-project and selected-chapter export**
3. **Title, author, language, and description metadata**
4. **Front-matter and back-matter inclusion**
5. **Automatic table of contents**
6. **Preservation of headings, paragraphs, emphasis, and scene breaks**
7. **Basic typography settings**
8. **PDF page size, margins, and page numbering**
9. **PDF font embedding and bookmarks**
10. **EPUB cover image and navigable contents**
11. **Standalone HTML and packaged HTML export**
12. **Built-in export profiles**
13. **Export preview**
14. **Format validation and useful error messages**
15. **Remembered settings and one-click re-export**

I'd add accessibility support early in the architecture, even if some of its more advanced configuration options come later.

**One feature I'd consider essential to the overall experience is the ability to export the same manuscript in multiple formats using a single saved profile.** It reduces repetitive work, encourages consistent output, and gives your app a strong foundation for future publishing features.

For reference, the current [W3C EPUB 3.3 specification](https://www.w3.org/TR/epub-33/) is a useful technical baseline for implementing EPUB export correctly.