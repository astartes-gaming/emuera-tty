package io.github.astartes_gaming.emuera

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.os.SystemClock
import android.util.AttributeSet
import android.util.TypedValue
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.View
import android.widget.OverScroller
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.charset.StandardCharsets
import java.util.Locale
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

private const val WIDE: Byte = 1
private const val SINGLE: Byte = 2
private const val IMAGE: Byte = 3
private const val RECT: Byte = 4
private const val BOX: Byte = 5

/** INPUTMOUSEKEY: every touch goes to the script. */
private const val RAW = 4

/** The smallest text a game starts with. In sp: it follows the screen, and Android's font size setting. */
private const val MIN_TEXT_SP = 12f

/** Pictures can be taller than a row: rows this far above the view are still drawn. */
private const val IMAGE_REACH = 60
private const val MAX_ROWS = 8000

class Run(
    @JvmField val col: Int, @JvmField val cells: Int, @JvmField val dy: Int, @JvmField val kind: Byte, @JvmField val flags: Int,
    @JvmField val fg: Int, @JvmField val bg: Int, @JvmField val btn: String?, @JvmField val text: String?,
    @JvmField val key: Int, @JvmField val w: Float, @JvmField val h: Float, @JvmField val ypos: Float
)

class Row(@JvmField val runs: Array<Run>, @JvmField val gen: Long)

/** A client background layer (CBG*): anchored by its bottom-left corner to the view's, in Emuera's pixels. */
class Layer(@JvmField val key: Int, @JvmField val x: Int, @JvmField val y: Int, @JvmField val w: Int, @JvmField val h: Int, @JvmField val behind: Boolean)

/** The console: rows on a grid of half-width cells, drawn straight onto a canvas. */
class GameView(ctx: Context, attrs: AttributeSet? = null) : View(ctx, attrs) {
    val images = mutableMapOf<Int, Bitmap>()

    private var rows = ArrayList<Row>()
    /** the row where each logical line starts; `firstLine` is the number of lineStart[0] */
    private val lineStart = ArrayList<Int>()
    private var firstLine = 0
    /** the rows the content reaches: a `<div>` box can reach past the last row */
    private var contentRows = 0

    private var cols = 80
    private var screenBg = 0xFF000000.toInt()
    private var focusFg = 0xFFFFFF00.toInt()
    private var fontSize = 18f
    private var lineRatio = 19f / 18f
    private var validGen = -1L
    /** The last update applied, to tell one with nothing new. */
    private var lastUpdate = ByteArray(0)
    var inputKind = 0
        private set

    private var cellW = 10f
    private var lineH = 20f
    private var imageScale = 1f
    /** The game's window across this view, whatever the text size: layers and positions go by it. */
    private var windowCols = 80
    private var layerScale = 1f
    var textPercent = 100

    /** Custom font loaded from a game's font file, if any. */
    private var customTypeface: Typeface? = null

    private val narrowPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { typeface = Typeface.MONOSPACE }
    private val widePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { textLocale = Locale.JAPAN }
    private val singlePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { textLocale = Locale.JAPAN }
    private val fillPaint = Paint()
    private val bitmapPaint = Paint(Paint.FILTER_BITMAP_FLAG)
    private val destRect = RectF()
    /** The baseline that centres each kind of text in its row. */
    private var narrowBase = 0f
    private var wideBase = 0f
    private var singleBase = 0f

    private var scrollY = 0f
    private val scroller = OverScroller(ctx)
    private var pressed: String? = null
    private var wheelDrag = 0f
    private var touchX = -1f
    private var touchY = -1f
    private var touchDown = false

    private var pendingAck = false
    private var lastParseUs = 0L

    var onResized: (() -> Unit)? = null
    private var layers = emptyArray<Layer>()

