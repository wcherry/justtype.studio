package studio.justtype.core.api

/** Why a request did not come back with what was asked for. Mirrors iOS's `APIError`. */
sealed class ApiError(message: String) : Exception(message) {
    /** The scene changed elsewhere since it was loaded. Reload before saving. */
    class Conflict(message: String) : ApiError(message)

    class Http(val status: Int, val code: String, message: String) : ApiError(message)

    /** No answer at all: no network, a dead server, a tunnel. */
    class Transport(message: String) : ApiError(message)

    class Decoding(detail: String) : ApiError("Unexpected response: $detail")

    class NotAuthenticated : ApiError("You are signed out.")
}
