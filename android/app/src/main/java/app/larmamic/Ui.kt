@file:OptIn(ExperimentalLayoutApi::class)

package app.larmamic

import android.app.Activity
import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlinx.coroutines.delay
import kotlin.math.hypot
import kotlin.math.roundToInt

private val Bg = Color(0xFF0D1317)
private val Ink = Color(0xFFE8F0F3)
private val Mute = Color(0xFF8FA6B1)
private val Line = Color(0xFF2A3A44)
private val Sig = Color(0xFF38E0C8)
private val Warn = Color(0xFFFF6B6B)
private val Off = Color(0xFF17232A)
private val Ghost = Color(0xFF344B57)

private val scheme = darkColorScheme(
    primary = Sig, onPrimary = Color(0xFF04201B), background = Bg, onBackground = Ink, surface = Bg, onSurface = Ink,
    secondaryContainer = Color(0xFF123A36), onSecondaryContainer = Sig, outline = Line, surfaceVariant = Off,
)

@Composable
fun LarmaApp(
    connection: Connection?,
    inputError: Msg?,
    language: String,
    onLanguage: (String) -> Unit,
    onMode: (String) -> Unit,
    onStart: () -> Unit,
    onStop: () -> Unit,
    onScan: () -> Unit,
    onAddress: (String) -> Unit,
    onForget: (Connection) -> Unit,
) {
    MaterialTheme(colorScheme = scheme) {
        val status by Engine.status.collectAsState()
        val message by Engine.message.collectAsState()
        val params by Engine.params.collectAsState()
        val ptt by Engine.ptt.collectAsState()
        var saver by remember { mutableStateOf(false) }
        val running = status != Status.IDLE && status != Status.ERROR

        // Stopped or failed: leave battery saver so the user sees what happened
        LaunchedEffect(status) { if (status == Status.IDLE || status == Status.ERROR) saver = false }

        if (saver) {
            SaverScreen(onExit = { saver = false })
            return@MaterialTheme
        }

        // Surface gives every Text the right default colour (otherwise labels without an explicit colour look dim)
        Surface(Modifier.fillMaxSize(), color = Bg, contentColor = Ink) {
            Column(
                Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing),
            ) {
                Column(
                    Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Header(status, message ?: inputError, language, onLanguage)
                    MeterView(enabled = params != null)
                    if (params != null) {
                        Controls()
                    } else {
                        ConnectionCard(connection, running, onMode, onScan, onAddress, onForget)
                    }
                    Spacer(Modifier.height(8.dp))
                }
                Footer(
                    running = running, hasConnection = connection != null, ptt = ptt && params != null,
                    onStart = onStart, onStop = onStop, onScan = onScan, onSaver = { saver = true },
                )
            }
        }
    }
}

// ------------------------------------------------------------------ header

private fun statusTitle(s: Status) = when (s) {
    Status.IDLE -> R.string.title_idle
    Status.CONNECTING -> R.string.title_connecting
    Status.LIVE -> R.string.title_live
    Status.RETRY -> R.string.title_retry
    Status.ERROR -> R.string.title_error
}

@Composable
private fun Header(status: Status, message: Msg?, language: String, onLanguage: (String) -> Unit) {
    Column(Modifier.padding(top = 8.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
            Text(stringResource(R.string.app_name), color = Mute, fontSize = 14.sp)
            LanguagePicker(language, onLanguage)
        }
        Text(
            stringResource(statusTitle(status)),
            fontSize = 32.sp, fontWeight = FontWeight.ExtraBold,
            color = when (status) { Status.LIVE -> Sig; Status.ERROR, Status.RETRY -> Warn; else -> Ink },
        )
        val sub = when {
            message != null -> stringResource(message.res, *message.args.toTypedArray())
            status == Status.IDLE -> stringResource(R.string.sub_idle, stringResource(R.string.app_name))
            status == Status.CONNECTING -> stringResource(R.string.sub_connecting)
            status == Status.LIVE -> stringResource(R.string.sub_live)
            status == Status.RETRY -> stringResource(R.string.sub_retry)
            else -> ""
        }
        Text(sub, color = Mute, fontSize = 15.sp, modifier = Modifier.padding(top = 6.dp))
    }
}

