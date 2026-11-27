package io.github.astartes_gaming.emuera

import android.app.Activity
import android.app.ActivityManager
import android.app.AlertDialog
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.text.method.LinkMovementMethod
import java.io.File
import java.io.IOException
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

import kotlin.random.Random
import kotlin.text.lowercase

private const val PICK_ARCHIVE = 1
private const val PICK_FOLDER = 2
private const val PICK_FONT = 3
private const val PREFS_ICON = "emuera_tty_icons"
private const val KEY_ICON = "alternate_icon"

private val iconJokes = mutableListOf<Triple<String, String, Int>>().apply {
    add(Triple("Default", "Egger", 20))
    add(Triple("Akumaid", "Coldbane", 10))
}

/** The game list, and importing games through the system's file picker (no storage permission needed). */
class MainActivity : Activity() {
    private lateinit var list: LinearLayout
    private lateinit var status: TextView
    private lateinit var spinner: ProgressBar
    private lateinit var importButtons: List<Button>
    private val games by lazy { File(filesDir, "games").apply { mkdirs() } }
    private var importing = false
    /** Game directory for the font picker. */
    private var fontGameDir: File? = null

    private fun gameIntent() = Intent(this, GameActivity::class.java).putExtra("tablet", resources.configuration.smallestScreenWidthDp >= 600)

