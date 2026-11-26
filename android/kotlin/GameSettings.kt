package io.github.astartes_gaming.emuera

import android.app.Activity
import android.app.AlertDialog
import android.graphics.Color
import android.graphics.Typeface
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.io.File

/**
 * The game's own settings: every item of Emuera's settings window, tab by tab,
 * kept in the game's emuera.config (which Emuera.exe reads too). `saved` runs
 * as the dialog closes, if anything was written.
 */
fun Activity.showGameSettings(game: File, saved: () -> Unit) {
    class Setting(
        val key: String,
        val label: String,
        val value: String,
        val source: String,
        val effect: String,
        val store: String,
        val kind: String,
        val choices: List<Pair<String, String>>,
        val view: View,
    ) {
        fun isFixed() = source == "fixed"
        fun isPlayer() = source == "player"

        /** the value the player has now, or null if unchanged */
        fun changed(): String? {
            val now = when (kind) {
                "bool" -> if ((view as CheckBox).isChecked) "YES" else "NO"
                "choice" -> {
                    val btn = view as Button
                    choices.find { it.second == btn.text.toString() }?.first ?: btn.text.toString()
                }
                else -> (view as EditText).text.toString().trim()
            }
            return now.takeIf { it != value }
        }
    }

    val dp = resources.displayMetrics.density
    val list = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding((20 * dp).toInt(), (8 * dp).toInt(), (20 * dp).toInt(), 0)
    }
    val activity = this@showGameSettings
    val problem = TextView(this).apply {
        setTextColor(Color.RED)
        visibility = View.GONE
    }
    list.addView(problem)
    list.addView(TextView(this).apply { text = "Saved in the game's emuera.config, which Emuera.exe reads too." })
    list.addView(TextView(this).apply {
        text = "Source —\n  emuera: Emuera's default\n  game: the game's default\n  player: yours (has Reset)\n  fixed: locked by the game"
        textSize = 11f
        setTextColor(0xFF999999.toInt())
        setPadding(0, (4 * dp).toInt(), 0, 0)
    })
    list.addView(TextView(this).apply {
        text = "Effect —\n  now: takes effect right away\n  restart: at the next game start\n  exe: Emuera.exe only, nothing on Android"
        textSize = 11f
        setTextColor(0xFF999999.toInt())
        setPadding(0, 0, 0, (8 * dp).toInt())
    })

    var tab = ""
    val settings = mutableListOf<Setting>()

    val lines = Native.gameSettings(game.path).lines().filter { it.isNotEmpty() }
    for (line in lines) {
        val f = line.split('\t')
        if (f.size < 9) continue
        val key = f[0]
        val label = f[1]
        val tabName = f[2]
        val kind = f[3]
        val value = f[4]
        val source = f[5]
        val effect = f[6]
        val choicesStr = f[7]
        val store = f[8]

        if (tabName != tab) {
            tab = tabName
            list.addView(TextView(this).apply {
                text = tabName
                setTypeface(Typeface.DEFAULT, Typeface.BOLD)
                setPadding(0, (16 * dp).toInt(), 0, 0)
            })
        }

        val shown = if (source == "fixed") "$label (the game doesn't allow changing it)" else label
        val choices = choicesStr.split(',').filter { it.isNotEmpty() }.map { pair ->
            val parts = pair.split(':')
            Pair(parts.getOrElse(0) { "" }, parts.getOrElse(1) { "" })
        }

        val row = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }

        val labelView = TextView(this).apply {
            text = shown
            textSize = 15f
            setTextColor(if (source == "fixed") 0xFF888888.toInt() else Color.WHITE)
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        }
        header.addView(labelView)

        var view: View
        when (kind) {
            "bool" -> {
                view = CheckBox(this).apply {
                    isChecked = value == "YES"
                    isEnabled = source != "fixed"
                }
                header.addView(view)
            }
            "choice" -> {
                val btn = Button(this).apply {
                    text = choices.find { it.first == value }?.second ?: value
                    isEnabled = source != "fixed"
                }
                btn.setOnClickListener {
                    activity.showChoicePicker(label, choices, btn)
                }
                view = btn
                header.addView(view)
            }
            else -> {
                view = EditText(this).apply {
                    setText(value)
                    setSingleLine()
                    isEnabled = source != "fixed"
                    contentDescription = label
                    if (kind.startsWith("int:")) {
                        inputType = InputType.TYPE_CLASS_NUMBER
                        val parts = kind.split(':')
                        if (parts.size >= 3) hint = "${parts[1]} to ${parts[2]}"
                    } else if (kind == "color") {
                        hint = "R,G,B"
                    }
                }
                header.addView(view)
            }
        }

        if (source == "player") {
            val reset = Button(this).apply {
                text = "Reset"
                textSize = 12f
                minWidth = 0
                minimumWidth = 0
                setOnClickListener {
                    val e = Native.setGameSetting(game.path, key, null)
                    if (e.isEmpty()) {
                        // the value reverts to its default; reopen the dialog to see it
                    }
                }
            }
            header.addView(reset)
        }

        row.addView(header)

        val infoView = TextView(this).apply {
            text = "$source · $effect"
            textSize = 11f
            setTextColor(0xFF999999.toInt())
            setPadding(0, (2 * dp).toInt(), 0, (8 * dp).toInt())
        }
        row.addView(infoView)

        list.addView(row)
        settings.add(Setting(key, label, value, source, effect, store, kind, choices, view))
    }

    var wrote = false
    val dialog = AlertDialog.Builder(this)
        .setTitle("${game.name}: settings")
        .setView(ScrollView(this).apply { addView(list) })
        .setPositiveButton("Save", null)
        .setNeutralButton("Game's defaults", null)
        .setNegativeButton("Cancel", null)
        .create()
    dialog.setOnShowListener {
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
            val errors = settings.mapNotNull { s ->
                val v = s.changed() ?: return@mapNotNull null
                val e = Native.setGameSetting(game.path, s.key, v)
                if (e.isEmpty()) {
                    wrote = true
                    null
                } else {
                    "${s.label}: $e"
                }
            }
            if (errors.isEmpty()) {
                dialog.dismiss()
            } else {
                problem.text = errors.joinToString("\n")
                problem.visibility = View.VISIBLE
            }
        }
        dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener {
            for (s in settings) if (s.isPlayer() && Native.setGameSetting(game.path, s.key, null).isEmpty()) wrote = true
            dialog.dismiss()
        }
    }
    dialog.setOnDismissListener { if (wrote) saved() }
    dialog.show()
}

private fun Activity.showChoicePicker(title: String, choices: List<Pair<String, String>>, btn: Button) {
    val labels = choices.map { it.second }.toTypedArray()
    val checked = labels.indexOf(btn.text.toString()).coerceAtLeast(0)
    AlertDialog.Builder(this)
        .setTitle(title)
        .setSingleChoiceItems(labels, checked) { _, sel ->
            btn.text = labels[sel]
        }
        .setPositiveButton("OK", null)
        .setNegativeButton("Cancel", null)
        .show()
}
