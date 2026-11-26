package io.github.astartes_gaming.emuera

import android.graphics.Bitmap
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.nio.ByteBuffer

/** The JNI bridge. The on* methods run on the VM thread, the UI's work is posted to the main thread. */
object Native {
    init {
        System.loadLibrary("emuera")
    }

    @JvmStatic external fun start(game: String, data: String, replay: String?, env: String, cols: Int)
    /** Sets HOME and XDG_CONFIG_HOME so the engine config file can be read and written. */
    @JvmStatic external fun setConfigHome(path: String)
    @JvmStatic external fun viewRows(rows: Int)
    @JvmStatic external fun textSize(percent: Int)
    @JvmStatic external fun maxTextCols(cols: Int)
    @JvmStatic external fun click(value: String)
    @JvmStatic external fun line(text: String)
    @JvmStatic external fun skip()
    @JvmStatic external fun quit()
    @JvmStatic external fun drawn(parseUs: Long, drawUs: Long)
    @JvmStatic external fun pointer(down: Boolean, col: Int, rowsUp: Int, button: String?)
    @JvmStatic external fun tap(btn: Int, col: Int, rowsUp: Int, button: String?)
    @JvmStatic external fun rightPress()
    @JvmStatic external fun wheel(up: Boolean, col: Int, rowsUp: Int)
    @JvmStatic external fun trim()
    /** The game's menu: the running game acts on these at its next input. */
    @JvmStatic external fun toTitle()
    @JvmStatic external fun reloadConfig()
    @JvmStatic external fun reloadErb()

    /**
     * The running game's saves: see Saves.kt. Those returning data start with \u0001 and the error when something went
     * wrong; the others return "" or the error.
     */
    @JvmStatic external fun saveList(): String
    @JvmStatic external fun saveCopy(path: String, slot: Int, moving: Boolean): String
    @JvmStatic external fun saveDelete(path: String): String
    @JvmStatic external fun saveImport(file: String, slot: Int): String
    @JvmStatic external fun saveBackup(): String
    @JvmStatic external fun saveBackups(): String
    @JvmStatic external fun saveRestore(from: String): String

    /** The game's real title (the `タイトル` line of its `CSV/GAMEBASE.csv`), or empty when it has none. */
    @JvmStatic external fun gameTitle(game: String): String
    /** The game's own settings (its emuera.config), in either process: see GameSettings.kt. */
    @JvmStatic external fun gameSettings(game: String): String
    /** "" or what went wrong; a null value takes the player's out. */
    @JvmStatic external fun setGameSetting(game: String, key: String, value: String?): String

    /** The engine's global settings: one TSV line per item (key, label, value, choices, live, appliesOnAndroid, group, help). */
    @JvmStatic external fun globalSettings(): String
    /** Sets one global setting; "" or what went wrong. A null value removes the key, back to its default. */
    @JvmStatic external fun setGlobalSetting(key: String, value: String?): String

    /** Whether the global "Show memory" setting is on (gates the memory readout overlay). */
    @JvmStatic external fun showMemory(): Boolean

    /** Import, on a worker thread. Each returns "" or what went wrong. */
    @JvmStatic external fun importArchive(fd: Int, suffix: String, tmp: String): String
    @JvmStatic external fun finishImport(tmp: String, games: String, name: String): String
    @JvmStatic external fun recoverImports(games: String)

    @JvmStatic external fun formattedMemory(): String
    @JvmStatic external fun rawMemory(): Long
    @JvmStatic external fun lastFunction(): String

    @Volatile var view: GameView? = null
    @Volatile var exitListener: ((Int) -> Unit)? = null
    @Volatile var importProgress: ((Int) -> Unit)? = null
    private val main = Handler(Looper.getMainLooper())

    @JvmStatic
    fun onUpdate(data: ByteArray) {
        main.post { view?.applyUpdate(data) }
    }

    /** `rgba` is only valid during this call. */
    @JvmStatic
    fun onImage(key: Int, w: Int, h: Int, rgba: ByteBuffer) {
        var bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        bmp.copyPixelsFromBuffer(rgba)
        // A hardware bitmap lives on the GPU only, so a picture isn't held in both places.
        if (Build.VERSION.SDK_INT >= 26) {
            val hw = bmp.copy(Bitmap.Config.HARDWARE, false)
            if (hw != null) {
                bmp.recycle()
                bmp = hw
            }
        } else {
            bmp.prepareToDraw()
        }
        val ready = bmp
        main.post { view?.images?.put(key, ready) }
    }

    @JvmStatic
    fun onImageDrop(key: Int) {
        main.post { view?.images?.remove(key)?.recycle() }
    }

    @JvmStatic
    fun onPlay(path: String, volume: Int, times: Int) {
        main.post { GameAudio.play(path, volume, times) }
    }

    @JvmStatic
    fun onStopAudio(bgm: Boolean) {
        main.post { GameAudio.stop(bgm) }
    }

    @JvmStatic
    fun onVolumes(effect: Int, bgm: Int) {
        main.post { GameAudio.volumes(effect, bgm) }
    }

    @JvmStatic
    fun onExit(code: Int) {
        main.post { exitListener?.invoke(code) }
    }

    @JvmStatic
    fun onImportProgress(files: Int) {
        importProgress?.invoke(files)
    }
}
