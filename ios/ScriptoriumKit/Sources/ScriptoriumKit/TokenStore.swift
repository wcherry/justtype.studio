import Foundation
#if canImport(Security)
import Security
#endif

/// Where the session lives between launches.
public protocol TokenStoring: Sendable {
    func read(_ key: String) -> String?
    func write(_ value: String, for key: String)
    func delete(_ key: String)
}

/// Refresh tokens are long-lived credentials, so they belong in the Keychain
/// rather than UserDefaults.
public struct KeychainTokenStore: TokenStoring {
    private let service: String

    public init(service: String = "app.scriptorium.tokens") {
        self.service = service
    }

    #if canImport(Security)
    private func query(_ key: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
    }

    public func read(_ key: String) -> String? {
        var query = query(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    public func write(_ value: String, for key: String) {
        let data = Data(value.utf8)
        SecItemDelete(query(key) as CFDictionary)
        var attributes = query(key)
        attributes[kSecValueData as String] = data
        attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(attributes as CFDictionary, nil)
    }

    public func delete(_ key: String) {
        SecItemDelete(query(key) as CFDictionary)
    }
    #else
    public func read(_ key: String) -> String? { nil }
    public func write(_ value: String, for key: String) {}
    public func delete(_ key: String) {}
    #endif
}

/// In-memory store for tests and previews.
public final class MemoryTokenStore: TokenStoring, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: String] = [:]

    public init() {}

    public func read(_ key: String) -> String? {
        lock.lock(); defer { lock.unlock() }
        return values[key]
    }

    public func write(_ value: String, for key: String) {
        lock.lock(); defer { lock.unlock() }
        values[key] = value
    }

    public func delete(_ key: String) {
        lock.lock(); defer { lock.unlock() }
        values.removeValue(forKey: key)
    }
}
