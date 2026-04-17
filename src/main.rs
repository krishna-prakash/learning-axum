
use std::{collections::HashMap, sync::{Arc, RwLock}, time::Instant};

use axum::{Json, Router, extract::{FromRequest, Path, Query, Request, State}, http::{HeaderMap, StatusCode}, middleware::{self, Next}, response::{IntoResponse, Response}, routing::{get, post}};
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use uuid::Uuid;


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

#[derive(Debug, Serialize)]
struct CreateUserResponse {
    id: u64,
    name: String,
    email: String,
}

async fn create_user(ValidateJson(payload): ValidateJson<ValidateUser>) -> Json<CreateUserResponse> {
    let response = CreateUserResponse {
        id: 1,
        name: payload.name,
        email: payload.email,
    };
    Json(response)
}

#[derive(Clone)]
struct AppState {
    db_pool: String, // Placeholder for a database connection pool
    api_version: String,
}

async fn with_state(State(state): State<Arc<AppState>>) -> String {
    format!("API Version: {}, DB Pool: {}", state.api_version, state.db_pool)
}

#[derive(Clone, Serialize)]
struct Config {
    app_name: String,
    app_version: String,
}

async fn get_config(State(config): State<Arc<Config>>) -> Json<Config> {
    Json(config.as_ref().clone())
}

// Todo implementation with in memeory db with hashmaps

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Todo {
    id: String,
    title: String,
    completed: bool,
}

#[derive(Debug, Deserialize)]
struct CreateTodo {
    title: String,
}

type TodoStore = Arc<RwLock<HashMap<String, Todo>>>;

async fn create_todo(
    State(store): State<TodoStore>,
    Json(todo): Json<CreateTodo>
) {
   let todo = Todo {
        id: Uuid::new_v4().to_string(), // Generate a unique ID for the todo
        title: todo.title,
        completed: false,
   };
    store.write().unwrap().insert(todo.id.to_string(), todo);
}

async fn list_todos(
    State(store): State<TodoStore>
) -> Json<Vec<Todo>> {
    let todos = store.read().unwrap();
    let todo_list: Vec<Todo> = todos.values().cloned().collect();
    Json(todo_list)
}

// DB connection example
#[allow(dead_code)]
struct DbPool {
    connection_string: String,
    max_connections: u32,
}

impl DbPool {
    fn new(connection_string: &str) -> Self {
        Self {
            connection_string: connection_string.to_string(),
            max_connections: 10,
        }
    }

    async fn query(&self, _sql: &str) -> Result<Vec<String>, String> {
        // Simulate a database query
        Ok(vec!["Result 1".to_string(), "Result 2".to_string()])
    }
}

async fn db_query(State(pool): State<Arc<DbPool>>) -> Json<Vec<String>> {
    match pool.query("SELECT * FROM my_table").await {
        Ok(results) => Json(results),
        Err(e) => Json(vec![format!("Error: {}", e)]),
    }
}

// Tracing middleware example

async fn with_tracing(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();

    let response = next.run(request).await;

    tracing::info!(
        method = %method,
        uri = %uri,
        status = %response.status().as_u16(),
        duration_ms = %start.elapsed().as_millis(),
        "Request completed"
    );
    response
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();
    let state = Arc::new(AppState {
        db_pool: "Database Connection Pool".to_string(),
        api_version: "v1".to_string(),
    });

    let config = Arc::new(Config {
        app_name: "My Axum App".to_string(),
        app_version: "1.0.0".to_string(),
    });

    // initialize the in-memory todo store
    let todo_store: TodoStore = Arc::new(RwLock::new(HashMap::new()));

    // dummy db connection pool
    let db_pool = Arc::new(DbPool::new("postgres://user:password@localhost/db"));

    let app = Router::new()
        .route("/", get(hello_world))
        .route("/health", get(health_check))
        .route("/resource/{id}", get(get_resource_by_id))
        // .nest("/api/v1", api_v1())
        .route("/multiple_headers/{id}", post(multiple_headers))
        .route("/validated_user", post(create_validated_user))
        .route("/users", post(create_user))
        .route("/state", get(with_state))
        .fallback(not_found)
        .with_state(state)
        .route("/config", get(get_config))
        .with_state(config)
        .route("/todos", post(create_todo).get(list_todos))
        .with_state(todo_store)
        .route("/db_query", get(db_query))
        .with_state(db_pool)
        .layer(middleware::from_fn(with_tracing))
        .layer(
            ServiceBuilder::new().layer(TraceLayer::new_for_http())
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.expect("Failed to bind to address");
    
    axum::serve(listener, app).await.expect("Failed to start server");
}
