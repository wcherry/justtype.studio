package studio.justtype.core.api

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** Where the session lives between launches. On a phone, sealed with a Keystore key. */
interface TokenStore {
    fun read(key: String): String?
    fun write(key: String, value: String)
    fun delete(key: String)
}

/** In memory, for tests. */
class MemoryTokenStore : TokenStore {
    private val values = mutableMapOf<String, String>()
    @Synchronized override fun read(key: String) = values[key]
    @Synchronized override fun write(key: String, value: String) { values[key] = value }
    @Synchronized override fun delete(key: String) { values.remove(key) }
}

/** Holds the token pair and makes sure a burst of 401s produces one refresh, not several. */
class TokenManager(private val store: TokenStore) {
    private val mutex = Mutex()

    val accessToken: String? get() = store.read(ACCESS)
    val refreshToken: String? get() = store.read(REFRESH)
    val isSignedIn: Boolean get() = refreshToken != null

    fun save(session: API.SessionResponse) {
        store.write(ACCESS, session.accessToken)
        store.write(REFRESH, session.refreshToken)
    }

    fun clear() {
        store.delete(ACCESS)
        store.delete(REFRESH)
    }

    /**
     * Refreshes once for everyone who failed with [stale]. Tokens are cleared only when [perform]
     * returns null (the server refused); if it throws, they are kept and the error goes up. A caller arriving after another has
     * already swapped the pair finds a different access token and goes straight back to retry.
     */
    suspend fun refresh(stale: String?, perform: suspend (String) -> API.SessionResponse?): Boolean =
        mutex.withLock {
            val current = accessToken
            if (current != null && current != stale) return@withLock true
            val token = refreshToken ?: return@withLock false
            val session = perform(token)
            if (session == null) {
                clear()
                false
            } else {
                save(session)
                true
            }
        }

    private companion object {
        const val ACCESS = "accessToken"
        const val REFRESH = "refreshToken"
    }
}
