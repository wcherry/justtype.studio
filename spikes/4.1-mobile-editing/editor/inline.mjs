/**
 * Fold the built JS into the HTML as a classic script.
 *
 * WKWebView will not load a `type="module"` script over `file://` — a file
 * URL has an opaque origin and the module fetch is a cross-origin one, so the
 * page comes up blank with nothing in the console to say why. The bundle has
 * no top-level imports or exports left after rollup, so it runs perfectly well
 * as a plain script. One file also means one thing to copy into the app.
 */
import { readdirSync, readFileSync, writeFileSync, rmSync } from 'fs'
import { join } from 'path'

const dist = new URL('./dist/', import.meta.url).pathname
const assets = join(dist, 'assets')
const script = readdirSync(assets).find((name) => name.endsWith('.js'))
const code = readFileSync(join(assets, script), 'utf8')

// The module tag goes, and the code lands at the end of the body instead.
// A module is deferred by definition; an inline classic script is not, so
// leaving it in the head would run the editor before `#editor` exists — which
// is a null and a thrown constructor, not an error message.
const wrapped =
  '  <script>\ntry {\n' +
  code +
  '\n} catch (e) { window.__error = (e && (e.stack || e.message)) || String(e) }\n</script>\n  </body>'

// The replacement is a *function*, not a string. A string replacement
// interprets `$&`, `` $` `` and friends, and minified JavaScript is full of
// dollars and backticks: passing the bundle as a string silently ate a
// template literal's opening backtick a hundred kilobytes in, and the only
// symptom was a blank page and `SyntaxError: Unexpected identifier`.
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace(/<script type="module"[^>]*><\/script>/, '')
  .replace('</body>', () => wrapped)
writeFileSync(join(dist, 'index.html'), html)
rmSync(assets, { recursive: true, force: true })

const kb = (n) => `${(n / 1024).toFixed(0)} KB`
console.log(`inlined ${script} — index.html is now ${kb(Buffer.byteLength(html))}`)