    override fun onCreate(state: Bundle?) {
        setTheme(android.R.style.Theme_Material_NoActionBar)
        super.onCreate(state)
        // so the engine's config file can be read and written from this (main) process:
        // the game process sets these in Native.start, but the list process needs them too.
        Native.setConfigHome(filesDir.path)
        Native.recoverImports(games.path)
        applyIcon()

        val pad = (16 * resources.displayMetrics.density).toInt()
        list = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        status = TextView(this).apply { setPadding(0, pad / 2, 0, pad / 2) }
        spinner = ProgressBar(this).apply { visibility = ProgressBar.GONE }
        val zip = Button(this).apply {
            text = "Import a ZIP/TAR archive"
            isAllCaps = false
            setOnClickListener { pickArchive() }
        }
        val folder = Button(this).apply {
            text = "Import a folder"
            isAllCaps = false
            setOnClickListener { pickFolder() }
        }
        importButtons = listOf(zip, folder)
        val buttons = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            addView(zip, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(folder, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad)
            padForSystemBars(pad)
            addView(TextView(context).apply {
                text = "Emuera - choose a game"
                textSize = 24f
                setPadding(0, 0, 0, pad)
            })
            addView(ScrollView(context).apply { addView(list) }, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(spinner)
            addView(status)
            addView(buttons)
        }
        setContentView(root)
    }

    override fun onResume() {
        super.onResume()
        if (::list.isInitialized) {
            refresh()
            reportDeadGame()
            restartGame()
        }
    }

    private fun gameRunning() = getSystemService(ActivityManager::class.java).runningAppProcesses?.any { it.processName.endsWith(":game") } == true

    /** A game that died without a word, like a native crash: this is the first chance to say so. */
    private fun reportDeadGame() {
        val marker = gameMarker(this)
        if (!marker.exists() || gameRunning()) return
        marker.delete()
        showEngineLog("The last game stopped unexpectedly", File(filesDir, "engine.log"))
    }

    /** A game's menu restarted it: start it again once its old process is gone, as a process starts one game only. */
    private fun restartGame(tries: Int = 100) {
        val marker = restartMarker(this)
        if (!marker.exists()) return
        if (gameRunning() && tries > 0) {
            list.postDelayed({ restartGame(tries - 1) }, 50)
            return
        }
        val name = marker.readText()
        marker.delete()
        startActivity(gameIntent().putExtra("game", name))
    }

    private fun refresh() {
        val pad = (16 * resources.displayMetrics.density).toInt()
        list.removeAllViews()
        // (a dot marks an import in progress or left over)
        val names = games.list()?.filter { !it.startsWith(".") }?.sorted() ?: emptyList()
        list.addView(TextView(this).apply {
        	text = """
        	Emuera for Android. Uses emuera-tty internally to run Era games.
        	Also the unofficial successor of uEmuera. (https://github.com/xerysherry/uEmuera)

        	Check out the project at https://github.com/astartes-gaming/emuera-tty for updates.

        	Currently in rapid development; the ERB interpreter/engine is fine, but some features from emuera-tty itself aren't fully exposed through the app yet.
        	""".trimIndent()
        	textSize = 12f
        	movementMethod = LinkMovementMethod.getInstance()
        	setPadding(0, 0, 0, pad/2)
        })
        list.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(Button(this@MainActivity).apply {
                text = "Settings"
                isAllCaps = false
                setOnClickListener { startActivity(Intent(this@MainActivity, SettingsActivity::class.java)) }
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(TextView(this@MainActivity).apply {
                text = "the engine's own settings, shared with the desktop program"
                textSize = 12f
                setTextColor(0xFF999999.toInt())
                setPadding(8, 0, 0, 0)
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))

        list.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(Button(this@MainActivity).apply {
                text = "App icon"
                isAllCaps = false
                setOnClickListener { showIconPicker() }
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(TextView(this@MainActivity).apply {
                text = "choose a different launcher icon"
                textSize = 12f
                setTextColor(0xFF999999.toInt())
                setPadding(8, 0, 0, 0)
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))

        if (names.isEmpty()) {
            list.addView(TextView(this).apply {
                text = """
                No games yet.
                Import a zip or tar of a game, or a folder holding its CSV and ERB folders. 7z and rar files need unpacking first: import the unpacked folder.

                Importing makes a COPY of the selected folder (or zip) stored internally in this app's data directory; the original folder/zip remains untouched.

                Large games may take a while to import (around 30-60 seconds); don't close the app while it's doing so.
                """.trimIndent()
                textSize = 16f
                setPadding(0, 0, 0, pad/2)
            })
        }
        for (name in names) {
            val gameDir = File(games, name)
            // the game's real title, when it has one: shown as "Title (folder)"
            val title = Native.gameTitle(gameDir.path).takeIf { it.isNotBlank() }
            val label = if (title != null && title != name) "$title ($name)" else name
            list.addView(LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                setBackground(gameRowOutline())
                setPadding(pad, pad, pad, pad)
                // top row: the long game button, then the short delete button
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER_VERTICAL
                    addView(Button(context).apply {
                        text = label
                        isAllCaps = false
                        // (an import may be replacing this game's folder)
                        isEnabled = !importing
                        setOnClickListener { startActivity(gameIntent().putExtra("game", name)) }
                    }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                    addView(iconButton("✕", "Delete $name") { confirmDelete(name) })
                })
                // everything else below
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER_VERTICAL
                    addView(iconButton("⚙", "Settings for $name") { showGameSettings(gameDir) {} })
                    addView(iconButton("𝑓", "Font for $name") { pickFont(gameDir) })
                    addView(iconButton("𝑓✕", "Unset font for $name") { unsetFont(gameDir) })
                })
            })
        }
        list.addView(TextView(this).apply {
            text = """
            For the curious: Why not Termux?

            While Termux is a supported build option, it comes with the most significant limitation of not having a implemented graphics protocol, which means that there is basically no image rendering support.
            """.trimIndent()
            textSize = 12f
            setPadding(0, pad/2, 0, pad/2)
        })
    }

    /** A one-symbol button, as big as a finger, named for screen readers (and the test scripts). */
    private fun iconButton(symbol: String, description: String, click: () -> Unit) = Button(this).apply {
        text = symbol
        // (at the button's own size the gear reads as a sun)
        textSize = 22f
        contentDescription = description
        // (an import may be replacing this game's folder)
        isEnabled = !importing
        minWidth = 0
        minimumWidth = (48 * resources.displayMetrics.density).toInt()
        setOnClickListener { click() }
    }

    /** A gray outline around each added game row, similar to the app's background. */
    private fun gameRowOutline(): android.graphics.drawable.GradientDrawable {
        val d = android.graphics.drawable.GradientDrawable()
        d.setShape(android.graphics.drawable.GradientDrawable.RECTANGLE)
        d.setStroke(2, 0xFF777777.toInt())
        d.setColor(0x00000000.toInt())
        return d
    }

    /** True with a 1-in-`n` chance. */
    private fun oneIn(n: Int): Boolean {
        require(n > 0) { "n must be positive" }
        return Random.nextInt(n) == 0
    }

    /** Enable exactly one launcher-activity alias, so there's always one app icon, and persist the choice. */
    private fun applyIcon() {
        val pm = packageManager
        val pref = getSharedPreferences(PREFS_ICON, MODE_PRIVATE)
        val current = pref.getString(KEY_ICON, "default")
        for (icon in appIcons) {
            pm.setComponentEnabledSetting(
                ComponentName(packageName, "io.github.astartes_gaming.emuera.Icon${icon.key.capitalize()}"),
                if (icon.key == current) PackageManager.COMPONENT_ENABLED_STATE_ENABLED
                else PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
                PackageManager.DONT_KILL_APP)
        }
    }

    /** Let the user pick which launcher icon to show; applies at once and survives restarts.
        Each row shows the icon image itself. Each option can carry joke replacements
        (see iconJokes): with a 1-in-`chance` roll, its label swaps to the joke text for
        half a second before reverting. */
    private fun showIconPicker() {
        val pref = getSharedPreferences(PREFS_ICON, MODE_PRIVATE)
        val current = pref.getString(KEY_ICON, "default")
        val selected = appIcons.indexOfFirst { it.key == current }.coerceAtLeast(0)

        val displayLabels = appIcons.map { it.label }.toMutableList()
        val joke = mutableListOf<Int>()
        for (i in displayLabels.indices) {
            for ((original, replacement, chance) in iconJokes) {
                if (displayLabels[i] == original && oneIn(chance)) {
                    displayLabels[i] = replacement
                    joke += i
                    break
                }
            }
        }

        val adapter = IconAdapter(appIcons, displayLabels)
        val dialog = AlertDialog.Builder(this)
            .setTitle("App icon")
            .setSingleChoiceItems(adapter, selected) { _, picked ->
                val chosen = appIcons[picked].key
                pref.edit().putString(KEY_ICON, chosen).apply()
                applyIcon()
                status.text = "App icon: ${appIcons[picked].label}"
            }
            .setPositiveButton("OK", null)
            .show()

        // revert the joke labels after half a second, if the dialog is still open
        if (joke.isNotEmpty()) {
            Handler(Looper.getMainLooper()).postDelayed({
                if (!dialog.isShowing) return@postDelayed
                for (i in joke) displayLabels[i] = appIcons[i].label
                adapter.notifyDataSetChanged()
            }, 500)
        }
    }

    /** One picker row: the icon image next to its label. */
    private inner class IconAdapter(
        private val icons: List<AppIcon>,
        private val displayLabels: MutableList<String>,
    ) : ArrayAdapter<AppIcon>(this@MainActivity, 0, 0, icons) {
        override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
            val row = if (convertView != null) convertView else buildRow(parent)
            val (image, text) = row.tag as Pair<ImageView, TextView>
            val icon = icons[position]
            val resId = resources.getIdentifier(icon.iconName, "drawable", packageName)
            image.setImageResource(if (resId == 0) 0 else resId)
            text.text = displayLabels[position]
            return row
        }
    }

