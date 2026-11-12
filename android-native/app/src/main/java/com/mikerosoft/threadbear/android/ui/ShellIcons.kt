package com.mikerosoft.threadbear.android.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

// More of Line's thin icons (Lucide's shapes, ISC licence), for the inbox's
// bar, Settings and the Pocket. Kept in their own file, as extensions on
// Line, so Icons.kt stays as other screens left it.

private fun circle(cx: Float, cy: Float, r: Float) = "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0"

private fun icon(name: String, vararg paths: String, fill: Boolean = false): ImageVector {
    val b = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f)
    for (d in paths) {
        b.addPath(
            pathData = addPathNodes(d),
            fill = if (fill) SolidColor(Color.Black) else null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.6f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        )
    }
    return b.build()
}

private val menu by lazy { icon("menu", "M4 6h16", "M4 12h16", "M4 18h16") }
private val squarePen by lazy {
    icon("squarePen", "M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7",
        "M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z")
}
private val wallet by lazy {
    icon("wallet", "M19 7V4a1 1 0 0 0-1-1H5a2 2 0 0 0 0 4h15a1 1 0 0 1 1 1v4h-3a2 2 0 0 0 0 4h3a1 1 0 0 0 1-1v-2a1 1 0 0 0-1-1",
        "M3 5v14a2 2 0 0 0 2 2h15a1 1 0 0 0 1-1v-4")
}
private val settings by lazy {
    icon("settings",
        "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z",
        circle(12f, 12f, 3f))
}
private val trash by lazy {
    icon("trash", "M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2")
}
private val mailOpen by lazy {
    icon("mailOpen", "M21.2 8.4c.5.38.8.97.8 1.6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V10a2 2 0 0 1 .8-1.6l8-6a2 2 0 0 1 2.4 0l8 6Z",
        "m22 10-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 10")
}
private val hash by lazy { icon("hash", "M4 9h16", "M4 15h16", "M10 3 8 21", "M16 3l-2 18") }
private val mapPin by lazy {
    icon("mapPin", "M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0", circle(12f, 10f, 3f))
}
private val messageCircle by lazy { icon("messageCircle", "M7.9 20A9 9 0 1 0 4 16.1L2 22Z") }
private val grip by lazy {
    icon("grip", circle(9f, 5f, 1f), circle(9f, 12f, 1f), circle(9f, 19f, 1f), circle(15f, 5f, 1f), circle(15f, 12f, 1f), circle(15f, 19f, 1f))
}
private val bellOff by lazy {
    icon("bellOff", "M8.7 3A6 6 0 0 1 18 8a21.3 21.3 0 0 0 .6 5", "M17 17H3s3-2 3-9a4.67 4.67 0 0 1 .3-1.7",
        "M10.3 21a1.94 1.94 0 0 0 3.4 0", "m2 2 20 20")
}
private val starOff by lazy {
    icon("starOff", "M8.34 8.34 2 9.27l5 4.87L5.82 21 12 17.77 18.18 21l-.59-3.43", "M18.42 12.76 22 9.27l-6.91-1L12 2l-1.44 2.91", "m2 2 20 20")
}
private val inboxUp by lazy {
    icon("inboxUp", "M22 12h-6l-2 3h-4l-2-3H2", "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89", "M12 2v8", "m9 5 3-3 3 3")
}

// The Pocket's card icons (PocketIcon's names).
private val plane by lazy {
    icon("plane", "M17.8 19.2 16 11l3.5-3.5C21 6 21.5 4 21 3c-1-.5-3 0-4.5 1.5L13 8 4.8 6.2c-.5-.1-.9.1-1.1.5l-.3.5c-.2.5-.1 1 .3 1.3L9 12l-2 3H4l-1 1 3 2 2 3 1-1v-3l3-2 3.5 5.3c.3.4.8.5 1.3.3l.5-.2c.4-.3.6-.7.5-1.2z")
}
private val parcel by lazy {
    icon("parcel", "M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z",
        "M12 22V12", "m3.3 7 7.703 4.734a2 2 0 0 0 1.994 0L20.7 7", "m7.5 4.27 9 5.15")
}
private val bag by lazy { icon("bag", "M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4Z", "M3 6h18", "M16 10a4 4 0 0 1-8 0") }
private val hammer by lazy {
    icon("hammer", "m15 12-8.373 8.373a1 1 0 1 1-3-3L12 9", "m18 15 4-4",
        "m21.5 11.5-1.914-1.914A2 2 0 0 1 19 8.172V7l-2.26-2.26a6 6 0 0 0-4.202-1.756L9 2.96l.92.82A6.18 6.18 0 0 1 12 8.4V10l2 2h1.172a2 2 0 0 1 1.414.586L18.5 14.5")
}
private val utensils by lazy { icon("utensils", "M3 2v7c0 1.1.9 2 2 2h4a2 2 0 0 0 2-2V2", "M7 2v20", "M21 15V2a5 5 0 0 0-5 5v6c0 1.1.9 2 2 2h3Zm0 0v7") }
private val car by lazy {
    icon("car", "M19 17h2c.6 0 1-.4 1-1v-3c0-.9-.7-1.7-1.5-1.9C18.7 10.6 16 10 16 10s-1.3-1.4-2.2-2.3c-.5-.4-1.1-.7-1.8-.7H5c-.6 0-1.1.4-1.4.9l-1.4 2.9A3.7 3.7 0 0 0 2 12v4c0 .6.4 1 1 1h2",
        circle(7f, 17f, 2f), "M9 17h6", circle(17f, 17f, 2f))
}
private val cross by lazy {
    icon("cross", "M4 9a2 2 0 0 0-2 2v2a2 2 0 0 0 2 2h4a1 1 0 0 1 1 1v4a2 2 0 0 0 2 2h2a2 2 0 0 0 2-2v-4a1 1 0 0 1 1-1h4a2 2 0 0 0 2-2v-2a2 2 0 0 0-2-2h-4a1 1 0 0 1-1-1V4a2 2 0 0 0-2-2h-2a2 2 0 0 0-2 2v4a1 1 0 0 1-1 1z")
}
private val ticket by lazy {
    icon("ticket", "M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z", "M13 5v2", "M13 17v2", "M13 11v2")
}
private val card by lazy { icon("card", "M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z", "M2 10h20") }
private val house by lazy {
    icon("house", "M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8",
        "M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z")
}

val Line.Menu: ImageVector get() = menu
val Line.SquarePen: ImageVector get() = squarePen
val Line.Wallet: ImageVector get() = wallet
val Line.Settings: ImageVector get() = settings
val Line.Trash: ImageVector get() = trash
val Line.MailOpen: ImageVector get() = mailOpen
val Line.Hash: ImageVector get() = hash
val Line.MapPin: ImageVector get() = mapPin
val Line.MessageCircle: ImageVector get() = messageCircle
val Line.Grip: ImageVector get() = grip
val Line.BellOff: ImageVector get() = bellOff
val Line.StarOff: ImageVector get() = starOff
val Line.InboxUp: ImageVector get() = inboxUp

/** A Pocket card's icon, by the name its agent chose; the wallet for anything else. */
fun Line.pocket(name: String): ImageVector = when (name) {
    "plane" -> plane
    "parcel" -> parcel
    "shopping" -> bag
    "tools" -> hammer
    "food" -> utensils
    "car" -> car
    "health" -> cross
    "ticket" -> ticket
    "money" -> card
    "home" -> house
    else -> wallet
}