@Composable
private fun LanguagePicker(language: String, onLanguage: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val current = LocalConfiguration.current.locales[0].language.let { if (it == "in") "id" else it }
    val name = LANGUAGES.firstOrNull { it.first == current }?.second ?: "English"
    Box {
        OutlinedButton(onClick = { open = true }, shape = RoundedCornerShape(10.dp)) {
            Text(if (language == "auto") "${stringResource(R.string.lang_auto)} · $name" else name, color = Ink, fontSize = 13.sp)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text(stringResource(R.string.lang_auto)) }, onClick = { open = false; onLanguage("auto") })
            LANGUAGES.forEach { (code, label) ->
                DropdownMenuItem(text = { Text(label) }, onClick = { open = false; onLanguage(code) })
            }
        }
    }
}

// ------------------------------------------------------------------ connection

@Composable
private fun ConnectionCard(
    connection: Connection?,
    running: Boolean,
    onMode: (String) -> Unit,
    onScan: () -> Unit,
    onAddress: (String) -> Unit,
    onForget: (Connection) -> Unit,
) {
    val mode by Engine.mode.collectAsState()
    var text by remember { mutableStateOf("") }
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Off).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(stringResource(R.string.mode_title), fontWeight = FontWeight.Bold, fontSize = 17.sp)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("wifi" to "Wi-Fi", "usb" to stringResource(R.string.mode_usb), "bt" to "Bluetooth").forEach { (m, label) ->
                FilterChip(selected = mode == m, onClick = { onMode(m) }, label = { Text(label) }, colors = chipColors())
            }
        }
        Text(
            stringResource(when (mode) { "usb" -> R.string.mode_usb_hint; "bt" -> R.string.mode_bt_hint; else -> R.string.mode_wifi_hint }),
            color = Mute, fontSize = 13.sp,
        )
        if (connection != null) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.conn_last, connection.host), color = Mute, fontSize = 13.sp, modifier = Modifier.weight(1f))
                TextButton(onClick = { onForget(connection) }) { Text(stringResource(R.string.conn_forget), color = Warn, fontSize = 13.sp) }
            }
        }
        Button(onClick = onScan, enabled = !running, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.scan_btn)) }
        OutlinedTextField(
            value = text, onValueChange = { text = it }, singleLine = true, modifier = Modifier.fillMaxWidth(),
            label = { Text(stringResource(R.string.addr_label)) },
        )
        OutlinedButton(onClick = { onAddress(text) }, enabled = text.isNotBlank() && !running, modifier = Modifier.fillMaxWidth()) {
            Text(stringResource(R.string.addr_connect), color = Ink)
        }
    }
}

@Composable
private fun chipColors() = FilterChipDefaults.filterChipColors(
    selectedContainerColor = Color(0xFF123A36), selectedLabelColor = Sig, labelColor = Ink,
)

// ------------------------------------------------------------------ meter

private const val DB_MIN = -72f
private const val SEGMENTS = 48

