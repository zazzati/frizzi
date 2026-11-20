#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    frizzi::init_tracing();
    let pool = frizzi::pool().await;
    let monitoring_pool = pool.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            timer.tick().await;
            frizzi::maintain_observability(&monitoring_pool).await;
        }
    });
    let worker_pool = pool.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = timer.tick() => {},
                _ = frizzi::wait_for_mail() => {},
            }
            if let Err(error) = frizzi::maintain(&worker_pool).await {
                frizzi::log_maintenance_error(&error);
            }
        }
    });
    tokio::spawn(frizzi::monitor_public_urls());
    let address = std::env::var("API_BIND").unwrap_or_else(|_| "127.0.0.1:3001".into());
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .expect("API port unavailable");
    tracing::info!(event = "api_started", address);
    axum::serve(
        listener,
        frizzi::router(pool).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap();
}
