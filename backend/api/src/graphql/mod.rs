pub mod schema;
pub mod types;
pub mod loaders;

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use crate::state::AppState;
use crate::graphql::schema::RegistrySchema;

pub async fn graphql_handler(
    State(schema): State<RegistrySchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

pub async fn graphql_playground() -> impl IntoResponse {
    Html(playground_source(GraphQLPlaygroundConfig::new("/api/graphql")))
}