@Composable
private fun MeterView(enabled: Boolean) {
    val meterState by Engine.meter.collectAsState()
    val params by Engine.params.collectAsState()
    val thr = (params?.get("thr") as? Double)?.toFloat() ?: -52f
    val thrNow by rememberUpdatedState(thr)
    var dIn by remember { mutableFloatStateOf(-99f) }
    var dOut by remember { mutableFloatStateOf(-99f) }
    var peak by remember { mutableFloatStateOf(-99f) }
    val latest by rememberUpdatedState(meterState)

    // animation: fast attack, smooth decay (only while this screen is visible)
    LaunchedEffect(Unit) {
        var last = 0L
        var peakAt = 0L
        while (true) {
            androidx.compose.runtime.withFrameNanos { now ->
                val dt = if (last == 0L) 0.016f else ((now - last) / 1e9f).coerceAtMost(0.1f)
                last = now
                dIn = maxOf(latest.inDb, dIn - 40f * dt)
                dOut = maxOf(latest.outDb, dOut - 48f * dt)
                if (dOut >= peak) {
                    peak = dOut; peakAt = now
                } else if ((now - peakAt) / 1_000_000 > 900) {
                    peak = maxOf(dOut, peak - 30f * dt)
                }
            }
        }
    }

    Column {
        Canvas(
            Modifier.fillMaxWidth().height(96.dp)
                .pointerInput(enabled) {
                    if (!enabled) return@pointerInput
                    detectTapGestures { setThreshold(it.x, size.width.toFloat()) }
                }
                .pointerInput(enabled) {
                    if (!enabled) return@pointerInput
                    detectDragGestures(onDragStart = { setThreshold(it.x, size.width.toFloat()) }) { change, _ ->
                        setThreshold(change.position.x, size.width.toFloat())
                    }
                },
        ) { drawMeter(dIn, dOut, peak, thrNow) }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("${stringResource(R.string.meter_in)} ${fmtDb(dIn)}", fontSize = 15.sp, fontWeight = FontWeight.Bold)
            Text(stringResource(if (meterState.gate) R.string.gate_open else R.string.gate_closed), color = Mute, fontSize = 15.sp)
            Text("${stringResource(R.string.meter_out)} ${fmtDb(dOut)}", fontSize = 15.sp, fontWeight = FontWeight.Bold)
        }
        Text(
            "${stringResource(R.string.lat_pre)} ${if (meterState.latency > 0) meterState.latency.toString() else "—"} ${stringResource(R.string.lat_post)}",
            color = Mute, fontSize = 13.sp,
        )
    }
}

private fun setThreshold(x: Float, width: Float) {
    if (width <= 0f) return
    val db = DB_MIN + (x / width) * -DB_MIN
    Engine.setParam("thr", db.coerceIn(-72f, -20f).roundToInt().toDouble())
}

private fun DrawScope.drawMeter(dIn: Float, dOut: Float, peak: Float, thr: Float) {
    val w = size.width
    val top = 26.dp.toPx()
    val bh = size.height - top - 20.dp.toPx()
    val sw = w / SEGMENTS
    val step = -DB_MIN / SEGMENTS
    for (i in 0 until SEGMENTS) {
        val db = DB_MIN + (i + 1) * step
        val c = when {
            db <= dOut -> if (db > -3f) Warn else Sig
            db <= dIn -> Ghost
            else -> Off
        }
        drawRoundRect(c, Offset(i * sw + 1f, top), Size(maxOf(1f, sw - 2f), bh), CornerRadius(2.dp.toPx()))
    }
    fun xOf(db: Float) = ((db.coerceIn(DB_MIN, 0f) - DB_MIN) / -DB_MIN) * w
    if (peak > DB_MIN) {
        drawRect(if (peak > -3f) Warn else Ink, Offset((xOf(peak) - 1f).coerceIn(0f, w - 2f), top - 4.dp.toPx()), Size(2f, bh + 8.dp.toPx()))
    }
    val tx = xOf(thr)
    drawRect(Color.White, Offset(tx - 1f, top - 8.dp.toPx()), Size(2f, bh + 16.dp.toPx()))
    val tri = Path().apply {
        moveTo(tx - 6.dp.toPx(), top - 16.dp.toPx()); lineTo(tx + 6.dp.toPx(), top - 16.dp.toPx()); lineTo(tx, top - 8.dp.toPx()); close()
    }
    drawPath(tri, Color.White)
}

// ------------------------------------------------------------------ controls