    private val gestures = GestureDetector(ctx, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(e: MotionEvent): Boolean {
            scroller.forceFinished(true)
            wheelDrag = 0f
            setPressed(buttonAt(e.x, e.y))
            return true
        }

        override fun onScroll(a: MotionEvent?, b: MotionEvent, dx: Float, dy: Float): Boolean {
            setPressed(null)
            if (inputKind == RAW) {
                val notch = 3 * lineH
                wheelDrag += dy
                while (abs(wheelDrag) >= notch) {
                    val up = wheelDrag < 0
                    Native.wheel(up, col(b.x), rowsUp(b.y))
                    wheelDrag += if (up) notch else -notch
                }
                return true
            }
            scrollY = clamp(scrollY - dy)
            invalidate()
            return true
        }

        override fun onFling(a: MotionEvent?, b: MotionEvent, vx: Float, vy: Float): Boolean {
            if (inputKind == RAW) return true
            scroller.fling(0, scrollY.toInt(), 0, vy.toInt(), 0, 0, 0, maxScroll().toInt())
            postInvalidateOnAnimation()
            return true
        }

        override fun onLongPress(e: MotionEvent) {
            Native.rightPress()
            if (inputKind == RAW) Native.tap(2, col(e.x), rowsUp(e.y), buttonAt(e.x, e.y))
        }

        override fun onSingleTapUp(e: MotionEvent): Boolean {
            val b = buttonAt(e.x, e.y)
            when {
                inputKind == RAW -> Native.tap(0, col(e.x), rowsUp(e.y), b)
                b != null -> Native.click(b)
                inputKind == 3 -> Native.click("")
            }
            return true
        }
    }).apply {
        setOnDoubleTapListener(null)
    }

    private fun maxScroll(): Float = max(0f, contentRows * lineH - height)
    private fun clamp(v: Float) = max(0f, min(v, maxScroll()))

    override fun computeScroll() {
        if (scroller.computeScrollOffset()) {
            scrollY = clamp(scroller.currY.toFloat())
            postInvalidateOnAnimation()
        }
    }

    private fun col(x: Float) = max(0, (x / (fontSize / 2f * layerScale)).toInt())
    /** Emuera's rows at the window's scale, which positions and CLIENTHEIGHT go by. */
    private fun rowPx() = fontSize * lineRatio * layerScale
    private fun rowsUp(y: Float) = max(0, ((height - y) / rowPx()).toInt())
    private fun sendPointer() = Native.pointer(touchDown, col(touchX), rowsUp(touchY), buttonAt(touchX, touchY))

    override fun onTouchEvent(e: MotionEvent): Boolean {
        touchX = e.x
        touchY = e.y
        touchDown = e.actionMasked != MotionEvent.ACTION_UP && e.actionMasked != MotionEvent.ACTION_CANCEL
        sendPointer()
        gestures.onTouchEvent(e)
        if (e.actionMasked == MotionEvent.ACTION_UP || e.actionMasked == MotionEvent.ACTION_CANCEL) {
            setPressed(null)
        }
        return true
    }

    /** The button under the finger. Its text turns to the focus colour. */
    private fun setPressed(b: String?) {
        if (pressed == b) return
        pressed = b
        invalidate()
    }

    /** The buttons a tap can answer with right now: value -> label, one per value. */
    fun buttonLabels(): List<Pair<String, String>> {
        val byValue = linkedMapOf<String, MutableList<String>>()
        for (i in rows.indices) {
            val row = rows[i]
            if (row.gen != validGen) continue
            for (r in row.runs) {
                val b = r.btn ?: continue
                val label = when (r.kind) {
                    IMAGE -> "[img:${r.key}]"
                    RECT -> "■"
                    else -> r.text ?: ""
                }
                byValue.getOrPut(b) { mutableListOf() }.add(label)
            }
        }
        return byValue.entries.map { (v, labels) -> v to labels.joinToString(" ").trim() }
    }

    /** The value of the live button at a point, if any. */
    private fun buttonAt(x: Float, y: Float): String? {
        if (rows.isEmpty()) return null
        val top = contentRows * lineH - height - scrollY
        val last = min(rows.size - 1, floor((y + top) / lineH).toInt())
        // (a little above and below: a <div> places its text away from the row holding it)
        for (i in min(rows.size - 1, last + IMAGE_REACH) downTo max(0, last - IMAGE_REACH)) {
            val row = rows[i]
            if (row.gen != validGen) continue
            val ry = i * lineH - top
            for (j in row.runs.indices.reversed()) {
                val r = row.runs[j]
                val b = r.btn ?: continue
                val rx = r.col * cellW
                val rowTop = ry + r.dy * lineH
                val inside = if (r.kind == IMAGE) {
                    x >= rx && x < rx + r.w * imageScale && y >= rowTop + r.ypos * imageScale && y < rowTop + (r.ypos + r.h) * imageScale
                } else {
                    x >= rx && x < rx + r.cells * cellW && y >= rowTop && y < rowTop + lineH
                }
                if (inside) return b
            }
        }
        return null
    }

    /** How many half-width cells fit across the view at the smallest text a game starts with. */
    fun columnsAtMinimumText(): Int {
        val tf = customTypeface ?: Typeface.MONOSPACE
        val advance = Paint().apply { typeface = tf; textSize = 100f }.measureText("0") / 100f
        return (width / (advance * TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, MIN_TEXT_SP, resources.displayMetrics))).toInt()
    }

    /** Loads a custom font from a file. Returns true if successful. */
    fun setCustomFont(fontPath: String?): Boolean {
        if (fontPath == null || fontPath.isEmpty()) {
            customTypeface = null
            fit()
            return true
        }
        val file = File(fontPath)
        if (!file.exists()) return false
        try {
            customTypeface = Typeface.createFromFile(file)
            fit()
            return true
        } catch (e: Exception) {
            return false
        }
    }

    /** Fits `cols` half-width cells across the view. */
    private fun fit() {
        if (width <= 0) return
        cellW = width.toFloat() / cols
        val tf = customTypeface ?: Typeface.MONOSPACE
        narrowPaint.typeface = tf
        widePaint.typeface = tf
        singlePaint.typeface = tf
        narrowPaint.textSize = 100f
        narrowPaint.letterSpacing = 0f
        narrowPaint.isFakeBoldText = false
        narrowPaint.textSkewX = 0f
        narrowPaint.isUnderlineText = false
        narrowPaint.isStrikeThruText = false
        val textSize = cellW / (narrowPaint.measureText("0") / 100f)
        narrowPaint.textSize = textSize
        widePaint.textSize = textSize
        singlePaint.textSize = textSize
        // whatever the font's own width, a double-width character takes exactly two cells
        widePaint.letterSpacing = 0f
        widePaint.letterSpacing = (2 * cellW - widePaint.measureText("あ")) / textSize
        imageScale = cellW / (fontSize / 2f)
        // Emuera's row (its LINE HEIGHT) on the same scale as pictures: the font's own size is narrower than Emuera's,
        // and rows from it were a sixth short, cramping lines and letting pictures reach into the text under them
        lineH = fontSize * lineRatio * imageScale
        layerScale = width.toFloat() / windowCols / (fontSize / 2f)
        narrowBase = baseline(narrowPaint)
        wideBase = baseline(widePaint)
        singleBase = baseline(singlePaint)
        Native.viewRows((height / rowPx()).toInt())
        invalidate()
    }

    /** The baseline that centres the text of this paint in a row. */
    private fun baseline(p: Paint): Float {
        val asc = -p.ascent()
        val desc = p.descent()
        return asc + (lineH - (asc + desc)) / 2f
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        fit()
        onResized?.invoke()
    }

    /** One update from the VM: see `AndroidFe::sync`. */
    fun applyUpdate(data: ByteArray) {
        val t0 = SystemClock.elapsedRealtimeNanos()
        val b = ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN)
        val newCols = b.int
        val newWindow = b.int
        textPercent = b.int
        screenBg = b.int or 0xFF000000.toInt()
        focusFg = b.int or 0xFF000000.toInt()
        val fs = b.float
        val lh = b.float
        validGen = b.long
        val kindAt = b.position()
        inputKind = b.get().toInt()
        val wantAck = b.get().toInt() != 0
        val truncateTo = b.int
        val nLines = b.int
        if (newCols != cols || newWindow != windowCols || fs != fontSize || lh / fs != lineRatio) {
            cols = newCols
            windowCols = newWindow
            fontSize = fs
            lineRatio = lh / fs
            fit()
        }
        // Scrollback: keep old rows, only replace from truncateTo onwards
        // Incoming lines correspond to absolute lines [truncateTo, truncateTo + nLines)
        truncateLines(truncateTo)
        for (l in 0 until nLines) {
            val absLine = truncateTo + l
            val gen = b.long
            val nRows = b.int
            val idx = absLine - firstLine
            if (idx >= 0 && idx < lineStart.size) {
                // Replace existing logical line
                val from = lineStart[idx]
                val to = if (idx + 1 < lineStart.size) lineStart[idx + 1] else rows.size
                rows.subList(from, to).clear()
                for (r in 0 until nRows) rows.add(from + r, readRow(b, data, gen))
                // Update subsequent lineStart indices
                val delta = nRows - (to - from)
                for (i in idx + 1 until lineStart.size) lineStart[i] += delta
            } else if (idx == lineStart.size) {
                // Append new logical line at the end
                lineStart.add(rows.size)
                for (r in 0 until nRows) rows.add(readRow(b, data, gen))
            } else if (idx > lineStart.size) {
                // Gap in lines (shouldn't happen, but fill with empty)
                while (lineStart.size < idx) {
                    lineStart.add(rows.size)
                }
                lineStart.add(rows.size)
                for (r in 0 until nRows) rows.add(readRow(b, data, gen))
            } else {
                // idx < 0: line is before our scrollback start, ignore but consume data
                for (r in 0 until nRows) readRow(b, data, gen)
            }
        }
        layers = Array(b.int) { Layer(b.int, b.int, b.int, b.int, b.int, b.get().toInt() != 0) }
        if (rows.size > MAX_ROWS) dropOldest(rows.size - MAX_ROWS + MAX_ROWS / 4)
        measureContent()
        val scrolled = scrollY
        if (inputKind != 0) scrollY = 0f
        lastParseUs = (SystemClock.elapsedRealtimeNanos() - t0) / 1000
        if (wantAck) pendingAck = true
        val same = data.size == lastUpdate.size && data.indices.all { it == kindAt || data[it] == lastUpdate[it] }
        lastUpdate = data
        if (same && !wantAck && scrollY == scrolled) return
        if (touchX >= 0) sendPointer()
        invalidate()
    }

    /** How many rows the content reaches: a `<div>` box can reach past the last row. */
    private fun measureContent() {
        var last = rows.size
        for (i in rows.indices) {
            for (r in rows[i].runs) {
                if (r.kind == BOX && r.dy + r.bg > 0 && i + r.dy + r.bg > last) last = i + r.dy + r.bg
            }
        }
        contentRows = last
    }

    private fun readRow(b: ByteBuffer, data: ByteArray, gen: Long): Row {
        val n = b.int
        val runs = Array(n) {
            val col = b.short.toInt()
            val cells = b.short.toInt()
            val dy = b.short.toInt()
            val kind = b.get()
            val flags = b.get().toInt()
            val fg = b.int or 0xFF000000.toInt()
            val bg = b.int
            val btnLen = b.int
            var btn: String? = null
            if (btnLen >= 0) {
                btn = String(data, b.position(), btnLen, StandardCharsets.UTF_8)
                b.position(b.position() + btnLen)
            }
            when (kind) {
                IMAGE -> {
                    val key = b.int
                    val w = b.float
                    val h = b.float
                    val y = b.float
                    Run(col, cells, dy, kind, flags, fg, bg, btn, null, key, w, h, y)
                }
                RECT -> {
                    val w = b.float
                    val h = b.float
                    Run(col, cells, dy, kind, flags, fg, bg, btn, null, 0, w, h, 0f)
                }
                BOX -> Run(col, cells, dy, kind, flags, fg, bg, btn, null, 0, 0f, 0f, 0f)
                else -> {
                    val len = b.int
                    val s = String(data, b.position(), len, StandardCharsets.UTF_8)
                    b.position(b.position() + len)
                    Run(col, cells, dy, kind, flags, fg, bg, btn, s, 0, 0f, 0f, 0f)
                }
            }
        }
        return Row(runs, gen)
    }

    /** Removes logical line `abs` and everything after it (CLEARLINE, REUSELASTLINE). */
    private fun truncateLines(abs: Int) {
        val keep = abs - firstLine
        if (keep < 0) {
            rows.clear()
            lineStart.clear()
            firstLine = abs
            return
        }
        if (keep < lineStart.size) {
            val from = lineStart[keep]
            rows.subList(from, rows.size).clear()
            lineStart.subList(keep, lineStart.size).clear()
        }
    }

    private fun dropOldest(n: Int) {
        var k = 0
        while (k < lineStart.size && lineStart[k] < n) k++
        if (k >= lineStart.size) return
        val cut = lineStart[k]
        rows.subList(0, cut).clear()
        lineStart.subList(0, k).clear()
        for (i in lineStart.indices) lineStart[i] = lineStart[i] - cut
        firstLine += k
    }

    override fun onDraw(c: Canvas) {
        val t0 = SystemClock.elapsedRealtimeNanos()
        c.drawColor(screenBg)
        val h = height.toFloat()
        val top = contentRows * lineH - h - scrollY
        val first = max(0, floor(top / lineH).toInt() - IMAGE_REACH)
        val last = min(rows.size - 1, floor((top + h) / lineH).toInt())
        drawLayers(c, true)
        // backgrounds and pictures first, so that the text lands on top
        for (i in first..last) {
            val row = rows[i]
            val y = i * lineH - top
            for (r in row.runs) drawUnder(c, r, y)
        }
        // the text, from the bottom up: a `<div>` box is drawn over the rows below it
        for (i in last downTo first) {
            val row = rows[i]
            val y = i * lineH - top
            val live = row.gen == validGen
            for (r in row.runs) {
                if (r.text != null) drawText(c, r, y, live)
            }
        }
        // click zones: what can be tapped, as the game drew it
        for (i in first..last) {
            val row = rows[i]
            if (row.gen != validGen) continue
            val y = i * lineH - top
            for (r in row.runs) {
                if (r.btn == null) continue
                fillPaint.color = 0x26FFFFFF
                val x = r.col * cellW
                val ry = y + r.dy * lineH
                when (r.kind) {
                    IMAGE -> c.drawRect(x, ry + r.ypos * imageScale, x + r.w * imageScale, ry + (r.ypos + r.h) * imageScale, fillPaint)
                    RECT -> c.drawRect(x, ry, x + r.w * imageScale, ry + max(lineH, r.h * imageScale), fillPaint)
                    BOX -> c.drawRect(x, ry, x + r.cells * cellW, ry + r.bg * lineH, fillPaint)
                    else -> c.drawRect(x, ry, x + r.cells * cellW, ry + lineH, fillPaint)
                }
            }
        }
        drawLayers(c, false)
        val drawUs = (SystemClock.elapsedRealtimeNanos() - t0) / 1000
        if (pendingAck) {
            pendingAck = false
            Native.drawn(lastParseUs, drawUs)
        }
    }

    private fun drawLayers(c: Canvas, behind: Boolean) {
        for (l in layers) {
            if (l.behind != behind) continue
            val bmp = images[l.key] ?: continue
            val left = l.x * layerScale
            val bottom = height + l.y * layerScale
            destRect.set(left, bottom - l.h * layerScale, left + l.w * layerScale, bottom)
            c.drawBitmap(bmp, null, destRect, bitmapPaint)
        }
    }

    /** The background of a run: the text's own colour, a picture, a shape or a `<div>` box. */
    private fun drawUnder(c: Canvas, r: Run, rowY: Float) {
        val y = rowY + r.dy * lineH
        val x = r.col * cellW
        when (r.kind) {
            IMAGE -> {
                val bmp = images[r.key] ?: return
                destRect.set(x, y + r.ypos * imageScale, x + r.w * imageScale, y + (r.ypos + r.h) * imageScale)
                if (destRect.bottom < 0f || destRect.top > height) return
                c.drawBitmap(bmp, null, destRect, bitmapPaint)
            }
            RECT -> {
                fillPaint.color = r.fg
                c.drawRect(x, y, x + r.w * imageScale, y + max(lineH, r.h * imageScale), fillPaint)
            }
            BOX -> {
                // `bg` holds how many rows it covers
                fillPaint.color = r.fg
                c.drawRect(x, y, x + r.cells * cellW, y + r.bg * lineH, fillPaint)
            }
            else -> {
                if (r.bg != -1) {
                    fillPaint.color = r.bg or 0xFF000000.toInt()
                    c.drawRect(x, y, x + r.cells * cellW, y + lineH, fillPaint)
                }
            }
        }
    }

    /** The text of a run: the game pre-wraps it, so it is drawn as it is, with no clipping. */
    private fun drawText(c: Canvas, r: Run, rowY: Float, live: Boolean) {
        val p: Paint
        val base: Float
        when (r.kind) {
            WIDE -> {
                p = widePaint
                base = wideBase
            }
            SINGLE -> {
                p = singlePaint
                base = singleBase
            }
            else -> {
                p = narrowPaint
                base = narrowBase
            }
        }
        p.color = if (live && r.btn != null && r.btn == pressed) focusFg else r.fg
        p.isFakeBoldText = r.flags and 1 != 0
        p.textSkewX = if (r.flags and 2 != 0) -0.25f else 0f
        p.isUnderlineText = r.flags and 4 != 0
        p.isStrikeThruText = r.flags and 8 != 0
        c.drawText(r.text!!, r.col * cellW, rowY + r.dy * lineH + base, p)
    }
}
