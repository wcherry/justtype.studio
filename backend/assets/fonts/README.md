# Bundled typefaces

Liberation Serif and Liberation Sans (version 2), in regular, italic, bold and
bold italic. They are metric-compatible with Times New Roman and Arial, and
licensed under the SIL Open Font License 1.1: see `LICENSE.txt`.

They are compiled into the server (`include_bytes!` in `src/export/font.rs`)
and embedded, subset to the glyphs a book uses, in book-layout PDF exports. A
document made with them is not itself bound by the OFL.
