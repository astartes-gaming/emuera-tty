package io.github.astartes_gaming.emuera

import android.database.Cursor
import android.database.MatrixCursor
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.provider.DocumentsContract.Root
import android.provider.DocumentsProvider
import android.webkit.MimeTypeMap
import java.io.File
import java.io.FileNotFoundException

private val ROOT_COLUMNS = arrayOf(Root.COLUMN_ROOT_ID, Root.COLUMN_TITLE, Root.COLUMN_DOCUMENT_ID, Root.COLUMN_FLAGS, Root.COLUMN_ICON)
private val DOCUMENT_COLUMNS = arrayOf(Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME, Document.COLUMN_MIME_TYPE, Document.COLUMN_SIZE, Document.COLUMN_LAST_MODIFIED, Document.COLUMN_FLAGS)

/**
 * The games in the system's file manager, for moving saves to and from other devices (Emuera
 * reads them as they are), or adding to a game. A document's id is its path under the games.
 */
class GamesProvider : DocumentsProvider() {
    private val root by lazy { File(context!!.filesDir, "games").apply { mkdirs() }.canonicalFile }
    private val authority by lazy { context!!.packageName + ".games" }

    override fun onCreate() = true

    private fun idOf(f: File) = if (f == root) "." else f.toRelativeString(root)

    private fun existing(id: String) = inside(root, id).also { if (!it.exists()) throw FileNotFoundException(id) }

    private fun changed(dir: File) = context!!.contentResolver.notifyChange(DocumentsContract.buildChildDocumentsUri(authority, idOf(dir)), null)

    private fun add(c: MatrixCursor, f: File) {
        var flags = if (f.isDirectory) Document.FLAG_DIR_SUPPORTS_CREATE else Document.FLAG_SUPPORTS_WRITE
        if (f != root) flags = flags or Document.FLAG_SUPPORTS_DELETE or Document.FLAG_SUPPORTS_RENAME
        val type = if (f.isDirectory) Document.MIME_TYPE_DIR else MimeTypeMap.getSingleton().getMimeTypeFromExtension(f.extension.lowercase()) ?: "application/octet-stream"
        val name = if (f == root) "Emuera-Android" else f.name
        c.newRow().add(Document.COLUMN_DOCUMENT_ID, idOf(f)).add(Document.COLUMN_DISPLAY_NAME, name).add(Document.COLUMN_MIME_TYPE, type)
            .add(Document.COLUMN_SIZE, f.length()).add(Document.COLUMN_LAST_MODIFIED, f.lastModified()).add(Document.COLUMN_FLAGS, flags)
    }

    override fun queryRoots(projection: Array<String>?): Cursor {
        val c = MatrixCursor(projection ?: ROOT_COLUMNS)
        c.newRow().add(Root.COLUMN_ROOT_ID, "games").add(Root.COLUMN_TITLE, "Emuera-Android").add(Root.COLUMN_DOCUMENT_ID, idOf(root))
            .add(Root.COLUMN_FLAGS, Root.FLAG_SUPPORTS_CREATE or Root.FLAG_LOCAL_ONLY).add(Root.COLUMN_ICON, android.R.drawable.ic_menu_agenda)
        return c
    }

    override fun queryDocument(id: String, projection: Array<String>?): Cursor {
        val c = MatrixCursor(projection ?: DOCUMENT_COLUMNS)
        add(c, existing(id))
        return c
    }

    override fun queryChildDocuments(parent: String, projection: Array<String>?, sortOrder: String?): Cursor {
        val c = MatrixCursor(projection ?: DOCUMENT_COLUMNS)
        val dir = existing(parent)
        // (a dot marks an import in progress or left over, and the engine's temporary files)
        dir.listFiles()?.filter { !it.name.startsWith(".") }?.forEach { add(c, it) }
        c.setNotificationUri(context!!.contentResolver, DocumentsContract.buildChildDocumentsUri(authority, idOf(dir)))
        return c
    }

    override fun openDocument(id: String, mode: String, signal: CancellationSignal?): ParcelFileDescriptor =
        ParcelFileDescriptor.open(existing(id), ParcelFileDescriptor.parseMode(mode))

    /** `name`, or as the file manager expects when it's taken, `name (1)`. */
    private fun free(dir: File, name: String): File {
        val f = inside(dir, name)
        if (f.parentFile != dir) throw FileNotFoundException(name)
        if (!f.exists()) return f
        val base = f.nameWithoutExtension
        val ext = if (f.extension.isEmpty()) "" else "." + f.extension
        return generateSequence(1) { it + 1 }.map { File(dir, "$base ($it)$ext") }.first { !it.exists() }
    }

    override fun createDocument(parent: String, type: String, name: String): String {
        val dir = existing(parent)
        val f = free(dir, name)
        if (!(if (type == Document.MIME_TYPE_DIR) f.mkdir() else f.createNewFile())) throw IllegalStateException("can't create $name")
        changed(dir)
        return idOf(f)
    }

    override fun deleteDocument(id: String) {
        val f = existing(id)
        if (f == root) throw IllegalArgumentException("not the games themselves")
        if (!f.deleteRecursively()) throw IllegalStateException("can't delete ${f.name}")
        revokeDocumentPermission(id)
        changed(f.parentFile!!)
    }

    override fun renameDocument(id: String, name: String): String {
        val f = existing(id)
        if (f == root) throw IllegalArgumentException("not the games themselves")
        val dir = f.parentFile!!
        val to = free(dir, name)
        if (!f.renameTo(to)) throw IllegalStateException("can't rename ${f.name}")
        revokeDocumentPermission(id)
        changed(dir)
        return idOf(to)
    }
}
