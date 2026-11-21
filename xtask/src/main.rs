//! Project automation. `cargo xtask <task>`, or one of the aliases in
//! `.cargo/config.toml`.
//!
//! Everything here is idempotent: no task overwrites a file that already has a
//! real value in it, so re-running after a half-finished setup is safe.

mod codegen;

use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{self, Command},
    thread,
    time::{Duration, Instant},
};

/// Secrets the backend refuses to start without. `cargo xtask setup` generates a
/// value for any of these the repo-root `.env` does not already define with one.
const REQUIRED_ENV_SECRETS: [&str; 1] = ["JUSTTYPE_JWT_SECRET"];
const ENV_PREFIX: &str = "JUSTTYPE_";
const LEGACY_ENV_PREFIX: &str = "SCRIPTORIUM_";

/// Values in `.env.example` that exist only to be replaced. A key set to one of
/// these is treated as unset — `.env.example` ships a JWT secret long enough to
/// pass the backend's length check, which would otherwise mean every clone runs
/// on the same signing key.
const PLACEHOLDER_PREFIXES: [&str; 3] = ["change-me", "changeme", "replace-me"];

/// How long `cargo xtask dev` waits for the backend's health endpoint. Generous
/// because the first `cargo run` of a session compiles the whole server before
/// it binds anything.
const BACKEND_HEALTH_TIMEOUT: Duration = Duration::from_secs(600);

struct Config {
    workspace_root: PathBuf,
    backend_package: String,
    web_dir: PathBuf,
    ios_dir: PathBuf,
    ios_package_dir: PathBuf,
    android_dir: PathBuf,
    xcode_project: String,
    default_bind_addr: String,
    health_path: String,
    openapi_path: PathBuf,
    ts_types_path: PathBuf,
    swift_types_path: PathBuf,
    kotlin_types_path: PathBuf,
}

const USAGE: &str = "\
Usage: cargo xtask <task> [args...]

Tasks:
  setup              Everything a fresh clone needs before dev, test or smoke
  dev                Backend and web dev server together
  build-web          Production build of the web client
  openapi            Write docs/openapi.json from the backend's routes
  codegen [--check]  Regenerate the client types; --check fails on any drift
  ios                Generate the Xcode project and open it
  sim [args...]      Build and run the iOS app (scripts/run_simulator.sh)
  testflight [args]  Build, sign and upload to TestFlight
  android [args]     Build the Android app and install it on the running emulator or phone
                       --api <url>  talk to this server (default: production)
                       --local      the backend on this computer, from the emulator
  test               Rust, Swift and web test suites
  check              fmt, clippy, and the web type-check
  smoke              End-to-end API walkthrough on a throwaway database
  backup             Back up the database and manuscripts, then restore-drill it
  docker [options]   Build the Docker image and run it on http://localhost:8080
                       --port <n>   publish on another port
                       --no-build   run the image last built
                       --detach     run in the background (docker stop justtype-local)
                       --fresh      start from an empty data volume
  admin grant|revoke <email>
                     Make an account a site administrator, or stop it being one
  fmt                Format the Rust sources";