@Composable
private fun Controls() {
    val params by Engine.params.collectAsState()
    val preset by Engine.preset.collectAsState()
    val profiles by Engine.profiles.collectAsState()
    val profile by Engine.profile.collectAsState()
    val ptt by Engine.ptt.collectAsState()
    val p = params ?: return

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            PRESETS.forEach { (key, res) ->
                FilterChip(selected = preset == key, onClick = { Engine.choosePreset(key) }, label = { Text(stringResource(res)) }, colors = chipColors())
            }
            FilterChip(
                selected = p["mute"] == true, onClick = { Engine.setParam("mute", p["mute"] != true) },
                label = { Text(stringResource(R.string.chip_mute)) },
                colors = FilterChipDefaults.filterChipColors(selectedContainerColor = Color(0xFF3A1A1A), selectedLabelColor = Warn, labelColor = Ink),
            )
        }
        if (profiles.isNotEmpty()) {
            Text(stringResource(R.string.profiles), color = Mute, fontSize = 14.sp)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                profiles.forEach { n ->
                    FilterChip(selected = profile == n, onClick = { Engine.chooseProfile(n) }, label = { Text(n) }, colors = chipColors())
                }
            }
        }
        SwitchRow(stringResource(R.string.ptt_title), stringResource(R.string.ptt_hint), ptt) { Engine.enablePtt(it) }

        SECTIONS.forEach { sec ->
            HorizontalLine()
            Text(stringResource(sec.title), fontSize = 20.sp, fontWeight = FontWeight.ExtraBold)
            Text(stringResource(sec.lead), color = Mute, fontSize = 14.sp)
            sec.items.forEach { c ->
                when (c) {
                    is SwitchCtl -> SwitchRow(
                        stringResource(c.label), c.hint?.let { stringResource(it) }, p[c.key] == true,
                    ) { Engine.setParam(c.key, it) }
                    is SliderCtl -> {
                        val value = ((p[c.key] as? Double) ?: c.min.toDouble()).roundToInt()
                        SliderRow(c, value) { Engine.setParam(c.key, it.toDouble()) }
                        if (c.key == "thr") CalibrateRow(value)
                    }
                }
            }
        }
    }
}

@Composable
private fun HorizontalLine() {
    Box(Modifier.fillMaxWidth().height(1.dp).background(Line))
}

@Composable
private fun SwitchRow(label: String, hint: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f)) {
            Text(label, fontSize = 16.sp)
            if (hint != null) Text(hint, color = Mute, fontSize = 13.sp)
        }
        Switch(
            checked = checked, onCheckedChange = onChange,
            colors = SwitchDefaults.colors(
                checkedTrackColor = Sig, checkedThumbColor = Color(0xFF04201B), checkedBorderColor = Sig,
                uncheckedTrackColor = Off, uncheckedThumbColor = Mute, uncheckedBorderColor = Ghost,
            ),
        )
    }
}

@Composable
private fun SliderRow(c: SliderCtl, value: Int, onChange: (Int) -> Unit) {
    val unit = c.unitRes?.let { stringResource(it) } ?: c.unit
    val bipolar = c.min < 0 && c.max > 0
    Column {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(stringResource(c.label), fontSize = 16.sp)
            Text(fmtValue(value, unit, bipolar), color = Sig, fontSize = 15.sp, fontWeight = FontWeight.Bold)
        }
        Slider(
            value = value.toFloat(), onValueChange = { onChange(it.roundToInt()) },
            valueRange = c.min.toFloat()..c.max.toFloat(),
            colors = SliderDefaults.colors(thumbColor = Sig, activeTrackColor = Sig, inactiveTrackColor = Ghost),
        )
        c.hint?.let { Text(stringResource(it), color = Mute, fontSize = 13.sp) }
    }
}

