package app.larmamic

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.core.os.LocaleListCompat
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions

class MainActivity : AppCompatActivity() {
    private lateinit var store: Store

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Engine.init(this)
        store = Store(this)
        val dark = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
        enableEdgeToEdge(statusBarStyle = dark, navigationBarStyle = dark)

        setContent {
            var conn by remember { mutableStateOf(store.connection) }
            var language by remember { mutableStateOf(store.language) }
            var inputError by remember { mutableStateOf<Msg?>(null) }
            val pcLang by Engine.pcLang.collectAsState()

            // Follow the PC language unless the user picked one here
            LaunchedEffect(language, pcLang) {
                val code = if (language == "auto") pcLang else language
                val tags = if (code == null) LocaleListCompat.getEmptyLocaleList() else LocaleListCompat.forLanguageTags(code)
                if (AppCompatDelegate.getApplicationLocales() != tags) AppCompatDelegate.setApplicationLocales(tags)
            }

            val startStream = {
                ContextCompat.startForegroundService(
                    this, Intent(this, MicService::class.java).setAction(MicService.ACTION_START),
                )
            }
            val permissions = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { grants ->
                if (grants[Manifest.permission.RECORD_AUDIO] == true || hasMic()) startStream()
                else inputError = Msg(R.string.perm_mic)
            }
            val begin = {
                inputError = null
                val needed = buildList {
                    if (!hasMic()) add(Manifest.permission.RECORD_AUDIO)
                    if (Build.VERSION.SDK_INT >= 33 &&
                        ContextCompat.checkSelfPermission(this@MainActivity, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                    ) add(Manifest.permission.POST_NOTIFICATIONS)
                }
                if (needed.isEmpty()) startStream() else permissions.launch(needed.toTypedArray())
            }

            val accept = { text: String, fresh: Boolean ->
                val c = Connection.parse(text)
                if (c == null) {
                    inputError = Msg(R.string.addr_invalid)
                } else {
                    if (fresh) Engine.forgetPin(c)
                    store.connection = c
                    conn = c
                    inputError = null
                    begin()
                }
            }
            val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
                result.contents?.let { accept(it, true) }
            }
            val scan = {
                scanner.launch(
                    ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setBeepEnabled(false)
                        .setOrientationLocked(false).setCaptureActivity(PortraitCaptureActivity::class.java)
                        .setPrompt(getString(R.string.scan_prompt)),
                )
            }

            LarmaApp(
                connection = conn,
                inputError = inputError,
                language = language,
                onLanguage = { code -> store.language = code; language = code },
                onMode = { m -> Engine.setMode(m) },
                onStart = {
                    if (conn == null) inputError = Msg(R.string.err_addr) else begin()
                },
                onStop = { startService(Intent(this, MicService::class.java).setAction(MicService.ACTION_STOP)) },
                onScan = { scan() },
                onAddress = { accept(it, true) },
                onForget = { c -> Engine.forgetPin(c); store.connection = null; conn = null },
            )
        }
    }

    private fun hasMic() =
        ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
}