fn main() {
    let mut args = env::args().skip(1);
    let task = args.next().unwrap_or_else(|| {
        eprintln!("{USAGE}");
        process::exit(1);
    });
    let extra: Vec<String> = args.collect();
    let cfg = config_from_metadata();

    match task.as_str() {
        "setup" => setup(&cfg),
        "dev" => run_dev(&cfg),
        "build-web" => build_web(&cfg.web_dir),
        "openapi" => {
            write_openapi(&cfg);
            println!("wrote {}", rel(&cfg.workspace_root, &cfg.openapi_path));
        }
        "codegen" => run_codegen(&cfg, extra.iter().any(|a| a == "--check")),
        "ios" => open_ios(&cfg),
        // Both are shell scripts: they are long, macOS-specific, and deal in
        // xcodebuild/simctl/devicectl invocations that read far better as
        // shell than as Rust building argv vectors. The xtask just forwards.
        "sim" => run_script(&cfg.workspace_root, "scripts/run_simulator.sh", &extra),
        "testflight" => run_script(&cfg.workspace_root, "scripts/deploy_testflight.sh", &extra),
        "android" => run_android(&cfg, &extra),
        "test" => run_tests(&cfg),
        "check" => run_checks(&cfg),
        "smoke" => run_smoke(&cfg.workspace_root, &extra),
        // A backup is only as good as its last restore, so the one command
        // does both; `--drill-only` re-proves the newest.
        "backup" => run_script(&cfg.workspace_root, "scripts/backup.sh", &extra),
        "docker" => run_docker(&cfg, &extra),
        "fmt" => run("cargo", &["fmt", "--all"], &cfg.workspace_root),
        // Never through the API: the server's own `--admin` flag, against the
        // database the environment (or .env) names.
        "admin" => match extra.as_slice() {
            [verb, email] if verb == "grant" || verb == "revoke" => {
                let mut args = vec![
                    "run",
                    "-q",
                    "-p",
                    cfg.backend_package.as_str(),
                    "--",
                    "--admin",
                ];
                args.push(verb);
                args.push(email);
                run("cargo", &args, &cfg.workspace_root);
            }
            _ => {
                eprintln!("Usage: cargo xtask admin grant|revoke <email>");
                process::exit(1);
            }
        },
        other => {
            eprintln!("Unknown task: {other}\n\n{USAGE}");
            process::exit(1);
        }
    }
}

// --- setup ---------------------------------------------------------------

/// Every prerequisite step a fresh clone needs before `cargo dev`, `cargo test`
/// or `cargo smoke` will work.
fn setup(cfg: &Config) {
    let root = &cfg.workspace_root;
    require_tools(&["pnpm", "node", "openssl", "curl"]);

    section("Rust backend");
    ensure_env(root);
    run("cargo", &["fetch"], root);

    section("Web client");
    run("pnpm", &["install"], &cfg.web_dir);

    section("iOS client");
    // Optional: the Swift package builds and tests without it, and only the app
    // target needs a generated project.
    if have_tool("xcodegen") {
        write_signing_xcconfig(root);
        run("xcodegen", &["generate"], &cfg.ios_dir);
        println!(
            "generated {}",
            cfg.ios_dir.join(&cfg.xcode_project).display()
        );
    } else {
        println!("xcodegen not installed — skipping the Xcode project.");
        println!("  `brew install xcodegen`, then `cargo ios`.");
        println!(
            "  The shared package still builds: cd {} && swift test",
            rel(root, &cfg.ios_package_dir)
        );
    }

    section("Setup complete");
    println!("  cargo dev      backend + web dev server");
    println!("  cargo smoke    end-to-end API walkthrough");
    println!("  cargo xtask test");
}

/// Creates `.env` from `.env.example` when it is missing, then makes sure every
/// required secret has a real value. An existing `.env` is edited in place —
/// only placeholder lines are rewritten, so local overrides survive.
fn ensure_env(root: &Path) {
    let path = root.join(".env");
    let example = root.join(".env.example");

    if !path.exists() {
        if example.exists() {
            fs::copy(&example, &path)
                .unwrap_or_else(|e| panic!("failed to copy {}: {e}", example.display()));
            println!("created {} from .env.example", path.display());
        } else {
            fs::write(&path, "# Generated by `cargo xtask setup`.\n")
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
            println!("created {}", path.display());
        }
    }

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    let mut filled: Vec<&str> = Vec::new();

    // The product was called Scriptorium, and its settings SCRIPTORIUM_*.
    // Renamed in place, values kept: left alone, the secret check below would
    // not see the old JWT secret and would generate a new one, signing out
    // every session and breaking every share link.
    let mut renamed = 0;
    for i in 0..lines.len() {
        let trimmed = lines[i].trim_start().to_string();
        let Some(rest) = trimmed.strip_prefix(LEGACY_ENV_PREFIX) else {
            continue;
        };
        let new_key = format!(
            "{ENV_PREFIX}{}",
            rest.split('=').next().unwrap_or_default().trim()
        );
        // A file that already sets the new name keeps both lines as they are:
        // the new one wins, and the old one is plainly unused.
        if find_assignment(&lines, &new_key).is_none() {
            lines[i] = format!("{ENV_PREFIX}{rest}");
            renamed += 1;
        }
    }
    if renamed > 0 {
        println!("renamed {renamed} {LEGACY_ENV_PREFIX}* setting(s) in .env to {ENV_PREFIX}*");
    }

    for key in REQUIRED_ENV_SECRETS {
        match find_assignment(&lines, key) {
            Some(index) if !is_placeholder(&lines[index], key) => {
                println!(".env already sets {key}");
            }
            Some(index) => {
                lines[index] = format!("{key}={}", random_hex_32());
                filled.push(key);
            }
            None => {
                lines.push(format!("{key}={}", random_hex_32()));
                filled.push(key);
            }
        }
    }

    if filled.is_empty() && renamed == 0 {
        return;
    }
    let mut out = lines.join("\n");
    out.push('\n');
    fs::write(&path, out).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
    if !filled.is_empty() {
        println!("generated {} in {}", filled.join(", "), path.display());
    }
}

