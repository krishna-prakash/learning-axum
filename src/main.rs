
use axum::{Json, Router, extract::{FromRequest, Path, Query, Request}, http::{HeaderMap, StatusCode}, response::{IntoResponse, Response}, routing::{get, post}};
use serde::Deserialize;


async fn hello_world() -> (StatusCode, &'static str) {
    (StatusCode::OK, "Hello, World!")
}

async fn health_check() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

async fn get_resource_by_id(Path(id): Path<u64>) -> String {
     format!("Resource ID: {}", id)
}

#[derive(Debug, Deserialize)]
struct Pagination {
    page: Option<u32>,
    limit: Option<u32>,
}

async fn list_items(Query(pagination): Query<Pagination>) -> String {
    let page = pagination.page.unwrap_or(1);
    let limit = pagination.limit.unwrap_or(10);
    format!("Listing items - Page: {}, Limit: {}", page, limit)
}

fn api_v1() -> Router {
    Router::new()
        .route("/items", get(list_items)
            .put(|| async { "Item updated" })
            .delete(|| async { "Item deleted" })
        )
}

async fn not_found() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "Not Found")
}

#[derive(Debug, Deserialize)]
struct Payload {
    name: String,
    value: String,
}

async fn multiple_headers(
    Path(id): Path<u64>,
    Query(pagination): Query<Pagination>,
    headers: HeaderMap,
    Json(payload): Json<Payload>
) -> String { 
    
    format!("ID: {}, Page: {}, Limit: {}, Name: {}, Value: {}, Headers: {:?}", 
        id, 
        pagination.page.unwrap_or(1), 
        pagination.limit.unwrap_or(10), 
        payload.name, 
        payload.value, 
        headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("unknown")
    )
}

#[derive(Debug, Deserialize)]
// A wrapper type to validate JSON payloads
struct ValidateJson<T>(T);

#[derive(Debug, Deserialize)]
struct ValidateUser {
    name: String,
    email: String,
}

#[derive(Debug)]
enum ValidationError {
    InvalaidJson(String),
    InvalidEmail,
    NameTooShort
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ValidationError::InvalaidJson(e) => (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)),
            ValidationError::InvalidEmail => (StatusCode::BAD_REQUEST, "Invalid email format".to_string()),
            ValidationError::NameTooShort => (StatusCode::BAD_REQUEST, "Name must be at least 3 characters long".to_string()),
        };
        (status, message).into_response()
    }
}

// Use axum FromRequest to implement validation logic for the defined wrapper type ValidateJson<T>
impl<S> FromRequest<S> for ValidateJson<ValidateUser>
where
    S: Send + Sync,
{
    type Rejection = ValidationError;
     
    fn from_request(
        req: Request,
        state: &S,
     ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let Json(user): Json<ValidateUser> = Json::from_request(req, state)
            .await
            .map_err(|e| ValidationError::InvalaidJson(e.to_string()))?;


            if !user.email.contains('@') {
                return Err(ValidationError::InvalidEmail);
            }

            if user.name.len() < 3 {
                return Err(ValidationError::NameTooShort);
            }
            
            Ok(ValidateJson(user))   
        }
     }
}

async fn create_validated_user(ValidateJson(user): ValidateJson<ValidateUser>) -> String {
    format!("Validated user created: Name: {}, Email: {}", user.name, user.email)
}



#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(hello_world))
        .route("/health", get(health_check))
        .route("/resource/{id}", get(get_resource_by_id))
        .nest("/api/v1", api_v1())
        .route("/multiple_headers/{id}", post(multiple_headers))
        .route("/validated_user", post(create_validated_user))
        .fallback(not_found);


    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.expect("Failed to bind to address");
    
    axum::serve(listener, app).await.expect("Failed to start server");
}
