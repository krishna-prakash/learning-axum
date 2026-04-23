
use std::{collections::HashMap, sync::{Arc, RwLock}, time::{Duration, Instant}};

use argon2::{Argon2, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{Json, Router, extract::{FromRequest, Multipart, Path, Query, Request, State, WebSocketUpgrade, ws::{Message, WebSocket}}, http::{HeaderMap, StatusCode}, middleware::{self, Next}, response::{IntoResponse, Response}, routing::{get, post}};
use ::chrono::Utc;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, postgres::PgPoolOptions, types::chrono};
use thiserror::Error;
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


async fn create_validated_user(ValidateJson(user): ValidateJson<ValidateUser>) -> String {
    format!("Validated user created: Name: {}, Email: {}", user.name, user.email)
}

#[derive(Debug, Serialize)]
struct CreateUserResponse {
    id: u64,
    name: String,
    email: String,
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

// Error handling
#[derive(Error, Debug)]
enum AppError {
    #[error("User not found with ID: {0}")]
    UserNotFound(u64),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Unauthorized access")]
    Unauthorized,
    #[error("Internal server error")]
    InternalError,
}

#[derive(Serialize, Debug)]
struct ErrorResponse {
    message: String,
    code: u16,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::UserNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::InvalidInput(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::DatabaseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::InternalError => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let error_response = ErrorResponse {
            message,
            code: status.as_u16(),
        };
        (status, Json(error_response)).into_response()
    }
}

#[derive(Serialize, sqlx::FromRow, sqlx::Decode, Debug)]
struct User {
    id: Uuid,
    name: String,
    email: String,
    password_hash: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
struct UpdateUser {
    name: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
// A wrapper type to validate JSON payloads
struct ValidateJson<T>(T);

#[derive(Debug, Deserialize)]
struct ValidateUser {
    name: String,
    email: String,
    password: String,
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

async fn private_route(axum::Extension(current_user): axum::Extension<CurrentUser>) -> impl IntoResponse {
    Json(serde_json::json!({
        "message": format!("Hello, {}! You have accessed a private route.", current_user.id),
        "role": current_user.role,
    }))
}

// Databse error handling example
#[derive(Error, Debug)]
enum DbError {
    #[error("User not found")]
    NotFound,
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

impl IntoResponse for DbError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            DbError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            DbError::Sqlx(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

       
        (status, message).into_response()
    }
}

async fn get_users(State(state): State<CombinedState>) -> Result<Json<Vec<User>>, DbError> {
    let results = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(results))
}

async fn create_user(State(state): State<CombinedState>, ValidateJson(payload): ValidateJson<ValidateUser>) -> Json<User> {
     let user  = sqlx::query_as::<_, User>("INSERT INTO users (id, name, email, password_hash, created_at) VALUES ($1, $2, $3, $4, NOW()) RETURNING *")
        .bind(Uuid::new_v4()) // Generate a new UUID for the user
        .bind(&payload.name)
        .bind(&payload.email)
        .bind(hash_password(&payload.password)) // Hash the password before storing
        .fetch_one(&state.pool)
        .await
        .expect("Failed to create user");
    Json(user)
}

async fn get_user_by_id(State(state): State<CombinedState>, Path(id): Path<Uuid>) -> Result<Json<User>, DbError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => DbError::NotFound,
            other => DbError::Sqlx(other),
        })?;

    Ok(Json(user))
}

async fn udpate_user(State(state): State<CombinedState>, Path(id): Path<Uuid>, Json(payload): Json<UpdateUser>) -> Result<Json<User>, DbError> {
    let mut qb = QueryBuilder::<Postgres>::new("UPDATE users SET ");

    if let Some(name) = &payload.name {
        qb.push("name = ").push_bind(name).push(", ");
    }
    if let Some(email) = &payload.email {
        qb.push("email = ").push_bind(email).push(", ");
    }
    qb.push("created_at = NOW() WHERE id = ").push_bind(id).push(" RETURNING *");

    let query = qb.build_query_as::<User>();
    let result  = query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => DbError::NotFound,
            other => DbError::Sqlx(other),
        });

        Ok(Json(result?))   
}    

async fn delete_user(State(state): State<CombinedState>, Path(id): Path<Uuid>) -> Result<StatusCode, DbError> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| DbError::Sqlx(e))?;

    if result.rows_affected() == 0 {
        Err(DbError::NotFound)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

// Auth 
// Password hashing example using argon2

#[derive(Clone, Debug)]
struct CombinedState {
    config: Arc<AuthConfig>,
    pool: PgPool,
}

#[derive(Clone, Debug)]
struct AuthConfig {
    jwt_secret: String,
    jwt_expiry_in_hours: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
    role: String,
}

fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("Failed to hash password")
        .to_string()
}

fn verify_password(password: &str, hash: &str) -> bool {
    let parsed_hash = argon2::PasswordHash::new(hash).expect("Invalid hash format");
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    token: String,
    expiry: i64,
}

#[derive(Debug, Clone)]
struct CurrentUser {
    id: String,
    role: String,
}

async fn login(State(state): State<CombinedState>, Json(payload): Json<LoginRequest>) -> Result<Json<LoginResponse>, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(&payload.email)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::Unauthorized,
            _ => AppError::DatabaseError(e.to_string()),
        })?;

        if verify_password(&payload.password, &user.password_hash) {
            let token = create_jwt(user.id, &state.config).map_err(|_| AppError::InternalError)?;
            Ok(Json(LoginResponse {
                token,
                expiry: state.config.jwt_expiry_in_hours as i64 * 3600,
            }))
        } else {
            Err(AppError::Unauthorized)
        }
}