/// Index of the line assigning `key`, comments aside.
fn find_assignment(lines: &[String], key: &str) -> Option<usize> {
    lines.iter().position(|line| {
        let line = line.trim_start();
        !line.starts_with('#')
            && line
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

fn is_placeholder(line: &str, key: &str) -> bool {
    let value = line
        .trim_start()
        .strip_prefix(key)
        .and_then(|rest| rest.trim_start().strip_prefix('='))
        .map(|v| unquote(v.trim()))
        .unwrap_or_default();
    value.is_empty()
        || PLACEHOLDER_PREFIXES
            .iter()
            .any(|prefix| value.to_ascii_lowercase().starts_with(prefix))
}

fn random_hex_32() -> String {
    let out = Command::new("openssl")
        .args(["rand", "-hex", "32"])
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn `openssl`: {e}"));
    if !out.status.success() {
        eprintln!("`openssl rand -hex 32` failed");
        process::exit(out.status.code().unwrap_or(1));
    }
    String::from_utf8(out.stdout)
        .expect("openssl returned non-UTF-8")
        .trim()
        .to_string()
}

// --- dev -----------------------------------------------------------------

/// Backend and web dev server together, the frontend held back until the
/// backend answers its health endpoint so the first proxied request does not
/// fail against nothing.
fn run_dev(cfg: &Config) {
    let root = &cfg.workspace_root;

    let mut backend = Command::new("cargo")
        .args(["run", "-p", &cfg.backend_package])
        .current_dir(root)
        .spawn()
        .expect("failed to spawn the backend");

    wait_for_backend(cfg, &mut backend);

    let mut frontend = Command::new("pnpm")
        .args(["dev"])
        .current_dir(&cfg.web_dir)
        .spawn()
        .expect("failed to spawn `pnpm dev`");

    // Whichever exits first takes the other down with it, so Ctrl-C never
    // leaves a stray server holding the port.
    loop {
        if let Some(status) = frontend.try_wait().expect("failed to wait on `pnpm dev`") {
            backend.kill().ok();
            process::exit(status.code().unwrap_or(1));
        }
        if let Some(status) = backend.try_wait().expect("failed to wait on the backend") {
            frontend.kill().ok();
            process::exit(status.code().unwrap_or(1));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Blocks until the backend answers with a 200. Exits rather than returning if
/// it dies first or never comes up, so `dev` never leaves a frontend proxying
/// to nothing.
fn wait_for_backend(cfg: &Config, backend: &mut process::Child) {
    let addr = backend_addr(cfg);
    println!(
        "waiting for the backend at http://{addr}{}",
        cfg.health_path
    );

    let deadline = Instant::now() + BACKEND_HEALTH_TIMEOUT;
    loop {
        if let Some(status) = backend.try_wait().expect("failed to wait on the backend") {
            eprintln!("backend exited before it became healthy");
            process::exit(status.code().unwrap_or(1));
        }
        if backend_healthy(&addr, &cfg.health_path) {
            println!("backend is healthy — starting the web dev server");
            return;
        }
        if Instant::now() >= deadline {
            eprintln!(
                "backend was not healthy within {}s, giving up",
                BACKEND_HEALTH_TIMEOUT.as_secs()
            );
            backend.kill().ok();
            process::exit(1);
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// One GET. Every failure — refused connection, timeout, non-200 — is just "not
/// ready yet"; the caller retries.
fn backend_healthy(addr: &str, path: &str) -> bool {
    let Some(sock) = addr.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&sock, Duration::from_secs(2)) else {
        return false;
    };
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    let request = format!("GET {path} HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = Vec::new();
    if stream.read_to_end(&mut response).is_err() {
        return false;
    }
    String::from_utf8_lossy(&response)
        .lines()
        .next()
        .is_some_and(|status| status.contains(" 200 "))
}

/// The address the backend will bind, resolved the way `Config::from_env`
/// resolves it: the environment first, then the repo-root `.env` that
/// `dotenvy` loads, then the default.
fn backend_addr(cfg: &Config) -> String {
    const KEY: &str = "JUSTTYPE_BIND_ADDR";
    if let Some(addr) = env::var(KEY).ok().filter(|a| !a.trim().is_empty()) {
        return addr.trim().to_string();
    }
    fs::read_to_string(cfg.workspace_root.join(".env"))
        .ok()
        .and_then(|contents| env_file_value(&contents, KEY))
        .unwrap_or_else(|| cfg.default_bind_addr.clone())
}

/// Value `key` is assigned in a `.env`, if any. Deliberately minimal: plain
/// `KEY=value` and the quoted forms, nothing more.
fn env_file_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let line = line.trim_start();
        if line.starts_with('#') {
            return None;
        }
        let value = unquote(
            line.strip_prefix(key)?
                .trim_start()
                .strip_prefix('=')?
                .trim(),
        );
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
}

// --- docker --------------------------------------------------------------

/// The image `cargo xtask docker` builds, its container and its data volume.
/// Named apart from anything published, so a local run never clobbers a pulled
/// `ghcr.io/…:latest` and never touches a real deployment's data.
const DOCKER_IMAGE: &str = "justtype-studio:local";
const DOCKER_CONTAINER: &str = "justtype-local";
const DOCKER_VOLUME: &str = "justtype-local-data";
/// Where the image listens inside the container (the Dockerfile's EXPOSE).
const DOCKER_INTERNAL_PORT: u16 = 8080;

struct DockerOptions {
    port: u16,
    build: bool,
    detach: bool,
    fresh: bool,
}

fn docker_options(extra: &[String]) -> DockerOptions {
    let mut options = DockerOptions {
        port: DOCKER_INTERNAL_PORT,
        build: true,
        detach: false,
        fresh: false,
    };
    let mut args = extra.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-build" => options.build = false,
            "--detach" | "-d" => options.detach = true,
            "--fresh" => options.fresh = true,
            "--port" | "-p" => {
                options.port = args.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--port needs a port number");
                    process::exit(1);
                })
            }
            other => {
                eprintln!("Unknown option for docker: {other}\n\n{USAGE}");
                process::exit(1);
            }
        }
    }
    options
}

/// Builds the image from the working tree and runs it the way it runs in
/// production — the same binary, web build, user and volume layout — with
/// only what a local run needs: the repo's JWT secret, and localhost as the
/// public address.
///
/// Deliberately not the whole `.env`: its database and data paths point into
/// this checkout, which does not exist inside the container.
fn run_docker(cfg: &Config, extra: &[String]) {
    let root = &cfg.workspace_root;
    let options = docker_options(extra);
    require_tools(&["docker"]);
    let daemon_up = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !daemon_up {
        eprintln!("Docker is installed but not running — start Docker Desktop and try again.");
        process::exit(1);
    }

    // The same secret as `cargo dev`, so it stays put between runs: a new one
    // would sign out every session in the volume.
    ensure_env(root);
    let secret = env::var("JUSTTYPE_JWT_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            fs::read_to_string(root.join(".env"))
                .ok()
                .and_then(|contents| env_file_value(&contents, "JUSTTYPE_JWT_SECRET"))
        })
        .unwrap_or_else(|| {
            eprintln!("JUSTTYPE_JWT_SECRET is not set in the environment or .env");
            process::exit(1);
        });

    if options.build {
        section("Building the image");
        let commit = git_output(root, &["rev-parse", "HEAD"]).unwrap_or_default();
        let short = commit.get(..7).unwrap_or("dev");
        let version = format!("local-{short}");
        run(
            "docker",
            &[
                "build",
                "-t",
                DOCKER_IMAGE,
                "--build-arg",
                &format!("APP_VERSION={version}"),
                "--build-arg",
                &format!("BUILD_COMMIT={commit}"),
                ".",
            ],
            root,
        );
    }

    // A container left from an earlier run holds the name and the port.
    Command::new("docker")
        .args(["rm", "-f", DOCKER_CONTAINER])
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .ok();
    if options.fresh {
        Command::new("docker")
            .args(["volume", "rm", DOCKER_VOLUME])
            .stdout(process::Stdio::null())
            .stderr(process::Stdio::null())
            .status()
            .ok();
        println!("starting from an empty {DOCKER_VOLUME} volume");
    }

    let url = format!("http://localhost:{}", options.port);
    let publish = format!("127.0.0.1:{}:{DOCKER_INTERNAL_PORT}", options.port);
    let volume = format!("{DOCKER_VOLUME}:/usr/local/data");
    let public_url = format!("JUSTTYPE_PUBLIC_URL={url}");
    let origins = format!("JUSTTYPE_CORS_ORIGINS={url}");
    let mut args: Vec<&str> = vec![
        "run",
        "--rm",
        // The server is PID 1 in the container, and PID 1 ignores Ctrl-C
        // unless it handles it; tini, as --init, passes it on.
        "--init",
        "--name",
        DOCKER_CONTAINER,
        // Localhost only: nothing else on the network should reach it.
        "-p",
        &publish,
        "-v",
        &volume,
        // Named, not `KEY=value`: the value comes from this process's
        // environment, so the secret is not on a command line `ps` can read.
        "-e",
        "JUSTTYPE_JWT_SECRET",
        "-e",
        &public_url,
        "-e",
        &origins,
        "-e",
        "JUSTTYPE_LOG_FORMAT=text",
    ];
    // Summaries, if this machine has a key for them; off otherwise.
    let anthropic_key = env::var("ANTHROPIC_API_KEY").ok().or_else(|| {
        fs::read_to_string(root.join(".env"))
            .ok()
            .and_then(|contents| env_file_value(&contents, "ANTHROPIC_API_KEY"))
    });
    if anthropic_key.is_some() {
        args.extend(["-e", "ANTHROPIC_API_KEY"]);
    }
    if options.detach {
        args.push("--detach");
    }
    args.push(DOCKER_IMAGE);

    section("Running the container");
    let mut command = Command::new("docker");
    command
        .args(&args)
        .current_dir(root)
        .env("JUSTTYPE_JWT_SECRET", &secret);
    if let Some(key) = &anthropic_key {
        command.env("ANTHROPIC_API_KEY", key);
    }
    if options.detach {
        command.stdout(process::Stdio::null());
    }
    let mut container = command
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn `docker run`: {e}"));

    let health = format!("127.0.0.1:{}", options.port);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if backend_healthy(&health, &cfg.health_path) {
            break;
        }
        // In the foreground the run *is* the container; detached, it exits as
        // soon as the container has started, and that is fine.
        if !options.detach {
            if let Some(status) = container
                .try_wait()
                .expect("failed to wait on `docker run`")
            {
                eprintln!("the container exited before it became healthy");
                process::exit(status.code().unwrap_or(1));
            }
        }
        if Instant::now() >= deadline {
            eprintln!(
                "the container was not healthy within 60s — `docker logs {DOCKER_CONTAINER}`"
            );
            process::exit(1);
        }
        thread::sleep(Duration::from_millis(500));
    }

    println!();
    println!("Just Type Studio is running at {url}");
    println!("  data:  the {DOCKER_VOLUME} volume (--fresh to start over)");
    println!("  mail:  docker exec {DOCKER_CONTAINER} cat /usr/local/data/outbox.jsonl");
    if options.detach {
        println!("  logs:  docker logs -f {DOCKER_CONTAINER}");
        println!("  stop:  docker stop {DOCKER_CONTAINER}");
        container.wait().ok();
        return;
    }
    println!("  stop:  Ctrl-C");
    println!();
    let status = container.wait().expect("failed to wait on `docker run`");
    process::exit(status.code().unwrap_or(0));
}

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// --- the rest ------------------------------------------------------------

/// The document comes from the backend binary itself rather than a running
/// server: generating it needs no database, no data directory and no port.
fn write_openapi(cfg: &Config) -> serde_json::Value {
    let out = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "-p",
            &cfg.backend_package,
            "--",
            "--openapi",
        ])
        .current_dir(&cfg.workspace_root)
        .output()
        .unwrap_or_else(|e| panic!("failed to run the backend: {e}"));
    if !out.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        process::exit(out.status.code().unwrap_or(1));
    }
    let document: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("the backend did not print valid JSON: {e}"));
    let mut text = serde_json::to_string_pretty(&document).expect("failed to format the document");
    text.push('\n');
    if let Some(parent) = cfg.openapi_path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&cfg.openapi_path, text)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", cfg.openapi_path.display()));
    document
}

