package studio.justtype.core.sync

import java.io.File

/**
 * Write to a neighbour and rename. A half-written scene is worse than no scene: the rename is the
 * only atomic step a filesystem offers.
 */
internal fun File.writeAtomically(text: String) {
    parentFile?.mkdirs()
    val temporary = File(parentFile, "$name.writing")
    try {
        temporary.writeText(text)
        if (!temporary.renameTo(this)) {
            // Some filesystems refuse to rename over an existing file.
            delete()
            if (!temporary.renameTo(this)) throw java.io.IOException("could not replace $name")
        }
    } catch (e: Exception) {
        temporary.delete()
    }
}

/** Ids come from the server, but a file path is not the place to find out they were wrong. */
internal fun safe(id: String) = id.map { if (it.isLetterOrDigit() || it == '-' || it == '_') it else '-' }.joinToString("")
