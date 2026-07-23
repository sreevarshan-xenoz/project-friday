package com.friday.console.network

import kotlinx.serialization.json.Json

/** Shared JSON codec. `encodeDefaults = true` so `Hello.token = null` is sent
 *  explicitly (the server treats a missing field as an error). */
val FridayJson = Json {
    ignoreUnknownKeys = true
    encodeDefaults = true
    isLenient = false
}