/// Regenerates every client's model types from the document.
///
/// `--check` is what CI runs: it regenerates into memory and fails if anything
/// on disk differs, so a schema change that was not regenerated cannot merge.
fn run_codegen(cfg: &Config, check_only: bool) {
    let document = write_openapi(cfg);
    let schemas = codegen::load(&document).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let outputs = [
        (cfg.ts_types_path.clone(), codegen::typescript(&schemas)),
        (cfg.swift_types_path.clone(), codegen::swift(&schemas)),
        (cfg.kotlin_types_path.clone(), codegen::kotlin(&schemas)),
    ];

    let mut stale = Vec::new();
    for (path, generated) in outputs {
        let generated = generated.unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        });
        let current = fs::read_to_string(&path).unwrap_or_default();
        let name = rel(&cfg.workspace_root, &path);
        if current == generated {
            println!("unchanged  {name}");
            continue;
        }
        if check_only {
            stale.push(name);
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, generated)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
        println!("wrote      {name}");
    }

    if !stale.is_empty() {
        eprintln!(
            "\nerror: generated types are out of date:\n  {}\n\nRun `cargo xtask codegen` and commit the result.",
            stale.join("\n  ")
        );
        process::exit(1);
    }
}

fn build_web(web_dir: &Path) {
    run("pnpm", &["install", "--prefer-offline"], web_dir);
    run("pnpm", &["build"], web_dir);
}

