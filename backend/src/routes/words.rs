//! The words around the writing: a thesaurus, and the writer's own additions
//! to the spellchecker's dictionary.
//!
//! The spellchecker itself runs in the client, against a dictionary it loads
//! once; nothing a writer types is sent anywhere to be checked. What lives
//! here is the part that has to follow the writer between devices — the names
//! and coinages they have told it are right.
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::schema::dictionary_words;
use crate::domain::now_iso;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;
use crate::thesaurus::{self, ThesaurusEntry};

/// A book's worth of invented names, several times over.
const MAX_WORDS: i64 = 5000;

#[utoipa::path(
    get,
    path = "/api/thesaurus/{word}",
    tag = "words",
    params(("word" = String, Path)),
    responses(
        (status = 200, description = "The word's meanings, and the words for each", body = ThesaurusEntry),
        (status = 4XX, description = "Not a word the thesaurus has", body = ErrorEnvelope),
    )
)]
pub async fn lookup(_user: AuthUser, Path(word): Path<String>) -> ApiResult<Json<ThesaurusEntry>> {
    // The first lookup unpacks the whole thesaurus; that is not work for the
    // async threads.
    let entry = tokio::task::spawn_blocking(move || thesaurus::lookup(&word))
        .await
        .map_err(|e| ApiError::internal(format!("the thesaurus broke off: {e}")))?;
    entry
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("the thesaurus has nothing for that word".into()))
}

/// A word as the dictionary keeps it, or why it will not.
fn dictionary_word(word: &str) -> ApiResult<String> {
    let word = word.trim().replace('’', "'");
    let fits = !word.is_empty()
        && word.chars().count() <= thesaurus::MAX_WORD
        && word.chars().any(char::is_alphabetic)
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || c == '\'' || c == '-');
    if fits {
        Ok(word)
    } else {
        Err(ApiError::BadRequest(
            "a dictionary word is one word: letters, and apostrophes or hyphens inside it".into(),
        ))
    }
}

#[utoipa::path(
    get,
    path = "/api/me/dictionary",
    tag = "words",
    responses(
        (status = 200, description = "The words you have added, in order", body = Vec<String>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<String>>> {
    let words = blocking(&state.pool, move |conn| {
        Ok(dictionary_words::table
            .filter(dictionary_words::user_id.eq(&user.id))
            .order(dictionary_words::word.asc())
            .select(dictionary_words::word)
            .load::<String>(conn)?)
    })
    .await?;
    Ok(Json(words))
}

/// Tell the spellchecker a word is right. Adding one already there is fine.
#[utoipa::path(
    put,
    path = "/api/me/dictionary/{word}",
    tag = "words",
    params(("word" = String, Path)),
    responses(
        (status = 200, description = "The words you have added, now", body = Vec<String>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add(
    State(state): State<AppState>,
    user: AuthUser,
    Path(word): Path<String>,
) -> ApiResult<Json<Vec<String>>> {
    let word = dictionary_word(&word)?;
    let uid = user.id.clone();
    blocking(&state.pool, move |conn| {
        let count: i64 = dictionary_words::table
            .filter(dictionary_words::user_id.eq(&uid))
            .count()
            .get_result(conn)?;
        if count >= MAX_WORDS {
            return Err(ApiError::BadRequest(format!(
                "your dictionary holds {MAX_WORDS} words at most — take some out first"
            )));
        }
        diesel::insert_or_ignore_into(dictionary_words::table)
            .values((
                dictionary_words::user_id.eq(&uid),
                dictionary_words::word.eq(&word),
                dictionary_words::created_at.eq(now_iso()),
            ))
            .execute(conn)?;
        Ok(())
    })
    .await?;
    list(State(state), user).await
}

/// Take a word back out. Taking out one that is not there is fine.
#[utoipa::path(
    delete,
    path = "/api/me/dictionary/{word}",
    tag = "words",
    params(("word" = String, Path)),
    responses(
        (status = 200, description = "The words you have added, now", body = Vec<String>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(word): Path<String>,
) -> ApiResult<Json<Vec<String>>> {
    let word = word.trim().replace('’', "'");
    let uid = user.id.clone();
    blocking(&state.pool, move |conn| {
        diesel::delete(
            dictionary_words::table
                .filter(dictionary_words::user_id.eq(&uid))
                .filter(dictionary_words::word.eq(&word)),
        )
        .execute(conn)?;
        Ok(())
    })
    .await?;
    list(State(state), user).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dictionary_word_is_one_word() {
        assert_eq!(dictionary_word(" Ysolde ").unwrap(), "Ysolde");
        assert_eq!(dictionary_word("o’er").unwrap(), "o'er");
        assert!(dictionary_word("half-elf").is_ok());
        assert!(dictionary_word("two words").is_err());
        assert!(dictionary_word("1984").is_err());
        assert!(dictionary_word("").is_err());
        assert!(dictionary_word("<script>").is_err());
    }
}
