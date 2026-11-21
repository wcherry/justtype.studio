package studio.justtype.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import studio.justtype.app.graph

@Composable
fun SignInScreen() {
    val graph = LocalContext.current.graph
    val session = graph.session
    val scope = rememberCoroutineScope()
    val error by session.error.collectAsState()
    val needsCode by session.needsCode.collectAsState()

    var registering by rememberSaveable { mutableStateOf(false) }
    var email by rememberSaveable { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var displayName by rememberSaveable { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var acceptTerms by rememberSaveable { mutableStateOf(false) }
    var showTerms by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }

    val ready = !busy && email.isNotBlank() && password.length >= 8 && (!registering || (acceptTerms && displayName.isNotBlank()))
    fun submit() {
        if (!ready) return
        busy = true
        scope.launch {
            if (registering) session.register(email.trim(), password, displayName.trim(), acceptTerms)
            else session.signIn(email.trim(), password, if (needsCode) code.trim() else null)
            busy = false
        }
    }

    Box(Modifier.fillMaxSize().safeDrawingPadding().imePadding(), contentAlignment = Alignment.Center) {
        Column(
            Modifier.widthIn(max = 420.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(28.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("Just Type Studio", style = MaterialTheme.typography.headlineLarge)
            Text(
                "Write it. Version it. Put it in front of readers.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            VSpace(8)
            if (registering) {
                OutlinedTextField(displayName, { displayName = it }, label = { Text("Display name") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            }
            OutlinedTextField(
                email, { email = it }, label = { Text("Email") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next),
            )
            OutlinedTextField(
                password, { password = it }, label = { Text("Password") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = if (needsCode && !registering) ImeAction.Next else ImeAction.Go),
                keyboardActions = androidx.compose.foundation.text.KeyboardActions(onGo = { submit() }),
            )
            if (needsCode && !registering) {
                OutlinedTextField(
                    code, { code = it }, label = { Text("Code from your authenticator") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                    supportingText = { Text("Or one of your recovery codes.") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Go),
                    keyboardActions = androidx.compose.foundation.text.KeyboardActions(onGo = { submit() }),
                )
            }
            if (registering) {
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                    Checkbox(acceptTerms, { acceptTerms = it })
                    Text("I agree to the terms")
                }
                TextButton(onClick = { showTerms = true }) { Text("Read them — they say my writing stays mine") }
            }
            if (error != null) Text(error!!, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            Button(onClick = ::submit, enabled = ready, modifier = Modifier.fillMaxWidth()) {
                Text(if (registering) "Create account" else "Sign in")
            }
            TextButton(onClick = { registering = !registering; session.clearError() }) {
                Text(if (registering) "Already have an account?" else "Need an account?")
            }
        }
    }

    if (showTerms) TermsDialog(onDismiss = { showTerms = false })
}

/** The terms, as the server serves them, so the phone shows the same words the browser does. */
@Composable
private fun TermsDialog(onDismiss: () -> Unit) {
    val client = LocalContext.current.graph.client
    var text by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        text = runCatching { client.terms().text.replace("#", "") }.getOrDefault("Could not load the terms.")
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onDismiss) { Text("Done") } },
        title = { Text("Terms") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(text ?: "Loading…", style = MaterialTheme.typography.bodyMedium.copy(fontFamily = Serif))
            }
        },
    )
}
