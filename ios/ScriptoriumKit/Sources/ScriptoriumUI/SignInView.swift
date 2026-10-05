import SwiftUI

struct SignInView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var registering = false
    @State private var email = ""
    @State private var password = ""
    @State private var displayName = ""
    @State private var code = ""
    @State private var acceptTerms = false
    @State private var showTerms = false
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
                if session.needsCode && !registering {
                    TextField("Code from your authenticator", text: $code)
                        .textContentType(.oneTimeCode)
                        #if os(iOS)
                        .keyboardType(.numberPad)
                        #endif
                }
            }
            .textFieldStyle(.roundedBorder)

            if registering {
                VStack(alignment: .leading, spacing: 4) {
                    Toggle("I agree to the terms", isOn: $acceptTerms)
                    Button("Read them — they say my writing stays mine") { showTerms = true }
                        .font(.footnote)
                }
            }

            if let error = session.errorMessage {
                Text(error).font(.footnote).foregroundStyle(.red)
            }

            Button(registering ? "Create account" : "Sign in") {
                busy = true
                Task {
                    if registering {
                        await session.register(
                            email: email, password: password, displayName: displayName,
                            acceptTerms: acceptTerms
                        )
                    } else {
                        await session.signIn(
                            email: email, password: password,
                            code: session.needsCode ? code : nil
                        )
                    }
                    busy = false
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(busy || email.isEmpty || password.count < 8 || (registering && !acceptTerms))

            Button(registering ? "Already have an account?" : "Need an account?") {
                registering.toggle()
            }
            .font(.footnote)
        }
        .padding(28)
        .frame(maxWidth: 420)
        .sheet(isPresented: $showTerms) { TermsSheet() }
    }
}

/// The terms, as the server serves them, so the phone shows the same words the
/// browser does.
private struct TermsSheet: View {
    @EnvironmentObject private var session: SessionStore
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""

    var body: some View {
        NavigationStack {
            ScrollView {
                Text(text.isEmpty ? "Loading…" : text)
                    .font(.system(.body, design: .serif))
                    .padding()
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .navigationTitle("Terms")
            .toolbar { Button("Done") { dismiss() } }
        }
        .task {
            text = (try? await session.client.terms().text)
                .map { $0.replacingOccurrences(of: "#", with: "") } ?? "Could not load the terms."
        }
    }
}