    /** Builds a reusable row view: an icon image followed by its label text. */
    private fun buildRow(parent: ViewGroup): LinearLayout {
        val density = resources.displayMetrics.density
        val size = (48 * density).toInt()
        val row = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(14, 10, 14, 10)
        }
        val image = ImageView(this@MainActivity).apply { id = View.generateViewId() }
        row.addView(image, LinearLayout.LayoutParams(size, size))
        val text = TextView(this@MainActivity).apply {
            id = View.generateViewId()
            textSize = 16f
        }
        row.addView(text, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        row.tag = Pair(image, text)
        return row
    }

    private fun confirmDelete(name: String) {
        AlertDialog.Builder(this)
            .setTitle("Delete $name?")
            .setMessage("This removes the game and all of its saves from the app. The original folder/ZIP will not be removed or modified.")
            .setPositiveButton("Delete") { _, _ ->
                AlertDialog.Builder(this)
                    .setTitle("Are you sure? (Delete $name)")
                    .setMessage("This will IRREVERSIBLY erase the game and its saves. While the original folder/ZIP will not be touched, any saves made after the import WILL BE LOST.\n\nProceed anyways?")
                    .setPositiveButton("Delete") { _, _ ->
                        // thousands of files take a while: a dot hides the game from the list at once,
                        // and recoverImports finishes the job if the app dies first (no import's name starts with one)
                        val dir = File(games, name)
                        val gone = File(games, ".import-.deleted-$name").takeIf { dir.renameTo(it) } ?: dir
                        refresh()
                            Thread {
                                gone.deleteRecursively()
                                // the engine's old copies of its saves, in backups/<name>
                                File(filesDir, "data/emuera-tty").listFiles()?.forEach { File(it, name).deleteRecursively() }
                                runOnUiThread { refresh() }
                            }.start()
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun pickArchive() {
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
        startActivityForResult(startAt(i), PICK_ARCHIVE)
    }

    private fun pickFolder() {
        startActivityForResult(startAt(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)), PICK_FOLDER)
    }

    private fun pickFont(gameDir: File) {
        fontGameDir = gameDir
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
        i.setType("font/*")
        i.putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("font/ttf", "font/otf", "application/x-font-ttf", "application/vnd.ms-opentype"))
        startActivityForResult(startAt(i), PICK_FONT)
    }

    private fun unsetFont(gameDir: File) {
        val ttf = File(gameDir, "custom.ttf")
        val otf = File(gameDir, "custom.otf")
        var deleted = false
        if (ttf.exists()) { ttf.delete(); deleted = true }
        if (otf.exists()) { otf.delete(); deleted = true }
        status.text = if (deleted) "Font unset for ${gameDir.name}" else "No custom font set for ${gameDir.name}"
        refresh()
    }

    /** `--es initial_uri`: where the picker opens, for testing. */
    private fun startAt(i: Intent): Intent {
        intent.getStringExtra("initial_uri")?.let { i.putExtra(DocumentsContract.EXTRA_INITIAL_URI, Uri.parse(it)) }
        return i
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(request: Int, result: Int, data: Intent?) {
        super.onActivityResult(request, result, data)
        val uri = data?.data
        if (result != RESULT_OK || uri == null) return
        when (request) {
            PICK_ARCHIVE -> {
                val file = displayName(uri) ?: "game.zip"
                val suffix = listOf(".tar.gz", ".tgz", ".zip", ".tar").firstOrNull { file.lowercase().endsWith(it) }
                if (suffix == null) {
                    status.text = "$file: only zip, tar and tar.gz can be read here. Unpack it first and import the folder."
                    return
                }
                confirmThenImport(file.dropLast(suffix.length)) { tmp -> importArchive(uri, suffix, tmp) }
            }
            PICK_FOLDER -> {
                val folder = uri.lastPathSegment?.substringAfterLast(':')?.substringAfterLast('/')?.ifBlank { null } ?: "game"
                confirmThenImport(folder) { tmp -> importFolder(uri, tmp) }
            }
            PICK_FONT -> {
                val gameDir = fontGameDir ?: return
                fontGameDir = null
                // Copy the font file to the game's directory
                val fontName = displayName(uri) ?: "font.ttf"
                val ext = when {
                    fontName.lowercase().endsWith(".ttf") -> ".ttf"
                    fontName.lowercase().endsWith(".otf") -> ".otf"
                    else -> ".ttf"
                }
                val destName = "custom$ext"
                val destFile = File(gameDir, destName)
                try {
                    contentResolver.openInputStream(uri).use { input ->
                        input?.let { destFile.outputStream().use { output -> it.copyTo(output) } }
                    }
                    status.text = "Font set for ${gameDir.name}"
                    refresh()
                } catch (e: Exception) {
                    status.text = "Failed to set font: ${e.message}"
                }
            }
        }
    }

    private fun displayName(uri: Uri): String? =
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }

    private fun confirmThenImport(raw: String, work: (File) -> String) {
        val name = cleanName(raw)
        val go = { startImport(name, work) }
        if (File(games, name).exists()) {
            AlertDialog.Builder(this)
                .setTitle("Replace $name?")
                .setMessage("The game's files are replaced. Its saves are kept.")
                .setPositiveButton("Replace") { _, _ -> go() }
                .setNegativeButton("Cancel", null)
                .show()
        } else {
            go()
        }
    }

    private fun startImport(name: String, work: (File) -> String) {
        importing = true
        refresh()
        importButtons.forEach { it.isEnabled = false }
        spinner.visibility = ProgressBar.VISIBLE
        status.text = "Importing $name…"
        // Android may stop a background app, and the game list must not be touched meanwhile
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val t0 = System.currentTimeMillis()
        Native.importProgress = { n -> runOnUiThread { status.text = "Importing $name: $n files" } }
        Thread {
            val tmp = File(games, ".import-$name")
            tmp.deleteRecursively()
            var error = try {
                work(tmp)
            } catch (e: Exception) {
                e.message ?: e.toString()
            }
            if (error.isEmpty()) error = Native.finishImport(tmp.path, games.path, name)
            tmp.deleteRecursively()
            File(cacheDir, "import.tmp").delete()
            val secs = (System.currentTimeMillis() - t0) / 1000
            runOnUiThread {
                importing = false
                Native.importProgress = null
                importButtons.forEach { it.isEnabled = true }
                spinner.visibility = ProgressBar.GONE
                window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                status.text = if (error.isEmpty()) "Imported $name in $secs s." else "Couldn't import $name: $error"
                refresh()
            }
        }.start()
    }

    private fun importArchive(uri: Uri, suffix: String, tmp: File): String {
        contentResolver.openFileDescriptor(uri, "r").use { pfd ->
            if (pfd == null) return "the file can't be opened"
            // A zip is read from its end, so it has to be seekable. Files from cloud
            // storage often aren't: those get copied here first.
            val seekable = try {
                Os.lseek(pfd.fileDescriptor, 0, OsConstants.SEEK_SET)
                true
            } catch (e: ErrnoException) {
                false
            }
            if (seekable || suffix != ".zip") return Native.importArchive(pfd.fd, suffix, tmp.path)
        }
        val copy = File(cacheDir, "import.tmp")
        contentResolver.openInputStream(uri).use { from ->
            if (from == null) return "the file can't be opened"
            copy.outputStream().use { to -> from.copyTo(to) }
        }
        ParcelFileDescriptor.open(copy, ParcelFileDescriptor.MODE_READ_ONLY).use { pfd ->
            return Native.importArchive(pfd.fd, suffix, tmp.path)
        }
    }

    private fun importFolder(tree: Uri, tmp: File): String {
        val files = ArrayList<Pair<String, File>>()
        fun walk(docId: String, to: File) {
            to.mkdirs()
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
            contentResolver.query(children, arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE), null, null, null)?.use { c ->
                while (c.moveToNext()) {
                    val id = c.getString(0)
                    val child = c.getString(1)
                    // a name comes from outside: it must not lead out of the folder
                    if (child.isNullOrEmpty() || child == "." || child == ".." || child.contains('/')) continue
                    if (c.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) {
                        // a game's git history is big and the game never reads it
                        if (child != ".git") walk(id, File(to, child))
                    } else {
                        files.add(id to File(to, child))
                    }
                }
            } ?: throw IOException("${to.relativeTo(tmp).path.ifEmpty { "the folder" }} can't be listed") // (or it would count as empty)
        }
        walk(DocumentsContract.getTreeDocumentId(tree), tmp)

        // Each file costs a round trip to the system's document service, and a game has
        // thousands of small ones, so most of the time is waiting: several at once helps.
        val done = AtomicInteger()
        val failed = AtomicReference<String?>()
        val pool = Executors.newFixedThreadPool(4)
        for ((id, to) in files) {
            pool.execute {
                if (failed.get() != null) return@execute
                try {
                    val from = contentResolver.openInputStream(DocumentsContract.buildDocumentUriUsingTree(tree, id))
                        ?: throw IOException("${to.name} can't be opened")
                    from.use { to.outputStream().use { out -> from.copyTo(out) } }
                    val n = done.incrementAndGet()
                    if (n % 50 == 0) Native.onImportProgress(n)
                } catch (e: Exception) {
                    // a game with a file missing would break later, in confusing ways
                    failed.compareAndSet(null, e.message ?: e.toString())
                }
            }
        }
        pool.shutdown()
        pool.awaitTermination(1, TimeUnit.DAYS)
        Native.onImportProgress(done.get())
        return failed.get() ?: ""
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        // leaving now would orphan the import
        if (!importing) super.onBackPressed()
    }
}
