import Typography from '@tiptap/extension-typography'

/**
 * Typographic input rules, narrowed to the ones prose wants.
 *
 * The extension ships a lot of rules aimed at technical writing — fractions,
 * ©, ™, ≠, ± — which are wrong here in different ways. `1/2` becoming ½ is
 * actively unwanted in fiction, where a character might reasonably say "half
 * past" or a manuscript might quote a measurement, and none of the symbol
 * substitutions belong in dialogue.
 *
 * What stays is the punctuation a typesetter would fix anyway: curly quotes,
 * an em dash for `--`, an ellipsis for `...`.
 */
export const ProseTypography = Typography.configure({
  oneHalf: false,
  oneQuarter: false,
  threeQuarters: false,
  plusMinus: false,
  notEqual: false,
  laquo: false,
  raquo: false,
  multiplication: false,
  superscriptTwo: false,
  superscriptThree: false,
  copyright: false,
  registeredTrademark: false,
  trademark: false,
  servicemark: false,
  // Left on: emDash, ellipsis, openDoubleQuote, closeDoubleQuote,
  // openSingleQuote, closeSingleQuote, leftArrow, rightArrow.
})