/// The Xcode project is generated rather than checked in, so this regenerates
/// before opening — a target added to project.yml shows up without anyone
/// having to remember the extra step.
fn open_ios(cfg: &Config) {
    require_tools(&["xcodegen"]);
    write_signing_xcconfig(&cfg.workspace_root);
    run("xcodegen", &["generate"], &cfg.ios_dir);
    run("open", &[cfg.xcode_project.as_str()], &cfg.ios_dir);
}

fn run_tests(cfg: &Config) {
    let root = &cfg.workspace_root;

    section("Rust");
    run("cargo", &["test", "-p", &cfg.backend_package], root);

    section("Swift");
    if have_tool("swift") {
        run("swift", &["test"], &cfg.ios_package_dir);
    } else {
        println!("swift not found — skipping the iOS package tests");
    }

    section("Android");
    if have_tool("java") {
        run(
            "./gradlew",
            &["--console=plain", ":core:test"],
            &cfg.android_dir,
        );
    } else {
        println!("java not found — skipping the Android core tests");
    }

    section("Web");
    run("pnpm", &["install", "--prefer-offline"], &cfg.web_dir);
    run("pnpm", &["typecheck"], &cfg.web_dir);
}

/// Builds the debug app and installs it on whatever `adb` sees. Production by
/// default, as the iOS app is; `--local` is the backend on this computer as
/// the emulator reaches it (10.0.2.2), `--api` anything else.
fn run_android(cfg: &Config, extra: &[String]) {
    let mut api = None;
    let mut args = extra.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--local" => {
                api = Some(format!(
                    "http://10.0.2.2:{}/api",
                    port_of(&cfg.default_bind_addr)
                ))
            }
            "--api" => api = args.next().cloned(),
            other => {
                eprintln!("Unknown option for android: {other}");
                process::exit(1);
            }
        }
    }
    let property = api.map(|url| format!("-PapiBaseUrl={url}"));
    let mut args = vec!["--console=plain", ":app:installDebug"];
    if let Some(property) = &property {
        args.push(property);
    }
    gradle(&cfg.android_dir, &args);
    run(
        "adb",
        &[
            "shell",
            "am",
            "start",
            "-n",
            "studio.justtype.app/.MainActivity",
        ],
        &cfg.android_dir,
    );
}

