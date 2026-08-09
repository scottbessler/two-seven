use crate::{app::AppState, render, session::MaybeUser};
use axum::{extract::State, response::Html};
pub async fn healthcheck() -> &'static str {
    "OK"
}
pub async fn index(State(s): State<AppState>, MaybeUser(user): MaybeUser) -> Html<String> {
    let current = match user {
        Some(id) => s.users.get(id).await.map(|u| (id, u.display_name)),
        None => None,
    };
    Html(render::home(current))
}
