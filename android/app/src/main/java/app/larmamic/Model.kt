package app.larmamic

import androidx.annotation.StringRes

enum class Status { IDLE, CONNECTING, LIVE, RETRY, ERROR }

/** A message shown under the title: a string resource plus optional format arguments. */
data class Msg(@StringRes val res: Int, val args: List<String> = emptyList())

/** Level data the PC sends about 15 times per second. */
data class Meter(
    val inDb: Float = -99f,
    val outDb: Float = -99f,
    val gate: Boolean = true,
    val buf: Int = 0,
    val under: Int = 0,
    val drops: Int = 0,
    /** Auto threshold: 0 idle, 1 measuring, 2 done, 3 failed. */
    val cal: Int = 0,
    val calProgress: Float = 0f,
    val calNoise: Float = -99f,
    val latency: Int = 0,
)

/** One slider or switch. Keys match `Params` in src/settings.rs. */
sealed interface Ctl {
    val key: String
    @get:StringRes val label: Int
    @get:StringRes val hint: Int?
}

data class SwitchCtl(override val key: String, override val label: Int, override val hint: Int? = null) : Ctl

data class SliderCtl(
    override val key: String,
    override val label: Int,
    override val hint: Int? = null,
    val min: Int,
    val max: Int,
    val unit: String = "",
    @StringRes val unitRes: Int? = null,
) : Ctl

data class Section(@StringRes val title: Int, @StringRes val lead: Int, val items: List<Ctl>)

val PRESETS = listOf(
    "voice" to R.string.preset_voice,
    "game" to R.string.preset_game,
    "podcast" to R.string.preset_podcast,
    "raw" to R.string.preset_raw,
)

val SECTIONS = listOf(
    Section(
        R.string.sec_noise, R.string.sec_noise_lead,
        listOf(
            SwitchCtl("ai", R.string.ctl_ai),
            SliderCtl("ai_mix", R.string.ctl_ai_mix, R.string.ctl_ai_mix_hint, 0, 100, "%"),
            SwitchCtl("gate_on", R.string.ctl_gate, R.string.ctl_gate_hint),
            SliderCtl("thr", R.string.ctl_thr, R.string.ctl_thr_hint, -72, -20, " dB"),
            SliderCtl("depth", R.string.ctl_depth, R.string.ctl_depth_hint, 6, 80, " dB"),
            SliderCtl("hp", R.string.ctl_hp, R.string.ctl_hp_hint, 40, 250, " Hz"),
            SwitchCtl("aec", R.string.ctl_aec, R.string.ctl_aec_hint),
        ),
    ),
    Section(
        R.string.sec_eq, R.string.sec_eq_lead,
        listOf(
            SliderCtl("low", R.string.ctl_low, R.string.ctl_low_hint, -12, 12, " dB"),
            SliderCtl("mud", R.string.ctl_mud, R.string.ctl_mud_hint, -12, 12, " dB"),
            SliderCtl("pres", R.string.ctl_pres, R.string.ctl_pres_hint, -12, 12, " dB"),
            SliderCtl("air", R.string.ctl_air, R.string.ctl_air_hint, -12, 12, " dB"),
        ),
    ),
    Section(
        R.string.sec_vol, R.string.sec_vol_lead,
        listOf(
            SwitchCtl("agc", R.string.ctl_agc, R.string.ctl_agc_hint),
            SliderCtl("comp", R.string.ctl_comp, R.string.ctl_comp_hint, 0, 100, "%"),
            SliderCtl("gain", R.string.ctl_gain, null, -12, 24, " dB"),
            SliderCtl("buf_ms", R.string.ctl_buf, R.string.ctl_buf_hint, 30, 300, " ms"),
        ),
    ),
    Section(
        R.string.sec_fx, R.string.sec_fx_lead,
        listOf(
            SliderCtl("fx_pitch", R.string.ctl_pitch, R.string.ctl_pitch_hint, -12, 12, unitRes = R.string.unit_semitones),
            SliderCtl("fx_reverb", R.string.ctl_reverb, null, 0, 100, "%"),
            SliderCtl("fx_echo", R.string.ctl_echo, null, 0, 100, "%"),
            SliderCtl("fx_chorus", R.string.ctl_chorus, null, 0, 100, "%"),
            SliderCtl("fx_dist", R.string.ctl_dist, null, 0, 100, "%"),
            SwitchCtl("fx_robot", R.string.ctl_robot),
        ),
    ),
)

/** "−12 dB", "+3 dB", "55%" */
fun fmtValue(v: Int, unit: String, signed: Boolean): String = when {
    v < 0 -> "−${-v}$unit"
    else -> "${if (signed && v > 0) "+" else ""}$v$unit"
}

fun fmtDb(v: Float): String = if (v <= -90f) "−∞" else fmtValue(Math.round(v), " dB", false)

/** Language codes shown in the picker: code, native name. */
val LANGUAGES = listOf(
    "vi" to "Tiếng Việt",
    "en" to "English",
    "zh" to "中文 (简体)",
    "ja" to "日本語",
    "ko" to "한국어",
    "id" to "Bahasa Indonesia",
    "es" to "Español",
    "fr" to "Français",
    "de" to "Deutsch",
    "ru" to "Русский",
)
