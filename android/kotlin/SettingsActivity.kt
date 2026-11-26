package io.github.astartes_gaming.emuera

import android.app.Activity
import android.app.AlertDialog
import android.graphics.Color
import android.graphics.Typeface
import android.os.Bundle
import android.view.Gravity
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast

/**
 * The engine's global settings: one screen listing every item `globalSettings()` gives,
 * grouped by category. Each item shows its current value; tapping it opens a picker of
 * its choices. Items that do not apply on Android are grayed out and cannot be changed.
 * Values are written to the engine's config file through `setGlobalSetting`, so they
 * survive restarts and are seen by the desktop program too.
 */
class SettingsActivity : Activity() {
    private lateinit var list: LinearLayout

    override fun onCreate(state: Bundle?) {
        setTheme(android.R.style.Theme_Material_NoActionBar)
        super.onCreate(state)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        val pad = (16 * resources.displayMetrics.density).toInt()
        val scroll = ScrollView(this)
        list = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad)
        }

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad)
            padForSystemBars(pad)
            addView(TextView(this@SettingsActivity).apply {
                text = "Settings"
                textSize = 24f
                setPadding(0, 0, 0, pad)
            })
            addView(TextView(this@SettingsActivity).apply {
                text = "The engine's own settings, shared with the desktop program. " +
                    "Items that do not apply on Android are shown but cannot be changed."
                textSize = 12f
                setTextColor(0xFF999999.toInt())
                setPadding(0, 0, 0, pad)
            })
            addView(scroll)
        }
        scroll.addView(list)
        setContentView(root)

        buildList()
    }

    /** Reads the engine's settings and rebuilds the list, grouped by category. */
    private fun buildList() {
        list.removeAllViews()
        val dp = resources.displayMetrics.density
        val order = listOf("Language", "Display", "Input", "Translation", "Advanced")

        val items = Native.globalSettings().lines().filter { it.isNotEmpty() }.mapNotNull { line ->
            val f = line.split('\t')
            if (f.size < 8) null else Row(f[0], f[1], f[2], f[3], f[4] == "true", f[5] == "true", f[6], f[7])
        }

        for (group in order) {
            val rows = items.filter { it.group == group }
            if (rows.isEmpty()) continue
            list.addView(TextView(this@SettingsActivity).apply {
                text = group
                setTypeface(Typeface.DEFAULT, Typeface.BOLD)
                textSize = 14f
                setTextColor(0xFFBBBBBB.toInt())
                setPadding(0, (16 * dp).toInt(), 0, (6 * dp).toInt())
            })
            for (row in rows) {
                list.addView(buildRow(row))
            }
        }
    }

    private inner class Row(
        val key: String,
        val label: String,
        val value: String,
        val choices: String,
        val live: Boolean,
        val appliesOnAndroid: Boolean,
        val group: String,
        val help: String,
    ) {
        val choiceList: List<String> = choices.split(',').filter { it.isNotEmpty() }
    }

    private fun buildRow(row: Row): LinearLayout {
        val dp = resources.displayMetrics.density
        val line = LinearLayout(this@SettingsActivity).apply {
            orientation = LinearLayout.VERTICAL
        }

        val header = LinearLayout(this@SettingsActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            val labelView = TextView(this@SettingsActivity).apply {
                text = row.label
                textSize = 16f
                setTextColor(if (row.appliesOnAndroid) Color.WHITE else 0xFF888888.toInt())
                layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
            }
            val valueView = TextView(this@SettingsActivity).apply {
                text = row.value
                textSize = 16f
                setTextColor(0xFF4CAF50.toInt())
                setGravity(Gravity.END)
            }
            addView(labelView)
            addView(valueView)
        }

        val helpView = TextView(this@SettingsActivity).apply {
            text = row.help
            textSize = 12f
            setTextColor(0xFF999999.toInt())
            setPadding(0, (2 * dp).toInt(), 0, (8 * dp).toInt())
        }

        line.addView(header)
        line.addView(helpView)

        if (!row.appliesOnAndroid) {
            val note = TextView(this@SettingsActivity).apply {
                text = "(does not apply on Android)"
                textSize = 12f
                setTextColor(0xFF888888.toInt())
                setPadding(0, 0, 0, (8 * dp).toInt())
            }
            line.addView(note)
        }

        line.setOnClickListener {
            if (!row.appliesOnAndroid) return@setOnClickListener
            showPicker(row)
        }

        return line
    }

    /** Shows the choices for one setting as a picker; "Reset" puts it back to its default. */
    private fun showPicker(row: Row) {
        // The app uses a fixed image protocol, so images is on/off only: "on" keeps pictures
        // (stored as "auto"), "off" stops them (stored as "none").
        val isImages = row.key == "images"
        val items = if (isImages) listOf("on", "off") else row.choiceList
        val shown = if (isImages) (if (row.value == "auto") "on" else "off") else row.value
        val checked = items.indexOf(shown).coerceAtLeast(0).coerceAtMost(items.lastIndex)
        var selected = checked
        val dialog = AlertDialog.Builder(this@SettingsActivity)
            .setTitle(row.label)
            .setSingleChoiceItems(items.toTypedArray(), checked) { _, sel -> selected = sel }
            .setPositiveButton("OK") { _, _ ->
                if (selected >= 0 && selected < items.size) {
                    val saved = if (isImages) (if (items[selected] == "on") "auto" else "none") else items[selected]
                    saveSetting(row.key, saved)
                }
            }
            .setNeutralButton("Reset") { _, _ ->
                saveSetting(row.key, null)
            }
            .setNegativeButton("Cancel", null)
            .create()
        dialog.show()
    }

    /** Writes one setting through the engine and refreshes the list on success. */
    private fun saveSetting(key: String, value: String?) {
        val err = Native.setGlobalSetting(key, value)
        if (err.isEmpty()) {
            buildList()
        } else {
            Toast.makeText(this@SettingsActivity, "Couldn't save: $err", Toast.LENGTH_SHORT).show()
        }
    }
}
