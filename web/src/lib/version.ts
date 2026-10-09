/**
 * The running build's identity, for the UI to show and for bug reports to quote.
 *
 * Both values come from `web/version.txt` and are inlined at build time by
 * `vite.config.ts` (`define`): the production build is static files served by
 * the Rust binary, with nothing left to read an environment at runtime.
 *
 * Only an image build has a version — that file is written by the Dockerfile —
 * so **every value here can be empty**: a local build has a commit but no
 * version, and a build from a tarball with no git has neither. Callers show
 * what is known and omit the rest; nothing here invents a placeholder version.
 */

/** The released version, e.g. `0.1.42`. Empty outside an image build. */
export const APP_VERSION: string = __APP_VERSION__

/** The commit the bundle was built from (short SHA). Empty if unknown. */
export const BUILD_ID: string = __BUILD_ID__

/** `v0.1.42` for a release, the bare commit for a build that has one but no version. */
export const VERSION_LABEL = APP_VERSION ? `v${APP_VERSION}` : BUILD_ID

/** The long form, for copying into a bug report: `v0.1.42 (a1b2c3d)`. */
export const FULL_VERSION_LABEL = [APP_VERSION && `v${APP_VERSION}`, BUILD_ID && `(${BUILD_ID})`]
  .filter(Boolean)
  .join(' ')
