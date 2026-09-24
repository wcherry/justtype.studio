import Foundation

/// The copy of the manuscript that lives on the phone.
///
/// Not a cache in the "nice to have" sense. A writer on a plane has opened
/// this app to write, and an app that shows them a spinner has failed at the
/// only thing it was for. Everything the writer has looked at is on disk, and
/// everything they type goes to disk before it goes anywhere else.
///
/// Files rather than a database: the shapes are small, they are read whole and
/// never queried, and the server stores manuscripts the same way for the same
/// reason.
public actor LocalStore {
    private let directory: URL
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    public init(directory: URL? = nil) {
        self.directory = directory ?? LocalStore.defaultDirectory()
        try? FileManager.default.createDirectory(
            at: self.directory,
            withIntermediateDirectories: true
        )
    }

    private static func defaultDirectory() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return base.appendingPathComponent("Scriptorium/Offline", isDirectory: true)
    }

    // MARK: - Reading and writing

    public func works() -> [API.Work]? { read("works") }

    public func putWorks(_ works: [API.Work]) { write(works, to: "works") }

    public func outline(workId: String) -> API.OutlineResponse? {
        read("outline-\(safe(workId))")
    }

    public func putOutline(_ outline: API.OutlineResponse) {
        write(outline, to: "outline-\(safe(outline.id))")
    }

    public func scene(id: String) -> API.SceneWithContent? { read("scene-\(safe(id))") }

    public func putScene(_ scene: API.SceneWithContent) {
        write(scene, to: "scene-\(safe(scene.id))")
    }

    /// Everything, for signing out. A shared phone should not keep somebody
    /// else's unpublished novel on it.
    public func clear() {
        try? FileManager.default.removeItem(at: directory)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    // MARK: - Plumbing

    /// Ids come from the server, but a file path is not the place to find out
    /// they were wrong: anything but the characters an id is made of goes.
    private func safe(_ id: String) -> String {
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_"))
        return String(id.unicodeScalars.map { allowed.contains($0) ? Character($0) : "-" })
    }

    private func url(_ name: String) -> URL {
        directory.appendingPathComponent("\(name).json")
    }

    private func read<T: Decodable>(_ name: String) -> T? {
        guard let data = try? Data(contentsOf: url(name)) else { return nil }
        return try? decoder.decode(T.self, from: data)
    }

    /// Write to a neighbour and rename. A half-written scene is worse than no
    /// scene: the rename is the only atomic step a filesystem offers.
    private func write<T: Encodable>(_ value: T, to name: String) {
        guard let data = try? encoder.encode(value) else { return }
        let destination = url(name)
        let temporary = destination.appendingPathExtension("writing")
        do {
            try data.write(to: temporary, options: .atomic)
            _ = try FileManager.default.replaceItemAt(destination, withItemAt: temporary)
        } catch {
            try? FileManager.default.removeItem(at: temporary)
        }
    }
}