/** Auto threshold: stay silent for 3 seconds and the PC measures the background noise. */
@Composable
private fun CalibrateRow(thr: Int) {
    val m by Engine.meter.collectAsState()
    when (m.cal) {
        1 -> Column {
            Text(stringResource(R.string.cal_measuring), fontSize = 14.sp)
            Box(Modifier.fillMaxWidth().padding(top = 6.dp).height(6.dp).clip(RoundedCornerShape(3.dp)).background(Ghost)) {
                Box(Modifier.fillMaxWidth(m.calProgress.coerceIn(0f, 1f)).height(6.dp).background(Sig))
            }
        }
        2 -> Column {
            Text(stringResource(R.string.cal_done, fmtDb(m.calNoise), fmtDb(thr.toFloat())), fontSize = 14.sp)
            if (m.calNoise > -35f) Text(stringResource(R.string.cal_noisy), color = Warn, fontSize = 13.sp)
        }
        3 -> Text(stringResource(R.string.cal_fail), color = Warn, fontSize = 14.sp)
        else -> OutlinedButton(onClick = { Engine.calibrate() }, modifier = Modifier.fillMaxWidth()) {
            Text(stringResource(R.string.cal_btn), color = Ink)
        }
    }
}

// ------------------------------------------------------------------ footer and battery saver

@Composable
private fun Footer(
    running: Boolean, hasConnection: Boolean, ptt: Boolean,
    onStart: () -> Unit, onStop: () -> Unit, onScan: () -> Unit, onSaver: () -> Unit,
) {
    Column(Modifier.fillMaxWidth().background(Bg).padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (ptt) {
            Box(
                Modifier.fillMaxWidth().height(64.dp).clip(RoundedCornerShape(16.dp)).background(Off).border(1.5.dp, Line, RoundedCornerShape(16.dp))
                    .pointerInput(Unit) {
                        detectTapGestures(onPress = {
                            Engine.setMuteNow(false)
                            tryAwaitRelease()
                            Engine.setMuteNow(true)
                        })
                    },
                contentAlignment = Alignment.Center,
            ) {
                val talking = (Engine.params.collectAsState().value?.get("mute") as? Boolean) == false
                Text(stringResource(if (talking) R.string.talk_on else R.string.talk_off), fontSize = 18.sp, fontWeight = FontWeight.Bold, color = if (talking) Sig else Ink)
            }
        }
        if (running && !ptt) {
            OutlinedButton(onClick = onSaver, modifier = Modifier.fillMaxWidth().height(44.dp), shape = RoundedCornerShape(14.dp)) {
                Text(stringResource(R.string.saver_btn), color = Mute)
            }
        }
        Button(
            onClick = if (running) onStop else if (hasConnection) onStart else onScan,
            modifier = Modifier.fillMaxWidth().height(60.dp), shape = RoundedCornerShape(16.dp),
            colors = ButtonDefaults.buttonColors(containerColor = Sig, contentColor = Color(0xFF04201B)),
        ) {
            Text(
                stringResource(if (running) R.string.go_stop else if (hasConnection) R.string.go_start else R.string.scan_btn),
                fontSize = 20.sp, fontWeight = FontWeight.Bold,
            )
        }
    }
}

/** Battery saver: all black (OLED pixels off), brightness at minimum, system bars hidden. Double-tap to leave. */
@Composable
private fun SaverScreen(onExit: () -> Unit) {
    val activity = LocalContext.current as? Activity
    var hint by remember { mutableStateOf(true) }
    LaunchedEffect(Unit) { delay(3500); hint = false }
    BackHandler(onBack = onExit)
    DisposableEffect(activity) {
        val window = activity?.window
        val controller = window?.let { WindowCompat.getInsetsController(it, it.decorView) }
        controller?.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        controller?.hide(WindowInsetsCompat.Type.systemBars())
        window?.attributes = window?.attributes?.also { it.screenBrightness = 0.01f }
        onDispose {
            controller?.show(WindowInsetsCompat.Type.systemBars())
            window?.attributes = window?.attributes?.also { it.screenBrightness = WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE }
        }
    }
    Box(
        Modifier.fillMaxSize().background(Color.Black).pointerInput(Unit) { detectTapGestures(onDoubleTap = { onExit() }) },
        contentAlignment = Alignment.BottomCenter,
    ) {
        if (hint) Text(stringResource(R.string.saver_hint), color = Color(0xFF3A3F44), fontSize = 14.sp, textAlign = TextAlign.Center, modifier = Modifier.padding(bottom = 40.dp))
    }
}
