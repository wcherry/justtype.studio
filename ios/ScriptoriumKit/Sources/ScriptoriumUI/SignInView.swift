import SwiftUI

struct SignInView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var registering = false
    @State private var email = ""
    @State private var password = ""
    @State private var displayName = ""
    @State private var busy = false

    var body: some View {
        VStack(spacing: 20) {
            VStack(spacing: 4) {
                Text("Scriptorium")
                    .font(.system(.largeTitle, design: .serif))
                Text("Write it. API.Version it. Put it in front of readers.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            VStack(spacing: 12) {
                if registering {
                    TextField("Display name", text: $displayName)
                        .textContentType(.name)
                }
                TextField("Email", text: $email)
                    .textContentType(.emailAddress)
                    #if os(iOS)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    #endif
                SecureField("Password", text: $password)
                    .textContentType(registering ? .newPassword : .password)
            }
            .textFieldStyle(.roundedBorder)

            if let error = session.errorMessage {
                Text(error).font(.footnote).foregroundStyle(.red)
            }

            Button(registering ? "Create account" : "Sign in") {
                busy = true
                Task {
                    if registering {
                        await session.register(email: email, password: password, displayName: displayName)
                    } else {
                        await session.signIn(email: email, password: password)
                    }
                    busy = false
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(busy || email.isEmpty || password.count < 8)

            Button(registering ? "Already have an account?" : "Need an account?") {
                registering.toggle()
            }
            .font(.footnote)
        }
        .padding(28)
        .frame(maxWidth: 420)
    }
}
