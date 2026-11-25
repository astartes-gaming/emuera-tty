package io.github.astartes_gaming.emuera

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.graphics.Color
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.Process
import android.util.Log
import android.view.FrameMetrics
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import java.io.File

import kotlin.ranges.step

private const val TEXT_SIZE_FILE = "text-size"
/** As percentages of the size the game's own window width gives. */
private val TEXT_SIZES = intArrayOf(
    75, 100, 125, 150, 175, 200, 250, 300
)

/**
 * A running game, in its own process (":game" in the manifest). The VM can only start once
 * per process, and ending the process is what returns all of a game's memory to Android.
 *
 * Intent extras (adb can start this directly with them: it holds the permission the manifest asks for, other apps don't):
 *   game    folder under files/games
 *   replay  a file of recorded answers under files/bench: plays files/bench/game with it,
 *           writes files/bench/replay/, and keeps away from the player's own saves
 *   env     extra engine settings, as KEY=VALUE separated by commas (AFFINITY=0f pins the app)
 *   cols    width in half-width cells, for a game that doesn't declare its own (default 120)
 *   tablet  set by the launcher on a large screen: the game turns with it, rather than staying landscape
 */
class GameActivity : Activity() {
    private lateinit var view: GameView
    private var started = false
    private var replayName: String? = null
    private val saves by lazy { Saves(this) }
    private val frames = StringBuilder("total_ns,draw_ns,sync_ns,command_ns\n")

    override fun onCreate(state: Bundle?) {
        setTheme(android.R.style.Theme_Material_NoActionBar_Fullscreen)
        super.onCreate(state)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE or WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_HIDDEN)
        // the volume keys turn the game's sound up and down, not the ringer
        volumeControlStream = AudioManager.STREAM_MUSIC
        // the manifest's landscape is for phones: a tablet is wide enough either way, and may not
        // turn for an app, which leaves the game in a box at the top of an upright screen.
        // (the launcher knows: in this process the screen's size is the box's, which looks like a phone)
        if (intent.getBooleanExtra("tablet", false)) requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED

        replayName = intent.getStringExtra("replay")
        val dataDir = if (replayName != null) File(filesDir, "bench") else filesDir
        val game = if (replayName != null) File(dataDir, "game") else intent.getStringExtra("game")?.let { File(File(filesDir, "games"), it) }
        if (game == null || !game.isDirectory) {
            setContentView(TextView(this).apply {
                text = "That game isn't here any more."
                setTextColor(Color.WHITE)
                setPadding(48, 48, 48, 48)
            })
            return
        }

