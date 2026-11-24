package io.github.astartes_gaming.emuera

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.text.InputType
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.Toast
import java.io.File

private const val EXPORT = 11
private const val IMPORT = 12

/**
 * The running game's saves, as emuera-tty's save manager has them (its crate::savemgr does the work): per save copy
 * or move to a slot, export, delete; for all of them back up, restore a backup, import a file. A save deleted or
 * overwritten here is kept in the backups first. The activity passes its onActivityResult on, for the file picker.
 */
class Saves(private val a: Activity) {
    private class Save(val slot: Int?, val path: String, val time: String, val comment: String) {
        val name get() = if (slot == null) "global" else "slot %02d".format(slot)
    }

    /** the save being exported, while the picker is open */
    private var exporting: Save? = null

    private fun failed(r: String) = r.startsWith('\u0001')
    private fun say(text: String) = Toast.makeText(a, text, Toast.LENGTH_LONG).show()
    private fun files(n: String) = if (n == "1") "1 file" else "$n files"

    private fun list(): List<Save>? {
        val r = Native.saveList()
        if (failed(r)) {
            say(r.drop(1))
            return null
        }
        return r.lines().filter { it.isNotEmpty() }.map { it.split('\t').let { f -> Save(f[0].toIntOrNull(), f[1], f[2], f[3]) } }
    }

    private fun freeSlot(saves: List<Save>) = (0..999).first { n -> saves.none { it.slot == n } }

    fun show() {
        val saves = list() ?: return
        val rows = mutableListOf<Pair<String, () -> Unit>>()
        for (s in saves) rows += "${s.name}   ${s.time}\n${s.comment}" to { actions(s, saves) }
        if (saves.isEmpty()) rows += "(no saves yet)" to { show() }
        rows += "Back up all saves now" to {
            background({ Native.saveBackup() }) { r -> say(if (failed(r)) r.drop(1) else "Backed up ${files(r)}") }
        }
        rows += "Restore a backup…" to { restore() }
        rows += "Import a save file…" to {
            a.startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*"), IMPORT)
        }
        menu("Saves", rows)
    }

    private fun actions(s: Save, saves: List<Save>) {
        val rows = mutableListOf<Pair<String, () -> Unit>>()
        // (global.sav holds what every save shares: it doesn't go into a slot)
        if (s.slot != null) {
            rows += "Copy to slot…" to { toSlot(s, saves, false) }
            rows += "Move to slot…" to { toSlot(s, saves, true) }
        }
        rows += "Export to a file…" to {
            exporting = s
            a.startActivityForResult(
                Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("application/octet-stream")
                    .putExtra(Intent.EXTRA_TITLE, File(s.path).name),
                EXPORT
            )
        }
        rows += "Delete" to {
            confirm("Delete ${s.name}?", "A copy is kept in the backups.", "Delete") { done(Native.saveDelete(s.path), "Deleted ${s.name}") }
        }
        menu("${s.name}: ${s.comment}", rows)
    }

    private fun toSlot(s: Save, saves: List<Save>, moving: Boolean) {
        askSlot("${if (moving) "Move" else "Copy"} ${s.name} to slot", freeSlot(saves)) { n ->
            val go = { done(Native.saveCopy(s.path, n, moving), "%s %s to slot %02d".format(if (moving) "Moved" else "Copied", s.name, n)) }
            if (saves.any { it.slot == n }) confirm("Slot %02d has a save. Overwrite it?".format(n), "The one there is kept in the backups.", "Overwrite", go) else go()
        }
    }

    private fun restore() {
        val r = Native.saveBackups()
        if (failed(r)) return say(r.drop(1))
        val backups = r.lines().filter { it.isNotEmpty() }.map { it.split('\t') }
        if (backups.isEmpty()) {
            say("No backups yet")
            return show()
        }
        menu("Restore a backup", backups.map { (path, n) ->
            val name = File(path).name
            "$name   (${files(n)})" to {
                confirm("Replace the saves with $name?", "The ones there now are backed up first.", "Restore") {
                    background({ Native.saveRestore(path) }) { x -> say(if (failed(x)) x.drop(1) else "Restored ${files(x)}: load one from the game's load menu") }
                }
            }
        })
    }

    /** The file picker's answer, for export and import. */
    fun onResult(request: Int, result: Int, data: Intent?) {
        if (request != EXPORT && request != IMPORT) return
        val uri = data?.data
        if (result != Activity.RESULT_OK || uri == null) return show()
        if (request == EXPORT) {
            val s = exporting ?: return
            exporting = null
            val r = runCatching { a.contentResolver.openOutputStream(uri)!!.use { out -> File(s.path).inputStream().use { it.copyTo(out) } } }
            say(r.exceptionOrNull()?.let { "Couldn't export ${s.name}: ${it.message}" } ?: "Exported ${s.name}")
            return show()
        }
        val name = displayName(uri) ?: "the file"
        val tmp = File(a.cacheDir, "import.sav")
        val read = runCatching { a.contentResolver.openInputStream(uri)!!.use { inp -> tmp.outputStream().use { inp.copyTo(it) } } }
        if (read.isFailure) {
            say("Couldn't read $name: ${read.exceptionOrNull()?.message}")
            return show()
        }
        val into = { slot: Int ->
            val r = Native.saveImport(tmp.path, slot)
            tmp.delete()
            done(r, "Imported $name")
        }
        // (as emuera-tty's save manager: a file named global goes in as global.sav)
        if (name.contains("global", ignoreCase = true)) into(-1) else askSlot("Import $name into slot", freeSlot(list() ?: return)) { into(it) }
    }

    private fun displayName(uri: Uri): String? =
        a.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c -> if (c.moveToFirst()) c.getString(0) else null }

    private fun done(error: String, ok: String) {
        say(error.ifEmpty { ok })
        show()
    }

    /** Copying every save can take a while: off the UI thread, then `then` and the list again. */
    private fun background(work: () -> String, then: (String) -> Unit) {
        say("Working…")
        Thread {
            val r = work()
            a.runOnUiThread {
                then(r)
                show()
            }
        }.start()
    }

    private fun menu(title: String, rows: List<Pair<String, () -> Unit>>) {
        AlertDialog.Builder(a)
            .setTitle(title)
            .setItems(rows.map { it.first }.toTypedArray()) { _, i -> rows[i].second() }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun confirm(title: String, message: String, yes: String, then: () -> Unit) {
        AlertDialog.Builder(a)
            .setTitle(title)
            .setMessage(message)
            .setPositiveButton(yes) { _, _ -> then() }
            .setNegativeButton("Cancel") { _, _ -> show() }
            .show()
    }

    private fun askSlot(title: String, suggested: Int, then: (Int) -> Unit) {
        val field = EditText(a).apply {
            inputType = InputType.TYPE_CLASS_NUMBER
            setText(suggested.toString())
            contentDescription = "Slot"
        }
        val pad = (20 * a.resources.displayMetrics.density).toInt()
        AlertDialog.Builder(a)
            .setTitle(title)
            .setView(FrameLayout(a).apply {
                setPadding(pad, 0, pad, 0)
                addView(field)
            })
            .setPositiveButton("OK") { _, _ ->
                val n = field.text.toString().trim().toIntOrNull()
                if (n == null || n !in 0..999) {
                    say("A slot is a number from 0 to 999")
                    show()
                } else {
                    then(n)
                }
            }
            .setNegativeButton("Cancel") { _, _ -> show() }
            .show()
    }
}
