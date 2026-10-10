//! The theme is kept with the account, so every device reads the same one.
use serde_json::json;

use super::prompts_api_tests::Server;

#[tokio::test]
async fn the_theme_is_kept_with_the_account() {
    let server = Server::start().await;
    let eve = server.writer("eve").await;

    let me = server.ok(&eve, "GET", "/auth/me", None).await;
    assert_eq!(me["theme"], "system", "follows the device until chosen");

    let saved = server
        .ok(
            &eve,
            "PATCH",
            "/me/settings",
            Some(json!({ "theme": "sepia" })),
        )
        .await;
    assert_eq!(saved["theme"], "sepia");
    let me = server.ok(&eve, "GET", "/auth/me", None).await;
    assert_eq!(me["theme"], "sepia");

    // Changing another setting leaves it alone.
    server
        .ok(
            &eve,
            "PATCH",
            "/me/settings",
            Some(json!({ "doubleEnterSplitsScenes": true })),
        )
        .await;
    let me = server.ok(&eve, "GET", "/auth/me", None).await;
    assert_eq!(me["theme"], "sepia");

    let refused = server
        .status(
            Some(&eve),
            "PATCH",
            "/me/settings",
            Some(json!({ "theme": "neon" })),
        )
        .await;
    assert_eq!(refused, 400);
    let me = server.ok(&eve, "GET", "/auth/me", None).await;
    assert_eq!(me["theme"], "sepia");
}