        // a crash in the UI goes to the log too, or the report would have nothing to say
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, e ->
            File(dataDir, "engine.log").appendText(Log.getStackTraceString(e))
            previous?.uncaughtException(thread, e)
        }

        view = GameView(this)
        Native.view = view
        // Load custom font from game directory (custom.ttf or custom.otf)
        val customTtf = File(game, "custom.ttf")
        val customOtf = File(game, "custom.otf")
        val fontPath = if (customTtf.exists()) customTtf.path else if (customOtf.exists()) customOtf.path else null
        view.setCustomFont(fontPath)
        view.onResized = {
            if (!started) {
                started = true
                val replay = replayName?.let { File(dataDir, it).path }
                if (replay == null) gameMarker(this).writeText("")
                var env = (intent.getStringExtra("env") ?: "").replace(',', '\n')
                if (replay == null) {
                    env += "\nMAX_TEXT_COLS=${view.columnsAtMinimumText()}"
                    val sizes = getSharedPreferences(TEXT_SIZE_FILE, MODE_PRIVATE)
                    val name = intent.getStringExtra("game")
                    if (sizes.contains(name)) env += "\nTEXT_PERCENT=${sizes.getInt(name, 100)}"
                }
                // (a replay starts from the headless reference's default width)
                val cols = intent.getStringExtra("cols")?.toIntOrNull() ?: if (replay != null) 80 else 120
                Native.start(game.path, dataDir.path, replay, env, cols)
            } else if (replayName == null) {
                Native.maxTextCols(view.columnsAtMinimumText())
            }
        }
        Native.exitListener = { code -> onEngineExit(code, dataDir) }

        val input = EditText(this).apply {
            setSingleLine()
            hint = "type here"
            imeOptions = EditorInfo.IME_ACTION_SEND or EditorInfo.IME_FLAG_NO_EXTRACT_UI
            setTextColor(Color.WHITE)
            setHintTextColor(Color.GRAY)
        }
        val send = {
            Native.line(input.text.toString())
            input.setText("")
        }
        input.setOnEditorActionListener { _, _, _ ->
            send()
            true
        }
        val bar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(0xFF202020.toInt())
            addView(input, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            for ((label, description, step) in listOf(Triple("FS-", "Smaller text", -1), Triple("FS+", "Larger text", 1))) {
                addView(Button(context).apply {
                    text = label
                    contentDescription = description
                    isAllCaps = false
                    minWidth = 0
                    minimumWidth = 0
                    setOnClickListener { changeTextSize(step) }
                })
            }
            addView(Button(context).apply {
                text = "Enter"
                setOnClickListener { send() }
            })
            addView(Button(context).apply {
                text = "Skip"
                setOnClickListener { Native.skip() }
            })
            val memoryView = TextView(context).apply {
                text = Native.formattedMemory()
            }
            addView(memoryView)
            val memoryHandler = Handler(Looper.getMainLooper())
            val memoryUpdater = object : Runnable {
                override fun run(){
                   memoryView.text = Native.formattedMemory()
                   memoryHandler.postDelayed(this, 1000)
                }
            }
            memoryHandler.post(memoryUpdater)
        }
        // the game's menu, over the console's top right corner
        val screen = FrameLayout(this).apply {
            addView(view)
            if (replayName == null) addView(Button(context).apply {
                text = "⚙"
                textSize = 22f
                contentDescription = "Game menu"
                setTextColor(Color.WHITE)
                setBackgroundColor(0x80303030.toInt())
                minWidth = 0
                minimumWidth = (48 * resources.displayMetrics.density).toInt()
                setOnClickListener { showMenu(game) }
            }, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.END))
        }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.BLACK)
            padForSystemBars()
            addView(screen, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
            if (replayName == null) addView(bar)
        }
        setContentView(root)

        // only for showing that a game is still running: all works without it
        if (replayName == null && Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }

        // how long each frame really took, the render thread and GPU submission included
        if (replayName != null) {
            val thread = HandlerThread("frames").apply { start() }
            window.addOnFrameMetricsAvailableListener({ _, m, _ ->
                frames.append(m.getMetric(FrameMetrics.TOTAL_DURATION)).append(',')
                    .append(m.getMetric(FrameMetrics.DRAW_DURATION)).append(',')
                    .append(m.getMetric(FrameMetrics.SYNC_DURATION)).append(',')
                    .append(m.getMetric(FrameMetrics.COMMAND_ISSUE_DURATION)).append('\n')
            }, Handler(thread.looper))
        }
    }

    override fun onResume() {
        super.onResume()
        if (::view.isInitialized && replayName == null) {
            // (each time: a notification allowed only just now appears, and Android refuses this from the background)
            val service = Intent(this, GameService::class.java).putExtra("game", intent.getStringExtra("game"))
            if (Build.VERSION.SDK_INT >= 26) startForegroundService(service) else startService(service)
        }
    }

    /**
     * The player's text size for this game, kept by the game's name so that a new version keeps it.
     * It wins over the minimum a game otherwise starts at, in case that's still not what they want.
     * shortcut: it's a share of the game's window, so on a turned tablet the same choice is smaller upright; keep one per orientation if players want that.
     */
    private fun changeTextSize(step: Int) {
        val now = view.textPercent
        val next = (if (step > 0) TEXT_SIZES.firstOrNull { it > now } else TEXT_SIZES.lastOrNull { it < now }) ?: return
        view.textPercent = next
        // (commit: leaving a game ends this process at once)
        getSharedPreferences(TEXT_SIZE_FILE, MODE_PRIVATE).edit().putInt(intent.getStringExtra("game"), next).commit()
        Native.textSize(next)
    }

    override fun onStart() {
        super.onStart()
        GameAudio.resume()
    }

    // (not onTrimMemory: Android doesn't send UI_HIDDEN to a process with a foreground service)
    override fun onStop() {
        super.onStop()
        GameAudio.pause()
        if (started) Native.trim()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && ::view.isInitialized) {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (!started) {
            backToList()
            return
        }
        AlertDialog.Builder(this)
            .setTitle("Leave this game?")
            .setMessage("Anything since your last in-game save will be lost.")
            .setPositiveButton("Leave") { _, _ -> backToList() }
            .setNegativeButton("Keep playing", null)
            .show()
    }

    /** The game's menu: of emuera-tty's settings overlay, what a phone needs. The game acts on it at its next input. */
    private fun showMenu(game: File) {
        val rows = listOf<Pair<String, () -> Unit>>(
            "Game settings…" to { showGameSettings(game) { Native.reloadConfig() } },
            "Manage saves…" to { saves.show() },
            "Back to the title" to { losing("Back to the title screen?", "Back to the title") { Native.toTitle() } },
            "Restart the game" to { losing("Restart the game?", "Restart") { restart() } },
            "Reload ERB files" to { Native.reloadErb() }
        )
        AlertDialog.Builder(this)
            .setItems(rows.map { it.first }.toTypedArray()) { _, i -> rows[i].second() }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun losing(title: String, yes: String, then: () -> Unit) {
        AlertDialog.Builder(this)
            .setTitle(title)
            .setMessage("Anything since your last in-game save will be lost.")
            .setPositiveButton(yes) { _, _ -> then() }
            .setNegativeButton("Keep playing", null)
            .show()
    }

    /**
     * This process can only start a game once: it ends, and the list starts the game again. The list is told through
     * a file, not an intent, so that other apps still can't start a game through it.
     */
    private fun restart() {
        restartMarker(this).writeText(intent.getStringExtra("game") ?: return)
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP))
        backToList()
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(request: Int, result: Int, data: Intent?) {
        saves.onResult(request, result, data)
    }

    /** Ends this process, which frees all of the game's memory. The launcher is underneath. */
    private fun backToList() {
        gameMarker(this).delete()
        if (started) Native.quit()
        finish()
        Process.killProcess(Process.myPid())
    }

    private fun onEngineExit(code: Int, dataDir: File) {
        if (replayName != null) {
            val dir = File(dataDir, "replay").apply { mkdirs() }
            File(dir, "frames.csv").writeText(frames.toString())
            File(dir, "exit.txt").writeText("exit=$code\n")
            File(dir, "DONE").writeText("")
        } else if (code != 0) {
            showEngineLog("The game stopped with an error", File(dataDir, "engine.log")) { backToList() }
        } else {
            backToList()
        }
    }
}
