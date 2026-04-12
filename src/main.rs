use axum::{Router, http::StatusCode, routing::get};


async fn hello_world() -> (StatusCode, &'static str) {
    (StatusCode::OK, "Hello, World!")
}

async fn health_check() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(hello_world))
        .route("/health", get(health_check));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.expect("Failed to bind to address");
    
    axum::serve(listener, app).await.expect("Failed to start server");
}
