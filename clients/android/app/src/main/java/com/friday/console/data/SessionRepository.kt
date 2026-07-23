package com.friday.console.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.dataStore by preferencesDataStore(name = "friday_session")

object SessionKeys {
    val TOKEN = stringPreferencesKey("token")
    val SERVER_URL = stringPreferencesKey("server_url")
}

/**
 * Persists the pairing token and the PC's WebSocket URL (DataStore). Once paired,
 * the launcher never asks again unless the token is revoked.
 */
class SessionRepository(private val context: Context) {
    val token: Flow<String?> = context.dataStore.data.map { it[SessionKeys.TOKEN] }
    val serverUrl: Flow<String?> = context.dataStore.data.map { it[SessionKeys.SERVER_URL] }

    suspend fun saveToken(value: String) {
        context.dataStore.edit { it[SessionKeys.TOKEN] = value }
    }

    suspend fun clearToken() {
        context.dataStore.edit { it.remove(SessionKeys.TOKEN) }
    }

    suspend fun saveServerUrl(value: String) {
        context.dataStore.edit { it[SessionKeys.SERVER_URL] = value }
    }
}
