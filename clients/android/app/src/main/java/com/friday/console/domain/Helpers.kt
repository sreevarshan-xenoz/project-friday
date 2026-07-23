package com.friday.console.domain

import java.security.MessageDigest
import java.util.Calendar

/** SHA-256 hex digest (matches the server's `sign_pairing`). */
fun sha256Hex(input: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(input.toByteArray(Charsets.UTF_8))
    return digest.joinToString("") { "%02x".format(it) }
}

/** Signature the phone returns for a pairing challenge: sha256(challenge || code). */
fun signPairing(challenge: String, code: String): String = sha256Hex(challenge + code)

/** Calm, time-of-day greeting for the home screen. */
fun greetingNow(): String {
    val hour = Calendar.getInstance().get(Calendar.HOUR_OF_DAY)
    return when {
        hour < 12 -> "Good morning"
        hour < 18 -> "Good afternoon"
        else -> "Good evening"
    }
}