/// Gradle 8 runs on JDK 17 to 24, and the `java` on a Mac's PATH is often newer
/// (Homebrew's). Without a JAVA_HOME of the caller's own, macOS is asked for a 17.
fn gradle(dir: &Path, args: &[&str]) {
    let mut command = Command::new("./gradlew");
    command.args(args).current_dir(dir);
    if env::var_os("JAVA_HOME").is_none() {
        if let Ok(out) = Command::new("/usr/libexec/java_home")
            .args(["-v", "17"])
            .output()
        {
            if out.status.success() {
                command.env("JAVA_HOME", String::from_utf8_lossy(&out.stdout).trim());
            }
        }
    }
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn gradlew: {e}"));
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

fn port_of(addr: &str) -> &str {
    addr.rsplit(':').next().unwrap_or("8088")
}

/// What CI enforces, runnable before pushing.
fn run_checks(cfg: &Config) {
    let root = &cfg.workspace_root;

    section("Formatting");
    run("cargo", &["fmt", "--all", "--", "--check"], root);

    section("Clippy");
    run(
        "cargo",
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        root,
    );

    section("Generated types");
    run_codegen(cfg, true);

    section("Web types");
    run("pnpm", &["install", "--prefer-offline"], &cfg.web_dir);
    run("pnpm", &["typecheck"], &cfg.web_dir);
}

/// The signing team lives in scripts/.env, not in project.yml, and reaches
/// xcodebuild and Xcode through a generated xcconfig. xcodegen refuses to
/// generate a project whose configFiles path is missing, so this runs first.
fn write_signing_xcconfig(root: &Path) {
    run("bash", &["scripts/ios_signing.sh"], root);
}

fn run_smoke(root: &Path, extra: &[String]) {
    run_script(root, "scripts/smoke.sh", extra);
}

/// Everything after the task name is handed to the script, so
/// `cargo sim --physical --console` reaches the script with both flags.
fn run_script(root: &Path, script: &str, extra: &[String]) {
    let mut args = vec![script.to_string()];
    args.extend_from_slice(extra);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run("bash", &args, root);
}

// --- plumbing ------------------------------------------------------------

/// A virtual workspace has no root package, so the settings live under
/// `[workspace.metadata.justtype]` rather than `[package.metadata]`.
fn config_from_metadata() -> Config {
    let metadata = cargo_metadata::MetadataCommand::new()
        .no_deps()
        .exec()
        .expect("cargo metadata failed");

    let settings = &metadata.workspace_metadata["justtype"];
    let get = |key: &str| -> String {
        settings[key]
            .as_str()
            .unwrap_or_else(|| panic!("[workspace.metadata.justtype] {key} missing"))
            .to_string()
    };

    let root = metadata.workspace_root.as_std_path().to_path_buf();
    Config {
        backend_package: get("backend_package"),
        web_dir: root.join(get("web_dir")),
        ios_dir: root.join(get("ios_dir")),
        ios_package_dir: root.join(get("ios_package_dir")),
        android_dir: root.join(get("android_dir")),
        xcode_project: get("xcode_project"),
        default_bind_addr: get("default_bind_addr"),
        health_path: get("health_path"),
        openapi_path: root.join(get("openapi_path")),
        ts_types_path: root.join(get("ts_types_path")),
        swift_types_path: root.join(get("swift_types_path")),
        kotlin_types_path: root.join(get("kotlin_types_path")),
        workspace_root: root,
    }
}

fn section(title: &str) {
    println!("\n== {title} ==");
}

/// Reports every missing tool at once, rather than one per re-run.
fn require_tools(tools: &[&str]) {
    let missing: Vec<&str> = tools.iter().copied().filter(|t| !have_tool(t)).collect();
    if missing.is_empty() {
        return;
    }
    eprintln!("Missing required tools: {}", missing.join(", "));
    if missing.contains(&"pnpm") {
        eprintln!("  pnpm: `corepack enable` picks up the pinned version.");
    }
    if missing.contains(&"xcodegen") {
        eprintln!("  xcodegen: `brew install xcodegen`.");
    }
    eprintln!("Install them and try again.");
    process::exit(1);
}

fn have_tool(tool: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {tool}")])
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn run(cmd: &str, args: &[&str], dir: &Path) {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn `{cmd}`: {e}"));
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}
