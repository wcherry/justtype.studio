import SwiftUI

/// What the phone knows that the server does not.
///
/// Shown rather than hidden. An app that quietly swallows a failed save is
/// telling the writer their words are safe when they are on one device with a
/// flat battery — and the fix is a sentence, not a spinner.
struct OfflineNotice: View {
    let offline: Bool
    let pending: Int

    var body: some View {
        Label {
            Text(message)
        } icon: {
            Image(systemName: offline ? "wifi.slash" : "arrow.triangle.2.circlepath")
        }
        .font(.footnote)
        .foregroundStyle(.secondary)
        .listRowBackground(Color.yellow.opacity(0.12))
    }

    private var message: String {
        switch (offline, pending) {
        case (true, 0):
            "Offline. This is the copy on your phone."
        case (true, let count):
            "Offline. \(count) \(count == 1 ? "scene" : "scenes") waiting to go up."
        case (false, let count) where count > 0:
            "\(count) \(count == 1 ? "scene" : "scenes") still going up."
        default:
            ""
        }
    }
}
