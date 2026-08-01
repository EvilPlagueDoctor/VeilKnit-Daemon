package com.example.veilknit_mailer

import android.content.Context
import org.json.JSONObject

class NicknameStore(context: Context) {
    private val preferences = context.getSharedPreferences("veilknit_mailer_names", Context.MODE_PRIVATE)

    fun load(): Map<String, String> {
        val raw = preferences.getString(KEY, "{}") ?: "{}"
        val objectValue = runCatching { JSONObject(raw) }.getOrElse { JSONObject() }
        val values = mutableMapOf<String, String>()
        val keys = objectValue.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            val value = objectValue.optString(key).trim()
            if (value.isNotEmpty()) values[key] = value
        }
        return values
    }

    fun save(values: Map<String, String>) {
        val objectValue = JSONObject()
        values.forEach { (key, value) -> if (value.isNotBlank()) objectValue.put(key, value.trim()) }
        preferences.edit().putString(KEY, objectValue.toString()).apply()
    }

    companion object { private const val KEY = "nicknames" }
}