fn create_jwt(user_id: Uuid, config: &AuthConfig) -> Result<String, StatusCode> {
    let expiry = Utc::now()  + Duration::from_hours(config.jwt_expiry_in_hours);
    let claims = Claims {
        sub: user_id.to_string(),
        exp: expiry.timestamp() as usize,
        role: "user".to_string(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn verify_jwt(token: &str, config: &AuthConfig) -> Result<Claims, StatusCode> {
    jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| StatusCode::UNAUTHORIZED)
}

// Auth middleware
async fn auth_middleware(
    State(config): State<Arc<AuthConfig>>,
    mut req: Request,
    next: Next
) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let token = auth_header.ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = verify_jwt(token, &config)?;

    let current_user = CurrentUser {
        id: claims.sub, 
        role: claims.role,
    };
    req.extensions_mut().insert(current_user);
    Ok(next.run(req).await)
    
}

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_websocket)
}

async fn handle_websocket(mut socket: WebSocket) {
    while let Some(msg) = socket.recv().await {
       if let Ok(Message::Text(text)) = msg {
            let response = format!("Received message: {}", text);
            if socket.send(Message::Text(response.into())).await.is_err() {
                break;
            }
       }
    }
}

async fn file_upload_handler(mut multipart: Multipart) -> impl IntoResponse {
    let mut files = Vec::new();

    while let Some(multipart_field) = multipart.next_field().await.unwrap() {
        let name = multipart_field.name().unwrap_or("file").to_string();
        let data = multipart_field.bytes().await.unwrap();
        files.push(format!("{} :{} bytes", name, data.len()));
    }

    if files.is_empty() {
        "No files uploaded".to_string()
    } else {
        format!("Uploaded file:{}", files.join(", "))
    }
}

async fn create_app(db_url: String) -> Router {

    let config = Arc::new(AuthConfig {
        jwt_secret: "supersecretkey".to_string(),
        jwt_expiry_in_hours: 24,
    });

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to create database pool");

    let combined_state = CombinedState {
        pool: pool.clone(),
        config: config.clone(),
    };

      // Run Migrations
    sqlx::query("CREATE TABLE IF NOT EXISTS users (id UUID PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at TIMESTAMPTZ DEFAULT NOW())")
        .execute(&pool)
        .await
        .expect("Failed to run migrations");
    
    let user_routes = Router::new()
        .route("/", post(create_user).get(get_users))
        .route("/{id}", get(get_user_by_id).put(udpate_user).delete(delete_user));

    let private_routes = Router::new()
        .route("/", get(private_route))
        .layer(middleware::from_fn_with_state(config.clone(), auth_middleware));


    Router::new()
        .route("/", get(hello_world))
        .route("/health", get(health_check))
        .route("/resource/{id}", get(get_resource_by_id))
        // .nest("/api/v1", api_v1())
        .route("/multiple_headers/{id}", post(multiple_headers))
        .route("/validated_user", post(create_validated_user))
        .route("/login", post(login))
        .merge(Router::new().nest("/users", user_routes))
        // .route("/state", get(with_state))
        .fallback(not_found)
        // .with_state(state)
        // .route("/config", get(get_config))
        // .with_state(config)
        // .route("/todos", post(create_todo).get(list_todos))
        // .with_state(todo_store)
        // .route("/db_query", get(db_query))
        // .with_state(db_pool)
        // .route("/users/{id}", get(get_user))
        .merge(Router::new().nest("/private", private_routes))
        .route("/ws", get(ws_handler))
        .route("/upload", post(file_upload_handler))
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .with_state(combined_state)
        .layer(middleware::from_fn(with_tracing))
        .layer(
            ServiceBuilder::new().layer(TraceLayer::new_for_http())
        )

}

#[tokio::main]
async fn main() {

    std::fs::create_dir_all("static").ok();
    if !std::path::Path::new("static/hello.txt").exists() {
        std::fs::write("static/hello.txt", "Hello, World!").ok();
    }
    
    dotenv::dotenv().ok();
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    let db_url = std::env::var("DATABASE_URL").expect("not able to get the url"); 
    // let state = Arc::new(AppState {
    //     db_pool: "Database Connection Pool".to_string(),
    //     api_version: "v1".to_string(),
    // });

    // let config = Arc::new(Config {
    //     app_name: "My Axum App".to_string(),
    //     app_version: "1.0.0".to_string(),
    // });

    // initialize the in-memory todo store
    // let todo_store: TodoStore = Arc::new(RwLock::new(HashMap::new()));

    // dummy db connection pool
    // let db_pool = Arc::new(DbPool::new("postgres://user:password@localhost/db"));

    // create files

    let app = create_app(db_url).await;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.expect("Failed to bind to address");
    
    axum::serve(listener, app).await.unwrap();
}


#[cfg(test)]
mod tests {
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn test_health_check_returns_ok() {
        dotenv::dotenv().ok(); 
        
        let db_url = std::env::var("DATABASE_URL").expect("not able to get the url"); 
        let app = create_app(db_url).await;
        
        let response = app.oneshot(
            axum::http::Request::builder()
            .method("GET")
                .uri("/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();  

        assert_eq!(response.status(), StatusCode::OK);   
    }
}