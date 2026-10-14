//! A ticket for the live channel goes to whoever the token says, or to nobody
//! when there is no token — but never to nobody because a token has lapsed.
use super::prompts_api_tests::{Server, Writer};

#[tokio::test]
async fn a_lapsed_token_is_refused_a_ticket_rather_than_given_an_anonymous_one() {
    let server = Server::start().await;
    let eve = server.writer("eve").await;

    let ticket = server.ok(&eve, "POST", "/live/ticket", None).await;
    assert!(ticket["ticket"].is_string());
    assert_eq!(server.status(None, "POST", "/live/ticket", None).await, 200);

    // Read as nobody, it would be turned away from a private review's room
    // without a word; refused, the client refreshes and asks again.
    let lapsed = Writer {
        id: eve.id.clone(),
        token: "lapsed".into(),
    };
    assert_eq!(
        server
            .status(Some(&lapsed), "POST", "/live/ticket", None)
            .await,
        401
    );
}
