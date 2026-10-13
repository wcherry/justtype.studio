import { zip } from './zip'

/**
 * A Scrivener project is a folder (`My Novel.scriv`), and a browser can only
 * upload files. These gather the parts of one that an import reads — the
 * binder, and each document's text and synopsis — into a zip the server
 * takes like any other project. Snapshots, media and settings stay behind,
 * which keeps the upload small.
 */

/** The binder, then Scrivener 3's and Scrivener 2's text and synopses. */
const WANTED = [
  /^[^/]+\/[^/]+\.scrivx$/i,
  /^[^/]+\/Files\/Data\/[^/]+\/(content\.rtf|synopsis\.txt)$/i,
  /^[^/]+\/Files\/Docs\/\d+(\.rtf|_synopsis\.txt)$/i,
]

const wanted = (path: string) => WANTED.some((re) => re.test(path))

export const isScrivenerFolder = (name: string) => /\.scriv$/i.test(name)

async function pack(folder: string, files: { path: string; file: File }[]): Promise<File> {
  if (!isScrivenerFolder(folder)) {
    throw new Error('That folder is not a Scrivener project — choose the one whose name ends in .scriv.')
  }
  const chosen = files.filter((f) => wanted(f.path))
  if (!chosen.some((f) => /\.scrivx$/i.test(f.path))) {
    throw new Error('That .scriv folder has no binder (.scrivx) in it.')
  }
  const entries = await Promise.all(
    chosen.map(async (f) => ({ path: f.path, data: new Uint8Array(await f.file.arrayBuffer()) })),
  )
  return new File([zip(entries)], `${folder}.zip`, { type: 'application/zip' })
}

/** From `<input webkitdirectory>`, whose files carry their path in the folder. */
export function fromFolderInput(list: FileList): Promise<File> {
  const files = Array.from(list).map((file) => ({ path: file.webkitRelativePath, file }))
  const folder = files[0]?.path.split('/')[0] ?? ''
  return pack(folder, files)
}

function readAll(dir: FileSystemDirectoryEntry): Promise<FileSystemEntry[]> {
  const reader = dir.createReader()
  const all: FileSystemEntry[] = []
  return new Promise((resolve, reject) => {
    // Each call returns a batch; an empty one means there are no more.
    const next = () =>
      reader.readEntries((batch) => {
        if (batch.length === 0) resolve(all)
        else {
          all.push(...batch)
          next()
        }
      }, reject)
    next()
  })
}

const fileOf = (entry: FileSystemFileEntry) =>
  new Promise<File>((resolve, reject) => entry.file(resolve, reject))

/** From a folder dropped on the page. */
export async function fromDroppedFolder(root: FileSystemDirectoryEntry): Promise<File> {
  const files: { path: string; file: File }[] = []
  const walk = async (dir: FileSystemDirectoryEntry, prefix: string) => {
    for (const entry of await readAll(dir)) {
      const path = `${prefix}/${entry.name}`
      if (entry.isDirectory) await walk(entry as FileSystemDirectoryEntry, path)
      else if (wanted(path)) files.push({ path, file: await fileOf(entry as FileSystemFileEntry) })
    }
  }
  if (isScrivenerFolder(root.name)) await walk(root, root.name)
  return pack(root.name, files)
}
