//! Hi5-Bot daemon entry point.
//!
//! Usage:
//!   hi5bot                        # long-running: cron loop + web server
//!   hi5bot --once                 # run a single evaluation tick now and exit
//!   hi5bot --once --dry-run       # compute but do not place orders
//!   hi5bot --web-only             # serve the dashboard API only (no trading)
//!
//! Data directory: `$HI5BOT_DATA_DIR` or `./data` (holds config.toml,
//! tokens.json, hi5bot.db, state.json). On the Synology container this is
//! the mounted `/app/data` volume.
//!
//! Web server: listens on `$HI5BOT_BIND` (default `0.0.0.0:8080`).

use std::sync::Arc;

use chrono::Utc;
use chrono_tz::America::Toronto;
use hi5bot::{Error, accounts, auth::TokenStore, calendar, config, db, engine, state_store::MonthlyStateStore, web};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let once = args.iter().any(|a| a == "--once");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let web_only = args.iter().any(|a| a == "--web-only");

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("hi5bot [--once] [--dry-run] [--web-only]");
        return Ok(());
    }

    // Open (or create) the SQLite database.
    let database = db::Database::open(db::db_path())?;
    let shared_state = Arc::new(web::AppState::new(database));

    // Load config + tokens (needed even in web-only mode for API context).
    let settings = config::Settings::load()?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let tokens = TokenStore::load(config::tokens_path())?;
    let state_store = MonthlyStateStore::load(config::state_path())?;

    let (eval_h, eval_m) = calendar::parse_eval_time(&settings.eval_time)
        .ok_or_else(|| anyhow::anyhow!("invalid eval_time '{}'", settings.eval_time))?;

    // ---- Account discovery via Questrade API ----
    tokens.ensure_valid(&http, &settings.token_url, chrono::Utc::now().naive_utc()).await?;
    let qt = hi5bot::questrade::QuestradeClient::new(
        http.clone(),
        tokens.api_server(),
        tokens.access_token(),
    );
    let discovered = accounts::discover(&qt, &settings.account_types).await?;
    tracing::info!(
        "loaded config; accounts={} ({:?}); eval={:02}:{:02} America/Toronto; M={}",
        discovered.len(),
        discovered.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(),
        eval_h,
        eval_m,
        settings.safety_buffer_m
    );

    // Bind address for the web server.
    let bind_addr = std::env::var("HI5BOT_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    // ---- Web-only mode: serve API, skip trading loop ----
    if web_only {
        tracing::info!("web-only mode: starting API server on {bind_addr}");
        let router = web::build_router(shared_state);
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        tracing::info!("dashboard API listening on http://{bind_addr}");
        axum::serve(listener, router).await?;
        return Ok(());
    }

    // ---- Once mode: single tick, then exit ----
    if once {
        let now = Utc::now().with_timezone(&Toronto);
        match engine::run_tick(
            &settings,
            &tokens,
            &state_store,
            Some(&shared_state.db),
            &http,
            now.naive_local(),
            dry_run,
            &discovered,
        )
        .await
        {
            Ok(summaries) => {
                for s in &summaries {
                    tracing::info!(
                        "account={} signal={:?} orders_placed={}",
                        s.account,
                        s.signal,
                        s.orders_placed
                    );
                }
            }
            Err(e) => {
                tracing::error!("tick failed: {e:?}");
                return Err(anyhow::anyhow!(e.to_string()));
            }
        }
        return Ok(());
    }

    // ---- Long-running mode: cron loop + web server ----
    // Spawn the web server on a background task.
    let web_state = shared_state.clone();
    let bind = bind_addr.clone();
    tokio::spawn(async move {
        let router = web::build_router(web_state);
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("failed to bind web server on {bind}: {e}");
                return;
            }
        };
        tracing::info!("dashboard API listening on http://{bind}");
        if let Err(e) = axum::serve(listener, router).await {
            tracing::error!("web server error: {e}");
        }
    });

    loop {
        let now = Utc::now().with_timezone(&Toronto);
        let next = calendar::next_eval(now, eval_h, eval_m)
            .ok_or_else(|| anyhow::anyhow!("could not compute next eval time"))?;
        let dur = (next - now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(60));
        tracing::info!("next eval at {} (sleeping {}s)", next, dur.as_secs());
        tokio::time::sleep(dur).await;

        let now = Utc::now().with_timezone(&Toronto);
        match engine::run_tick(
            &settings,
            &tokens,
            &state_store,
            Some(&shared_state.db),
            &http,
            now.naive_local(),
            dry_run,
            &discovered,
        )
        .await
        {
            Ok(summaries) => {
                for s in &summaries {
                    tracing::info!(
                        "account={} signal={:?} orders_placed={}",
                        s.account,
                        s.signal,
                        s.orders_placed
                    );
                }
            }
            Err(Error::UsdCashExhausted) | Err(Error::SettlementNotCurrencyOfTransaction(_)) => {
                tracing::error!("hard-abort condition; exiting non-zero");
                std::process::exit(1);
            }
            Err(e) => {
                tracing::error!("tick error (will retry next eval): {e:?}");
            }
        }
    }
}
