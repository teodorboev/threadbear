package com.mikerosoft.threadbear.android.ui

import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.LocalSnackbar
import com.mikerosoft.threadbear.android.R

/**
 * Pairing: scan the code the Mac shows (Settings > Phone & browser), or
 * paste a pairing link from this phone's own clipboard. Either is asked
 * about before anything is kept, and checked with phone:me.
 */
@Composable
fun PairScreen() {
    val model = LocalModel.current
    val context = LocalContext.current
    val busy by model.pairingBusy.collectAsStateWithLifecycle()

    Scaffold(snackbarHost = { SnackbarHost(LocalSnackbar.current) }) { inner ->
        Box(Modifier.fillMaxSize().padding(inner).safeDrawingPadding(), contentAlignment = Alignment.Center) {
            Column(
                Modifier.widthIn(max = 360.dp).fillMaxWidth().padding(horizontal = 32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Image(
                    painterResource(R.drawable.ic_launcher_fg), contentDescription = null,
                    modifier = Modifier.size(96.dp).clip(RoundedCornerShape(24.dp)),
                )
                Spacer(Modifier.height(8.dp))
                Text("Threadbear", style = MaterialTheme.typography.headlineSmall)
                Text(
                    "Pair with the code on your Mac, in Settings, Phone & browser.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.height(16.dp))
                if (busy) {
                    CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 2.dp)
                } else {
                    Button(
                        onClick = {
                            scan(context, failed = { model.note(it) }) { if (!model.ask(it)) model.note("That code isn't a Threadbear pairing link.") }
                        },
                        modifier = Modifier.fillMaxWidth().height(52.dp),
                        contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                    ) {
                        Icon(Line.Scan, null, Modifier.size(20.dp))
                        Spacer(Modifier.size(10.dp))
                        Text("Scan the code")
                    }
                    OutlinedButton(
                        onClick = {
                            val text = clipboardText(context)
                            if (text == null || !model.ask(text)) model.note("There's no pairing link on the clipboard.")
                        },
                        modifier = Modifier.fillMaxWidth().height(52.dp),
                    ) {
                        Icon(Line.Paste, null, Modifier.size(20.dp))
                        Spacer(Modifier.size(10.dp))
                        Text("Paste a pairing link")
                    }
                }
            }
        }
    }
}

private fun scan(context: Context, failed: (String) -> Unit, found: (String) -> Unit) {
    val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
    GmsBarcodeScanning.getClient(context, options).startScan()
        .addOnSuccessListener { code -> code.rawValue?.let(found) }
        .addOnFailureListener { failed("Couldn't open the scanner: ${it.message}") }
}

private fun clipboardText(context: Context): String? {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    val clip = clipboard.primaryClip ?: return null
    if (clip.itemCount == 0) return null
    return clip.getItemAt(0).coerceToText(context)?.toString()?.trim()
}
