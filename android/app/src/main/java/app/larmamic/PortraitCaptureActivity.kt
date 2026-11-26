package app.larmamic

import com.journeyapps.barcodescanner.CaptureActivity

/** QR scanner locked to portrait (the library's default screen is landscape). Declared in the manifest with screenOrientation="portrait". */
class PortraitCaptureActivity : CaptureActivity()
